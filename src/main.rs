use std::{net::SocketAddr, sync::Arc, collections::HashMap};
use serde_json::{json, Value};
use serde::{Serialize,Deserialize};
use axum::{
    routing::{get},//, post},
    Json, 
    Router,
    http::StatusCode,
    Server,
    extract::{State,Query,TypedHeader},//,Multipart,DefaultBodyLimit,Path},
    // response::{IntoResponse, Response},
};
use http::header::{ACCEPT, CONTENT_TYPE};
use tower_http::services::ServeDir;
use crate::error::RingError;

pub mod error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppState {
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExternalSystem {
    ORCID,
}

impl ExternalSystem {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ORCID => "orcid",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalSystemUser {
    pub system: ExternalSystem,
    pub name: String,
    pub external_id: String,
    pub bespoke_data: Value,
}

async fn redirect_orcid(State(_state): State<Arc<AppState>>, 
    Query(params): Query<HashMap<String, String>>, 
    _cookies: Option<TypedHeader<headers::Cookie>>,
) -> Result<Json<Value>, StatusCode> {
    let code = match params.get("code") {
        Some(code) => code,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let my_url = "https://127.0.0.1:8082/redirect/orcid";
    let client_id = "APP-05P9ELF6BR53WKIS";
    let client_secret = "b34ab9fa-d96d-470e-8cb6-983b5ad8c916";

    let body = vec![
        ("client_id",client_id),
        ("client_secret",client_secret),
        ("grant_type","authorization_code"),
        ("code",code.as_str()),
        ("redirect_uri",my_url),
    ]
        .iter()
        .map(|(k,v)|format!("{k}={v}"))
        .collect::<Vec<String>>()
        .join("&");

    let client = reqwest::Client::new();
    let res = client
        .post("https://orcid.org/oauth/token")
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await;
    let response = match res {
        Ok(response) => response,
        Err(_e) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let j = match response.json::<Value>().await {
        Ok(j) => j,
        Err(_e) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let user = ExternalSystemUser {
        system: ExternalSystem::ORCID,
        name: j.get("name").unwrap().to_string(),
        external_id: j.get("orcid").unwrap().to_string(),
        bespoke_data: j,
    };

    let j = json!({"status":"OK","user":user});
    Ok(Json(j))
}


pub async fn run_server(state: Arc<AppState>) -> Result<(), RingError> {
    tracing_subscriber::fmt::init();

    // let cors = CorsLayer::new().allow_origin(Any);

    let app = Router::new()
        .route("/redirect/orcid", get(redirect_orcid))
        .nest_service("/", ServeDir::new("html"))
        .with_state(state.clone())
        // .layer(DefaultBodyLimit::max(1024*1024*MAX_UPLOAD_MB))
        // .layer(TraceLayer::new_for_http())
        // .layer(CompressionLayer::new())
        // .layer(cors)
        ;

    let port: u16 = 8082;
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
    // let app = Arc::new(AppState::from_config_file("config.json").expect("app creation failed"));
    let state = Arc::new(AppState{});
    run_server(state).await?;
    Ok(())
}
