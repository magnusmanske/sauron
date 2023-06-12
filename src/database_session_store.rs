use std::{collections::HashMap, sync::Arc};

use mysql_async::{prelude::*, Conn};
use async_session::{Session, SessionStore};
use async_trait::async_trait;
use serde_json::json;
use tokio::sync::Mutex;

use crate::{db_tables::DbTableSession, error::RingError};

#[derive(Debug, Clone)]
pub struct DatabaseSessionStore {
    pub pool: mysql_async::Pool,
    cache: Arc<Mutex<HashMap<String,DbTableSession>>>,
}

#[async_trait]
impl SessionStore for DatabaseSessionStore {
    async fn load_session(&self, cookie_value: String) -> async_session::Result<Option<Session>> {
        let id_string = Session::id_from_cookie_value(&cookie_value)?;
        let ret = match self.cache.lock().await.get(&id_string) {
            Some(s) => serde_json::from_str(&s.json)?,
            None => None
        };
        Ok(ret)
    }

    async fn store_session(&self, session: Session) -> async_session::Result<Option<String>> {
        let id_string = session.id().to_string();
        let json = json!(session).to_string();
        let sql = "INSERT INTO `session` (`id_string`,`json`) VALUES (:id_string,:json) ON DUPLICATE KEY UPDATE `json`=:json" ;
        let mut conn = self.db_conn().await;
        conn.exec_drop(sql, params!{id_string,json}).await?;
        match conn.last_insert_id() {
            Some(id) => {
                let id_string = session.id().to_string();
                let json = json!(session).to_string();
                let mut cache = self.cache.lock().await;
                cache.remove(&id_string);
                let s = DbTableSession {id: id as usize,id_string,json};
                cache.insert(s.id_string.to_owned(), s);
            }
            None => {}
        }
        session.reset_data_changed();
        Ok(session.into_cookie_value())
    }

    async fn destroy_session(&self, session: Session) -> async_session::Result {
        let id_string = session.id().to_string();
        let sql = "DELETE FROM `session` WHERE `id_string`=:id_string" ;
        self.db_conn().await.exec_drop(sql, params!{id_string}).await?;
        self.cache.lock().await.remove(session.id());
        Ok(())
    }

    async fn clear_store(&self) -> async_session::Result {
        let id = 0; // Dummy
        let sql = "DELETE FROM `session` WHERE id>:id" ;
        self.db_conn().await.exec_drop(sql, params!{id}).await?;
        self.cache.lock().await.clear();
        Ok(())
    }
}

impl DatabaseSessionStore {
    /// Create a new instance of DatabaseSessionStore
    pub async fn new_with_pool(pool: &mysql_async::Pool) -> Result<Self,RingError> {
        let ret = Self {
            pool: pool.clone(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        };
        *ret.cache.lock().await = ret.db_conn().await
            .exec_iter("SELECT `id`,`id_string`,`json` FROM `session`",()).await?
            .map_and_drop(|row| DbTableSession::from_row(&row) ).await?
            .into_iter().map(|s|(s.id_string.to_owned(),s)).collect();
        Ok(ret)
    }

    async fn db_conn(&self) -> Conn {
        self.pool.get_conn().await.unwrap()
    }
}
