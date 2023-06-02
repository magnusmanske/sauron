use std::{net::SocketAddr, sync::Arc, collections::HashMap};
use serde_json::{Value, json};
use axum::{
    routing::get,
    Router,
    http::StatusCode,
    Server,
    extract::{State,Query}, response::{Redirect, IntoResponse, Response}, TypedHeader, Json,
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


async fn auth_info(State(state): State<Arc<AppState>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user = ExternalSystemUser::from_cookies(&state, &cookies).await;
    let j = json!({"status":"OK","user":user});
    (StatusCode::OK, Json(j)).into_response()
}


async fn redirect_orcid(State(state): State<Arc<AppState>>, 
    Query(params): Query<HashMap<String, String>>, 
    _cookies: Option<TypedHeader<headers::Cookie>>,
) -> impl IntoResponse {
    let code = match params.get("code") {
        Some(code) => code,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    let server = &state.server;
    let port = state.config["port"].as_u64().expect("port");
    let redirect_uri = format!("https://{server}:{port}/redirect/orcid");
    let client_id = state.config["systems"]["orcid"]["client_id"].as_str().expect("ORCID client_id");
    let client_secret = state.config["systems"]["orcid"]["client_secret"].as_str().expect("ORCID client_secret");
    let body = format!("client_id={client_id}&client_secret={client_secret}&grant_type=authorization_code&code={code}&redirect_uri={redirect_uri}");

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

    let mut user = ExternalSystemUser {
        id: None,
        system: ExternalSystem::ORCID,
        name: j["name"].as_str().unwrap().to_string(),
        external_id: j["orcid"].as_str().unwrap().to_string(),
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
    headers.insert(SET_COOKIE, cookie.parse().unwrap());

    Ok((headers, Redirect::to("/")))
}


pub async fn run_server(state: Arc<AppState>) -> Result<(), RingError> {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/redirect/orcid", get(redirect_orcid))
        .route("/auth/info", get(auth_info))
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
