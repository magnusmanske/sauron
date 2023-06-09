use std::{net::SocketAddr, sync::Arc, collections::HashMap};
use async_session::SessionStore;
use serde_json::{Value, json};
use axum::{
    routing::get,
    Router,
    http::StatusCode,
    Server,
    extract::{State,Query, Path}, response::{Redirect, IntoResponse}, TypedHeader, Json,
};
use http::{header::{ACCEPT, CONTENT_TYPE, SET_COOKIE}, HeaderMap};
use tower_http::{services::ServeDir, trace::TraceLayer, compression::CompressionLayer};
use crate::error::RingError;
use crate::app_state::AppState;
use crate::external_system::*;

pub mod error;
pub mod app_state;
pub mod database_session_store;
pub mod external_system;
pub mod entity;


async fn redirect_to_orcid(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let redirect_url = format!("{}/redirect/orcid",state.get_redirect_server());
    let client_id = match state.config["systems"]["orcid"]["client_id"].as_str() {
        Some(id) => id,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let url = format!("https://orcid.org/oauth/authorize?client_id={client_id}&response_type=code&scope=/authenticate&redirect_uri={redirect_url}");
    Ok(Redirect::to(&url))
}

async fn auth_info(State(state): State<Arc<AppState>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let user = ExternalSystemUser::from_cookies(&state, &cookies).await;
    let j = json!({"status":"OK","user":user});
    (StatusCode::OK, Json(j))
}

async fn user_logout(State(state): State<Arc<AppState>>, cookies: TypedHeader<headers::Cookie>,) -> impl IntoResponse {
    let cookie = cookies.get(COOKIE_NAME).unwrap().to_string();
    if let Some(session) = state.store.load_session(cookie).await.unwrap() {
        state.store.destroy_session(session).await.unwrap();
    };
    Redirect::to("/")
}

async fn user_entities(State(state): State<Arc<AppState>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let user = match ExternalSystemUser::from_cookies(&state, &cookies).await {
        Some(user) => user,
        None => return (StatusCode::OK, Json(json!({"status":"not_logged_in"}))),
    };
    let entities = match user.get_entities_with_access(&state).await {
        Ok(entities) => entities,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    let j = json!({"status":"OK","entities":entities.as_sorted_vec()});
    (StatusCode::OK, Json(j))
}

async fn entities(State(state): State<Arc<AppState>>, Path(entity_ids): Path<String>, _cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let entity_ids: Vec<usize> = entity_ids
        .split(',')
        .filter_map(|e|e.parse::<usize>().ok())
        .collect();
    let mut entities = match state.load_entities(&entity_ids).await {
        Ok(entities) => entities,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    if let Err(e) = state.annotate_entities(&mut entities).await {
        return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    };
    let j = json!({"status":"OK","entities":entities.as_sorted_vec()});
    (StatusCode::OK, Json(j))
}


async fn redirect_orcid(State(state): State<Arc<AppState>>, 
    Query(params): Query<HashMap<String, String>>, 
    _cookies: Option<TypedHeader<headers::Cookie>>,
) -> impl IntoResponse {
    println!("A");
    let code = match params.get("code") {
        Some(code) => code,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    println!("B");
    let port = state.config["port"].as_u64().expect("port");
    let redirect_url = format!("{}/redirect/orcid",state.get_redirect_server());
    let client_id = state.config["systems"]["orcid"]["client_id"].as_str().expect("ORCID client_id");
    let client_secret = state.config["systems"]["orcid"]["client_secret"].as_str().expect("ORCID client_secret");
    let body = format!("client_id={client_id}&client_secret={client_secret}&grant_type=authorization_code&code={code}&redirect_uri={redirect_uri}");
    println!("C");

    let j = reqwest::Client::new()
        .post("https://orcid.org/oauth/token")
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?
        .json::<Value>().await
        .map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;

    println!("D: {j}");

    let name = j["name"].as_str()
        .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    println!("E");
    let external_id = j["orcid"].as_str()
        .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();

    println!("F");

    let mut user = ExternalSystemUser {
        id: None,
        system: ExternalSystem::ORCID,
        name,
        external_id,
        bespoke_data: j,
    };
    let _user_id = user
        .add_to_database(state.clone())
        .await
        .map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;
    println!("G");

    let cookie = user.set_cookie(state).await
        .map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Set cookie
    let mut headers = HeaderMap::new();
    let val = cookie.parse().map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;
    headers.insert(SET_COOKIE, val);
    println!("H");

    Ok((headers, Redirect::to("/")))
}


pub async fn run_server(state: Arc<AppState>) -> Result<(), RingError> {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/redirect_to/orcid", get(redirect_to_orcid))
        .route("/redirect/orcid", get(redirect_orcid))
        .route("/auth/info", get(auth_info))
        .route("/user/entities", get(user_entities))
        .route("/user/logout", get(user_logout))
        .route("/entities/:ids", get(entities))
        .nest_service("/", ServeDir::new("html"))
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        ;

    let port: u16 = state.port;
    let ip = [0, 0, 0, 0];

    let addr = SocketAddr::from((ip, port));
    tracing::info!("listening on http://{}", addr);
    if let Err(e) = Server::bind(&addr).serve(app.into_make_service()).await {
        return Err(RingError::String(format!("Server fail: {e}")));
    }
        

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), RingError> {
    // let cli = Cli::parse();
    let state = Arc::new(AppState::from_config_file("config.json").expect("app creation failed"));
    run_server(state).await?;
    Ok(())
}
