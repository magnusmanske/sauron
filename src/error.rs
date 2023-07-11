// use std::{sync::Arc, num::ParseIntError, string::FromUtf8Error};
// use wikibase::mediawiki::media_wiki_error::MediaWikiError;

use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum RingError { // Lava etc
    String(String),
    MySQL(Arc<mysql_async::Error>),
    IO(Arc<std::io::Error>),
    Serde(Arc<serde_json::Error>),
}

impl std::error::Error for RingError {}

impl std::fmt::Display for RingError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::String(s) => f.write_str(s),
            Self::MySQL(e) => f.write_str(&e.to_string()),
            Self::IO(e) => f.write_str(&e.to_string()),
            Self::Serde(e) => f.write_str(&e.to_string()),
        }
    }
}

impl From<String> for RingError {  
    fn from(e: String) -> Self {Self::String(e)}
}

impl From<&str> for RingError {  
    fn from(e: &str) -> Self {Self::String(e.to_string())}
}

impl From<mysql_async::Error> for RingError {  
    fn from(e: mysql_async::Error) -> Self {Self::MySQL(Arc::new(e))}
}

impl From<std::io::Error> for RingError {  
    fn from(e: std::io::Error) -> Self {Self::IO(Arc::new(e))}
}

impl From<serde_json::Error> for RingError {  
    fn from(e: serde_json::Error) -> Self {Self::Serde(Arc::new(e))}
}
