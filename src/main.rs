use std::{net::SocketAddr, sync::Arc, collections::HashMap, path::PathBuf};
use async_session::SessionStore;
use axum_server::tls_rustls::RustlsConfig;
use serde_json::{Value, json};
use axum::{
    routing::get,
    Router,
    http::StatusCode,
    extract::{State,Query, Path}, response::{Redirect, IntoResponse}, TypedHeader, Json,
};
use http::{header::{ACCEPT, CONTENT_TYPE, SET_COOKIE}, HeaderMap};
use tower_http::{services::ServeDir, trace::TraceLayer, compression::CompressionLayer};
use crate::error::RingError;
use crate::app_state::AppState;
use crate::external_system::*;

pub mod error;
pub mod db_tables;
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
    let entities = user.get_entities_with_access(&state);
    let j = json!({"status":"OK","entities":entities.as_sorted_vec()});
    (StatusCode::OK, Json(j))
}

async fn entities(State(state): State<Arc<AppState>>, Path(entity_ids): Path<String>, _cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let entity_ids: Vec<usize> = entity_ids
        .split(',')
        .filter_map(|e|e.parse::<usize>().ok())
        .collect();
    let mut entities = state.load_entities(&entity_ids);
    state.annotate_entities(&mut entities);
    let j = json!({"status":"OK","entities":entities.as_sorted_vec()});
    (StatusCode::OK, Json(j))
}


async fn redirect_orcid(State(state): State<Arc<AppState>>, 
    Query(params): Query<HashMap<String, String>>, 
    _cookies: Option<TypedHeader<headers::Cookie>>,
) -> impl IntoResponse {
    let code = match params.get("code") {
        Some(code) => code,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    let redirect_url = format!("{}/redirect/orcid",state.get_redirect_server());
    let client_id = state.config["systems"]["orcid"]["client_id"].as_str().expect("ORCID client_id");
    let client_secret = state.config["systems"]["orcid"]["client_secret"].as_str().expect("ORCID client_secret");
    let body = format!("client_id={client_id}&client_secret={client_secret}&grant_type=authorization_code&code={code}&redirect_uri={redirect_url}");

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

    let name = j["name"].as_str()
        .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let external_id = j["orcid"].as_str()
        .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();

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

    let cookie = user.set_cookie(state).await
        .map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Set cookie
    let mut headers = HeaderMap::new();
    let val = cookie.parse().map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;
    headers.insert(SET_COOKIE, val);

    Ok((headers, Redirect::to("/")))
}


pub async fn run_server(state: Arc<AppState>) -> Result<(), RingError> {
    tracing_subscriber::fmt::init();

    let cert_dir = "."; // env!("CARGO_MANIFEST_DIR")
    let cert_path = match state.config["ssl"]["cert"].as_str() {
        Some(s) => PathBuf::from(s),
        None => PathBuf::from(cert_dir).join("self_signed_certs").join("cert.pem")
    };
    let key_path = match state.config["ssl"]["key"].as_str() {
        Some(s) => PathBuf::from(s),
        None => PathBuf::from(cert_dir).join("self_signed_certs").join("key.pem")
    };
    let config = RustlsConfig::from_pem_file(cert_path,key_path).await.unwrap();


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

    let addr = SocketAddr::from(([0, 0, 0, 0], state.port_https));
    tracing::info!("listening on {}", addr);
    axum_server::bind_rustls(addr, config).serve(app.into_make_service()).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), RingError> {
    let state = Arc::new(AppState::from_config_file("config.json").await.expect("app creation failed"));
    run_server(state).await?;
    Ok(())
}
