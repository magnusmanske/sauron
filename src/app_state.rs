use std::sync::Arc;
use std::{env, fs::File};
use serde_json::Value;
use tokio::sync::RwLock;
use crate::error::RingError;
use crate::database_abstraction_layer::DatabaseAbstractionLayer;


// ************************************************************************************************

#[derive(Clone, Debug)]
pub struct AppState {
    pub config: Value,
    pub port_http: u16,
    pub port_https: u16,
    pub server: String,
    pub dal: Arc<RwLock<DatabaseAbstractionLayer>>,
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
        let ret = Self {
            port_http: config["port_http"].as_u64().expect("Port number in config file missing or not an integer") as u16,
            port_https: config["port_https"].as_u64().expect("Port number in config file missing or not an integer") as u16,
            server: config["server"].as_str().expect("server URL not in config").to_string(),
            dal: Arc::new(RwLock::new(DatabaseAbstractionLayer::new(&config).await?)),
            config: config,
        };
        Ok(ret)
    }


    pub fn get_redirect_server(&self) -> String {
        match self.port_https {
            443 => format!("https://{}",self.server),
            port => format!("https://{}:{}",self.server,port),
        }
    }
}
