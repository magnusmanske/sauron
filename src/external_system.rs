use std::sync::Arc;
use async_session::{Session, SessionStore};
use mysql_async::{params, prelude::*};
use serde_json::Value;
use serde::{Serialize,Deserialize};
use crate::error::RingError;
use crate::app_state::AppState;

pub static COOKIE_NAME: &str = "SESSION";

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

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "orcid" => Some(Self::ORCID),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalSystemUser {
    pub id: Option<u64>,
    pub system: ExternalSystem,
    pub name: String,
    pub external_id: String,
    pub bespoke_data: Value,
}

impl ExternalSystemUser {
    pub async fn add_to_database(&mut self, state: Arc<AppState>) -> Result<u64,RingError> {
        let system = self.system.as_str();
        let external_id = &self.external_id;
        let name = &self.name;
        let bespoke_data = &self.bespoke_data;
        let sql = r#"REPLACE INTO `user` (`system`,`external_id`,`name`,`bespoke_data`) VALUES (:system,:external_id,:name,:bespoke_data)"# ;
        let mut conn = state.db_conn().await?;
        conn.exec_drop(sql, params!{system,external_id,name,bespoke_data}).await?;
        self.id = conn.last_insert_id();
        self.id.ok_or_else(||format!("User {system}:{external_id} was not added to database").into())
    }

    pub async fn set_cookie(&self, state: Arc<AppState>) -> Result<String,RingError> {
        // Create a new session filled with user data
        let mut session = Session::new();
        session.insert("user", &self).unwrap();

        // Store session and get corresponding cookie
        let cookie = state.store.store_session(session).await.unwrap().unwrap();

        // Build the cookie
        let cookie = format!("{}={}; SameSite=Lax; Path=/", COOKIE_NAME, cookie);
        Ok(cookie)
    }
}