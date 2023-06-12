use std::collections::HashMap;
use std::{env, fs::File, time::Duration};
use mysql_async::prelude::*;
use mysql_async::{Conn, PoolOpts, PoolConstraints, OptsBuilder, Opts};
use serde_json::Value;
use crate::db_tables::{DbTableAccess, DbTableConnection, DbTableEntity};
use crate::error::RingError;
use crate::database_session_store::DatabaseSessionStore;
use crate::entity::{Entity, EntityGroup};


// ************************************************************************************************

#[derive(Clone, Debug)]
pub struct AppState {
    pub config: Value,
    pub port_http: u16,
    pub port_https: u16,
    pub server: String,
    pub store: DatabaseSessionStore,
    pub db_pool: mysql_async::Pool,
    pub db_access: HashMap<usize,DbTableAccess>,
    pub db_connection: HashMap<usize,DbTableConnection>,
    pub db_entity: HashMap<usize,DbTableEntity>,
}

impl AppState {
    pub async fn from_config_file(filename: &str) -> Result<Self,RingError> {
        let mut path = env::current_dir().expect("Can't get CWD");
        path.push(filename);
        let file = File::open(&path)?;
        let config: Value = serde_json::from_reader(file)?;
        let ret = Self::from_config(config).await?;
        Ok(ret)
    }

    /// Creatre an AppState object from a config JSON object
    pub async fn from_config(config: Value) -> Result<Self,RingError> {
        let db_pool = Self::create_pool(&config["database"]);
        let mut ret = Self {
            port_http: config["port_http"].as_u64().expect("Port number in config file missing or not an integer") as u16,
            port_https: config["port_https"].as_u64().expect("Port number in config file missing or not an integer") as u16,
            server: config["server"].as_str().expect("server URL not in config").to_string(),
            db_pool: db_pool.clone(),
            store: DatabaseSessionStore::new_with_pool(&db_pool).await?,
            config: config,
            db_access: HashMap::new(),
            db_connection: HashMap::new(),
            db_entity: HashMap::new(),
        };
        ret.init_from_db().await?;
        Ok(ret)
    }

    async fn init_from_db(&mut self) -> Result<(),RingError> {
        let mut conn = self.db_conn().await?;
        self.db_access = conn
            .exec_iter("SELECT `id`,`user_id`,`entity_id`,`right` FROM `access`",()).await?
            .map_and_drop(|row| DbTableAccess::from_row(&row) ).await?.into_iter().map(|x|(x.id,x)).collect();
        self.db_connection = conn
            .exec_iter("SELECT `id`,`parent_id`,`child_id` FROM `connection`",()).await?
            .map_and_drop(|row| DbTableConnection::from_row(&row) ).await?.into_iter().map(|x|(x.id,x)).collect();
        self.db_entity = conn
            .exec_iter("SELECT `id`,`name` FROM `entity`",()).await?
            .map_and_drop(|row| DbTableEntity::from_row(&row) ).await?.into_iter().map(|x|(x.id,x)).collect();
        Ok(())
    }

    /// Helper function to create a DB pool from a JSON config object
    fn create_pool(config: &Value) -> mysql_async::Pool {
        let min_connections = config["min_connections"].as_u64().expect("No min_connections value") as usize;
        let max_connections = config["max_connections"].as_u64().expect("No max_connections value") as usize;
        let keep_sec = config["keep_sec"].as_u64().expect("No keep_sec value");
        let url = config["url"].as_str().expect("No url value");
        let pool_opts = PoolOpts::default()
            .with_constraints(PoolConstraints::new(min_connections, max_connections).expect("pool_opts error"))
            .with_inactive_connection_ttl(Duration::from_secs(keep_sec));
        let wd_url = url;
        let wd_opts = Opts::from_url(wd_url).expect(format!("Can not build options from db_wd URL {}",wd_url).as_str());
        mysql_async::Pool::new(OptsBuilder::from_opts(wd_opts).pool_opts(pool_opts.clone()))
    }

    /// Returns a connection to the GULP tool database
    pub async fn db_conn(&self) -> Result<Conn, mysql_async::Error> {
        self.db_pool.get_conn().await
    }

    pub fn load_entities(&self, entity_ids: &[usize]) -> EntityGroup {
        let v: Vec<Entity> = entity_ids
            .iter()
            .filter_map(|id|self.db_entity.get(id))
            .map(|e|Entity::from_table_entity(e))
            .collect();
        EntityGroup::from_vec(v)
    }

    /// Returns Vec<(parent,child)>
    fn load_entity_children(&self, entity_ids: &[usize]) -> Vec<(usize,usize)> {
        self.db_connection
            .iter()
            .filter(|(_id,c)|entity_ids.contains(&c.parent_id))
            .map(|(_id,c)|(c.parent_id,c.child_id))
            .collect()
    }

    /// Returns Vec<(parent,child)>
    fn load_entity_parents(&self, entity_ids: &[usize]) -> Vec<(usize,usize)> {
        self.db_connection
            .iter()
            .filter(|(_id,c)|entity_ids.contains(&c.child_id))
            .map(|(_id,c)|(c.parent_id,c.child_id))
            .collect()
    }

    pub fn annotate_entities(&self, entities: &mut EntityGroup) {
        self.load_entity_children(&entities.keys())
            .iter()
            .for_each(|(parent,child)|{
                if let Some(entity) = entities.get_mut(*parent) {
                    entity.child_ids.push(*child)
                }
            });
        self.load_entity_parents(&entities.keys())
            .iter()
            .for_each(|(parent,child)|{
                if let Some(entity) = entities.get_mut(*child) {
                    entity.parent_ids.push(*parent)
                }
            });
    }

    pub fn get_redirect_server(&self) -> String {
        match self.port_https {
            443 => format!("https://{}",self.server),
            port => format!("https://{}:{}",self.server,port),
        }
    }
}
