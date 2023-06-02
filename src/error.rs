// use std::{sync::Arc, num::ParseIntError, string::FromUtf8Error};
// use wikibase::mediawiki::media_wiki_error::MediaWikiError;

use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum RingError { // Lava etc
    String(String),
    MySQL(Arc<mysql_async::Error>),
    IO(Arc<std::io::Error>),
    Serde(Arc<serde_json::Error>),
    // Reqwest(Arc<reqwest::Error>),
    // ParseInt(ParseIntError),
    // Csv(Arc<csv::Error>),
    // FromUtf8(FromUtf8Error),
    // Ureq(Arc<ureq::Error>),
}

impl std::error::Error for RingError {}

impl std::fmt::Display for RingError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::String(s) => f.write_str(s),
            Self::MySQL(e) => f.write_str(&e.to_string()),
            Self::IO(e) => f.write_str(&e.to_string()),
            Self::Serde(e) => f.write_str(&e.to_string()),
            // Self::Reqwest(e) => f.write_str(&e.to_string()),
            // Self::ParseInt(e) => f.write_str(&e.to_string()),
            // Self::Csv(e) => f.write_str(&e.to_string()),
            // Self::FromUtf8(e) => f.write_str(&e.to_string()),
            // Self::Ureq(e) => f.write_str(&e.to_string()),
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

// impl From<reqwest::Error> for RingError {  
//     fn from(e: reqwest::Error) -> Self {Self::Reqwest(Arc::new(e))}
// }

// impl From<ParseIntError> for RingError {  
//     fn from(e: ParseIntError) -> Self {Self::ParseInt(e)}
// }

// impl From<csv::Error> for RingError {  
//     fn from(e: csv::Error) -> Self {Self::Csv(Arc::new(e))}
// }

// impl From<FromUtf8Error> for RingError {  
//     fn from(e: FromUtf8Error) -> Self {Self::FromUtf8(e)}
// }

// impl From<ureq::Error> for RingError {  
//     fn from(e: ureq::Error) -> Self {Self::Ureq(Arc::new(e))}
// }
