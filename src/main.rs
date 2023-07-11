use std::{net::SocketAddr, sync::Arc, collections::HashMap, path::PathBuf, env};
use async_session::SessionStore;
use axum_server::tls_rustls::RustlsConfig;
use entity::{Entity, EntityGroup};
use serde_json::{Value, json};
use google_oauth::AsyncClient;
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
pub mod database_abstraction_layer;
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

async fn redirect_to_google(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let redirect_url = format!("{}/redirect/google",state.get_redirect_server());
    let client_id = match state.config["systems"]["google"]["client_id"].as_str() {
        Some(id) => id,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let scope = "https://www.googleapis.com/auth/userinfo.profile openid email";
    let url = format!("https://accounts.google.com/o/oauth2/v2/auth?client_id={client_id}&response_type=code&scope={scope}&redirect_uri={redirect_url}");
    Ok(Redirect::to(&url))
}


async fn auth_info(State(state): State<Arc<AppState>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let user = ExternalSystemUser::from_cookies(&state, &cookies).await;
    let j = json!({"status":"OK","user":user});
    (StatusCode::OK, Json(j))
}

async fn user_logout(State(state): State<Arc<AppState>>, cookies: TypedHeader<headers::Cookie>,) -> impl IntoResponse {
    let cookie = cookies.get(COOKIE_NAME).unwrap().to_string();
    if let Some(session) = state.dal.read().await.session_store.load_session(cookie).await.unwrap() {
        state.dal.read().await.session_store.destroy_session(session).await.unwrap();
    };
    Redirect::to("/")
}

async fn parents_children_entities(state: Arc<AppState>, entities: &Vec<Entity>) -> Result<(EntityGroup,EntityGroup),RingError> {
    let parents: Vec<usize> = entities.iter().map(|e|e.parent_ids.to_owned()).flatten().collect();
    let mut parents = state.dal.read().await.load_entities(&parents).await?;
    state.dal.read().await.annotate_entities(&mut parents).await?;

    let children: Vec<usize> = entities.iter().map(|e|e.child_ids.to_owned()).flatten().collect();
    let mut children = state.dal.read().await.load_entities(&children).await?;
    state.dal.read().await.annotate_entities(&mut children).await?;

    Ok((parents,children))
}

async fn user_entities(State(state): State<Arc<AppState>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let user = match ExternalSystemUser::from_cookies(&state, &cookies).await {
        Some(user) => user,
        None => return (StatusCode::OK, Json(json!({"status":"not_logged_in"}))),
    };
    let entities = match user.get_entities_with_access(&state).await {
        Ok(x) => x.as_sorted_vec(),
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    let (parents,children) = match parents_children_entities(state,&entities).await {
        Ok(x) => x,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    let j = json!({
        "status":"OK",
        "entities":entities,
        "parents": parents.as_sorted_vec(),
        "children": children.as_sorted_vec()
    });
    (StatusCode::OK, Json(j))
}

async fn search_user(State(state): State<Arc<AppState>>, Path(query): Path<String>,) -> impl IntoResponse {
    let user_ids = match state.dal.read().await.search_user_name(&query).await {
        Ok(ids) => ids,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    let mut users = HashMap::new();
    for user_id in user_ids {
        let mut user = match state.dal.read().await.get_user(user_id).await {
            Ok(user) => user,
            Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
        };
        user.strip_private_data();
        users.insert(user_id,user);
    }
    let j = json!({
        "status":"OK",
        "results":users,
    });
    (StatusCode::OK, Json(j))
}

async fn search_access(State(state): State<Arc<AppState>>, Path(query): Path<String>,) -> impl IntoResponse {
    let rights = match state.dal.read().await.search_access_rights(&query).await {
        Ok(ids) => ids,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    let j = json!({
        "status":"OK",
        "results":rights,
    });
    (StatusCode::OK, Json(j))
}

// async fn search_entity(State(_state): State<Arc<AppState>>, Path(_query): Path<String>,) -> impl IntoResponse {
//     todo!()
// }

async fn user_info(State(state): State<Arc<AppState>>, Path(user_id): Path<usize>,) -> impl IntoResponse {
    let mut user = match state.dal.read().await.get_user(user_id).await {
        Ok(user) => user,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    user.strip_private_data(); // Prevent private data from leaking
    let mut user_j = json!(user);
    user_j["external_url"] = json!(user.external_url());
    let j = json!({
        "status":"OK",
        "user":user_j,
    });
    (StatusCode::OK, Json(j))
}
async fn get_rights_entities(State(state): State<Arc<AppState>>, Path(entity_ids): Path<String>,) -> impl IntoResponse {
    let entity_ids: Vec<usize> = entity_ids
        .split(',')
        .filter_map(|e|e.parse::<usize>().ok())
        .collect();

    let mut rights = HashMap::new(); // id => Vec(user_id,right)
    for entity_id in &entity_ids {
        let r = match state.dal.read().await.get_all_rights_for_entity(*entity_id).await {
            Ok(r) => r,
            Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
        };
        rights.insert(entity_id,r);
    }

    let mut access_requests = vec![];
    for entity_id in &entity_ids {
        let mut access_requests_tmp = match state.dal.read().await.get_access_requests(*entity_id).await {
            Ok(data) => data,
            Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
        };
        access_requests.append(&mut access_requests_tmp);
    }

    let mut user_ids: Vec<usize> = rights.iter()
        .map(|(_entity_id,user_right)|user_right)
        .flatten()
        .map(|(user_id,_right)|*user_id)
        .collect();
    user_ids.append(&mut access_requests.iter().map(|ar|ar.user_id).collect());
    user_ids.sort();
    user_ids.dedup();
    let mut users = HashMap::new();
    for user_id in user_ids {
        let mut user = match state.dal.read().await.get_user(user_id).await {
            Ok(user) => user,
            Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
        };
        user.strip_private_data(); // Prevent private data from leaking
        let mut user_j = json!(user);
        user_j["external_url"] = json!(user.external_url());
        users.insert(user_id,user_j);
    }

    let j = json!({
        "status":"OK",
        "rights":rights,
        "users":users,
        "access_requests":access_requests,
    });
    (StatusCode::OK, Json(j))
}

fn parse_rights_string(rights: &str) -> Vec<String> {
    rights.split(",")
        .map(|s|s.to_lowercase().trim().to_string())
        .filter(|s|!s.is_empty())
        .collect()
}

async fn get_current_user_id(state: &Arc<AppState>, cookies: &Option<TypedHeader<headers::Cookie>>) -> Result<usize,RingError> {
    let current_user = ExternalSystemUser::from_cookies(&state, &cookies).await.ok_or_else(||RingError::String("not logged in".into()))?;
    let current_user_id = current_user.id.ok_or_else(||RingError::String("logged in but no user ID".into()))? as usize;
    Ok(current_user_id)
}

async fn user_rights_prep(state: &Arc<AppState>, entity_ids: String, cookies: &Option<TypedHeader<headers::Cookie>>) -> Result<Vec<usize>,RingError> {
    let current_user_id = get_current_user_id(state,cookies).await?;

    // Parse entity IDs from String, and check that the logged-in user has admin rights on all of them
    let entity_ids: Vec<usize> = entity_ids
        .split(',')
        .filter_map(|e|e.parse::<usize>().ok())
        .collect();
    let allowed_entities = state.dal.read().await.get_all_user_rights_for_entities(current_user_id,Some("admin".into())).await?;
    if entity_ids.iter().any(|entity_id|!allowed_entities.has(*entity_id)) {
        return Err(RingError::String("You do not have admin rights to all these entities".into()));
    }
    Ok(entity_ids)
}

async fn add_entity_child(State(state): State<Arc<AppState>>, Path((entity_id,name,ext_id)): Path<(usize,String,String)>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let current_user_id = match get_current_user_id(&state,&cookies).await {
        Ok(id) => id,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    };
    let allowed_entities = match state.dal.read().await.get_all_user_rights_for_entities(current_user_id,Some("admin".into())).await {
        Ok(entity_ids) => entity_ids,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    };
    if !allowed_entities.has(entity_id) {
        return (StatusCode::OK, Json(json!({"status":"You do not have admin rights to create a child entity here"})))
    }
    let child_id = match state.dal.write().await.create_child_entity(entity_id,&name,&ext_id).await {
        Ok(id) => id,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    };
    let j = json!({"status":"OK","child_id":child_id});
    (StatusCode::OK, Json(j))
}

async fn set_user_rights(State(state): State<Arc<AppState>>, Path((entity_ids,user_id,rights)): Path<(String,usize,String)>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let rights = parse_rights_string(&rights);
    let entity_ids = match user_rights_prep(&state,entity_ids,&cookies).await {
        Ok(ids) => ids,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    };
    if let Err(e) = state.dal.write().await.set_access_rights(user_id,entity_ids,rights).await {
        return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    }
    let j = json!({"status":"OK"});
    (StatusCode::OK, Json(j))
}

async fn add_user_rights(State(state): State<Arc<AppState>>, Path((entity_ids,user_id,rights)): Path<(String,usize,String)>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let rights = parse_rights_string(&rights);
    let entity_ids = match user_rights_prep(&state,entity_ids,&cookies).await {
        Ok(ids) => ids,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    };
    if let Err(e) = state.dal.write().await.add_access_rights(user_id,entity_ids,rights).await {
        return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    }
    let j = json!({"status":"OK"});
    (StatusCode::OK, Json(j))
}

async fn remove_user_rights(State(state): State<Arc<AppState>>, Path((entity_ids,user_id,rights)): Path<(String,usize,String)>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let rights = parse_rights_string(&rights);
    let entity_ids = match user_rights_prep(&state,entity_ids,&cookies).await {
        Ok(ids) => ids,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    };
    let j = json!({"status":"OK","rights":rights,"entities":entity_ids,"user":user_id});
    if let Err(e) = state.dal.write().await.remove_access_rights(user_id,entity_ids,rights).await {
        return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    }
    (StatusCode::OK, Json(j))
}

async fn request_access_rights(State(state): State<Arc<AppState>>, Path((entity_ids,note)): Path<(String,String)>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let current_user_id = match get_current_user_id(&state,&cookies).await {
        Ok(id) => id,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    };
    let entity_ids: Vec<usize> = entity_ids
        .split(',')
        .filter_map(|e|e.parse::<usize>().ok())
        .collect();
    if let Err(e) = state.dal.write().await.request_access_rights(current_user_id,entity_ids,&note).await {
        return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    }
    let j = json!({"status":"OK"});
    (StatusCode::OK, Json(j))
}

async fn user_entity_rights(State(state): State<Arc<AppState>>, Path(entity_ids): Path<String>, cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let user = match ExternalSystemUser::from_cookies(&state, &cookies).await {
        Some(user) => user,
        None => return (StatusCode::OK, Json(json!({"status":"not_logged_in"}))),
    };
    let user_id = match user.id {
        Some(id) => id as usize,
        None => return (StatusCode::OK, Json(json!({"status":"logged in but no user ID"}))),
    };

    let allowed_entities = match state.dal.read().await.get_all_user_rights_for_entities(user_id,None).await {
        Ok(x) => x,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };

    let entities: Vec<&Entity> = entity_ids
        .split(',')
        .filter_map(|e|e.parse::<usize>().ok())
        .filter_map(|entity_id| allowed_entities.get(entity_id))
        .collect();

    let j = json!({
        "status":"OK",
        "entities":entities,
    });
    (StatusCode::OK, Json(j))
}


async fn entities(State(state): State<Arc<AppState>>, Path(entity_ids): Path<String>, _cookies: Option<TypedHeader<headers::Cookie>>,) -> impl IntoResponse {
    let entity_ids: Vec<usize> = entity_ids
        .split(',')
        .filter_map(|e|e.parse::<usize>().ok())
        .collect();
    let mut entities = match state.dal.read().await.load_entities(&entity_ids).await {
        Ok(x) => x,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    if let Err(e) = state.dal.read().await.annotate_entities(&mut entities).await {
        return (StatusCode::OK, Json(json!({"status":e.to_string()})))
    }
    let entities = entities.as_sorted_vec();
    let (parents,children) = match parents_children_entities(state,&entities).await {
        Ok(x) => x,
        Err(e) => return (StatusCode::OK, Json(json!({"status":e.to_string()}))),
    };
    let j = json!({
        "status":"OK",
        "entities":entities,
        "parents": parents.as_sorted_vec(),
        "children": children.as_sorted_vec()
    });
    (StatusCode::OK, Json(j))
}

async fn redirect_google(State(state): State<Arc<AppState>>, 
    Query(params): Query<HashMap<String, String>>, 
    _cookies: Option<TypedHeader<headers::Cookie>>,
) -> impl IntoResponse {
    let code = match params.get("code") {
        Some(code) => code,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    let redirect_url = format!("{}/redirect/google",state.get_redirect_server());
    let client_id = state.config["systems"]["google"]["client_id"].as_str().expect("Google client_id");
    let client_secret = state.config["systems"]["google"]["client_secret"].as_str().expect("Google client_secret");
    let body = format!("client_id={client_id}&client_secret={client_secret}&grant_type=authorization_code&code={code}&redirect_uri={redirect_url}");

    let j = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?
        .json::<Value>().await
        .map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;

    // let _access_token = j["access_token"].as_str()
    //     .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
    //     .to_string();
    let id_token = j["id_token"].as_str()
        .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();

    let client = AsyncClient::new(client_id);
    let data = match client.validate_id_token(id_token).await {
        Ok(data) => data,
        Err(_e) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let j = json!(data);

    let name = j["name"].as_str()
        .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let external_id = j["sub"].as_str()
        .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let email = j["email"].as_str()
        .ok_or_else(|| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();

    let mut user = ExternalSystemUser {
        id: None,
        system: ExternalSystem::GOOGLE,
        name,
        external_id,
        email,
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
        email: String::new(),
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
        .route("/redirect_to/google", get(redirect_to_google))
        .route("/redirect/orcid", get(redirect_orcid))
        .route("/redirect/google", get(redirect_google))
        .route("/auth/info", get(auth_info))
        .route("/user/entities", get(user_entities))
        .route("/user/entity_rights/:ids", get(user_entity_rights))
        .route("/rights/set/:entity_ids/:user_id/:rights", get(set_user_rights))
        .route("/rights/add/:entity_ids/:user_id/:rights", get(add_user_rights))
        .route("/rights/remove/:entity_ids/:user_id/:rights", get(remove_user_rights))
        .route("/rights/request/:entity_ids/:note", get(request_access_rights))
        .route("/rights/get/entities/:ids", get(get_rights_entities))
        .route("/user/logout", get(user_logout))
        .route("/user/info/:id", get(user_info))
        .route("/entities/:ids", get(entities))
        .route("/entity/add/child/:entity_id/:name/:ext_id", get(add_entity_child))
        .route("/search/user/:query", get(search_user))
        .route("/search/access/:query", get(search_access))
        // .route("/search/entity/:query", get(search_entity))
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
    let argv = env::args();
    if argv.len()>1 { // For testing
        let x = state.dal.read().await.search_user_name("Magnus").await?;
        println!("{x:#?}");
        return Ok(());
    }

    // Start the server
    run_server(state).await?;
    Ok(())
}
