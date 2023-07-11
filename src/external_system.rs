use std::sync::Arc;
use async_session::{Session, SessionStore};
use axum::TypedHeader;
use serde_json::{Value, json};
use serde::{Serialize,Deserialize};
use crate::error::RingError;
use crate::app_state::AppState;
use crate::entity::EntityGroup;

pub static COOKIE_NAME: &str = "SESSION";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExternalSystem {
    ORCID,
    GOOGLE,
    Unknown,
}

impl ExternalSystem {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ORCID => "orcid",
            Self::GOOGLE => "google",
            Self::Unknown => "",
        }
    }

    pub fn as_url(&self, external_id: &str) -> String {
        match self {
            Self::ORCID => format!("https://orcid.org/{external_id}"),
            Self::GOOGLE => String::new(),
            Self::Unknown => String::new(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "orcid" => Self::ORCID,
            "google" => Self::GOOGLE,
            _ => Self::Unknown,
        }
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalSystemUser {
    pub id: Option<u64>,
    pub system: ExternalSystem,
    pub name: String,
    pub email: String,
    pub external_id: String,
    pub bespoke_data: Value,
}

impl ExternalSystemUser {
    pub async fn add_to_database(&mut self, state: Arc<AppState>) -> Result<u64,RingError> {
        let system = self.system.as_str();
        let external_id = &self.external_id;
        let name = &self.name;
        let email = &self.email;
        let bespoke_data = self.bespoke_data.to_string();
        self.id = state.dal.write().await.add_user(system,external_id,name,email,&bespoke_data).await?;
        self.id.ok_or_else(||format!("User {system}:{external_id} was not added to database").into())
    }

    pub fn strip_private_data(&mut self) {
        self.bespoke_data = Value::Null;
        self.email = String::new();
    }

    pub fn from_row(row: &mysql_async::Row) -> Self {
        let system: String = row.get(1).unwrap();
        let json: String = row.get(5).unwrap();
        Self {
            id: row.get(0),
            system: ExternalSystem::from_str(&system),
            name: row.get(2).unwrap(),
            external_id: row.get(3).unwrap(),
            email: row.get(4).unwrap(),
            bespoke_data: serde_json::from_str(&json).unwrap_or(Value::Null),
        }
    }

    pub fn external_url(&self) -> String {
        self.system.as_url(&self.external_id)
    }

    pub async fn set_cookie(&self, state: Arc<AppState>) -> Result<String,RingError> {
        // Create a new session filled with user data
        let mut session = Session::new();
        session.insert("user", &self)?;

        // Store session and get corresponding cookie
        let cookie = state.dal.read().await.session_store.store_session(session)
            .await
            .map_err(|e|e.to_string())?
            .ok_or_else(||format!("Session store error"))?;

        // Build the cookie
        let cookie = format!("{}={}; SameSite=Lax; Path=/", COOKIE_NAME, cookie);
        Ok(cookie)
    }

    pub async fn from_cookies(app: &Arc<AppState>, cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<Self> {
        let cookie = cookies.to_owned()?.get(COOKIE_NAME)?.to_string();
        let session = app.dal.read().await.session_store.load_session(cookie).await.ok()??;
        let j = json!(session).get("data").cloned()?.get("user")?.to_owned();
        let user: Value = serde_json::from_str(j.as_str()?).ok()?;
        let s = serde_json::to_string(&user).ok()?;
        let ret: Self = serde_json::from_str(&s).ok()?;
        Some(ret)
    }

    pub async fn get_entities_with_access(&self, app: &Arc<AppState>) -> Result<EntityGroup,RingError> {
        match self.id {
            Some(user_id) => app.dal.read().await.get_entities_with_user_access(user_id as usize,None).await,
            None => Ok(EntityGroup::empty()),
        }
        
    }

}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalAccessRequest {
    pub id: usize,
    pub user_id: usize,
    pub entity_id: usize,
    pub note: String,
}

impl ExternalAccessRequest {
    pub fn from_row(row: &mysql_async::Row) -> Self {
        Self {
            id: row.get(0).unwrap(),
            user_id: row.get(1).unwrap(),
            entity_id: row.get(2).unwrap(),
            note: row.get(3).unwrap(),
        }
    }
}