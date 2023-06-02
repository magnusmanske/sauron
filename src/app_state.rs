use std::{env, fs::File, time::Duration};
use mysql_async::{Conn, PoolOpts, PoolConstraints, OptsBuilder, Opts};
use serde_json::Value;
use crate::error::RingError;
use crate::database_session_store::DatabaseSessionStore;

#[derive(Clone, Debug)]
pub struct AppState {
    pub config: Value,
    pub port: u16,
    pub server: String,
    pub store: DatabaseSessionStore,
    pub db_pool: mysql_async::Pool,
}

impl AppState {
    pub fn from_config_file(filename: &str) -> Result<Self,RingError> {
        let mut path = env::current_dir().expect("Can't get CWD");
        path.push(filename);
        let file = File::open(&path)?;
        let config: Value = serde_json::from_reader(file)?;
        Ok(Self::from_config(config))
    }

    /// Creatre an AppState object from a config JSON object
    pub fn from_config(config: Value) -> Self {
        let db_pool = Self::create_pool(&config["database"]);
        Self {
            port: config["port"].as_u64().expect("Port number in config file missing or not an integer") as u16,
            server: config["server"].as_str().expect("server URL not in config").to_string(),
            db_pool: db_pool.clone(),
            store: DatabaseSessionStore{pool: Some(db_pool.clone())},
            config: config,
        }
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
}
