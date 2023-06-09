use std::sync::Arc;
use async_session::{Session, SessionStore};
use axum::TypedHeader;
use mysql_async::{params, prelude::*};
use serde_json::{Value, json};
use serde::{Serialize,Deserialize};
use crate::error::RingError;
use crate::app_state::AppState;
use crate::entity::EntityGroup;

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
        let sql = r#"INSERT INTO `user` (`system`,`external_id`,`name`,`bespoke_data`) VALUES (:system,:external_id,:name,:bespoke_data) ON DUPLICATE KEY UPDATE `bespoke_data`=:bespoke_data"# ;
        let mut conn = state.db_conn().await?;
        conn.exec_drop(sql, params!{system,external_id,name,bespoke_data}).await?;
        self.id = conn.last_insert_id();
        self.id.ok_or_else(||format!("User {system}:{external_id} was not added to database").into())
    }

    pub async fn set_cookie(&self, state: Arc<AppState>) -> Result<String,RingError> {
        // Create a new session filled with user data
        let mut session = Session::new();
        session.insert("user", &self)?;

        // Store session and get corresponding cookie
        let cookie = state.store.store_session(session)
            .await
            .map_err(|e|e.to_string())?
            .ok_or_else(||format!("Session store error"))?;

        // Build the cookie
        let cookie = format!("{}={}; SameSite=Lax; Path=/", COOKIE_NAME, cookie);
        Ok(cookie)
    }

    pub async fn from_cookies(app: &Arc<AppState>, cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<Self> {
        let cookie = cookies.to_owned()?.get(COOKIE_NAME)?.to_string();
        let session = app.store.load_session(cookie).await.ok()??;
        let j = json!(session).get("data").cloned()?.get("user")?.to_owned();
        let user: Value = serde_json::from_str(j.as_str()?).ok()?;
        let s = serde_json::to_string(&user).ok()?;
        let ret: Self = serde_json::from_str(&s).ok()?;
        Some(ret)
    }

    pub async fn get_entities_with_access(&self, app: &Arc<AppState>) -> Result<EntityGroup,RingError> {
        let sql = r#"SELECT `entity_id`,`right` FROM `access` WHERE user_id=:user_id"# ;
        let user_id = self.id;

        let res = app.db_conn().await?
            .exec_iter(sql,params! {user_id}).await?
            .map_and_drop(|row|  mysql_async::from_row::<(usize,String)>(row) ).await?;
        let mut entity_ids: Vec<usize> = res
            .iter()
            .map(|(id,_right)|*id)
            .collect();
        entity_ids.sort();
        entity_ids.dedup();

        let mut entities = app.load_entities(&entity_ids).await?;
        res
            .iter()
            .for_each(|(id,right)|{
                if let Some(entity) = entities.get_mut(*id) {
                    entity.rights.push(right.to_owned())
                }
            });
        app.annotate_entities(&mut entities).await?;
        Ok(entities)
    }

}