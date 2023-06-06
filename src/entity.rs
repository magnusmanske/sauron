use std::cmp::Ordering;

use mysql_async::Row;
use serde::{Serialize, Deserialize};


#[derive(Clone, Debug, Serialize, Deserialize, Eq)]
pub struct Entity {
    pub id: usize,
    pub name: String,
    pub child_ids: Vec<usize>,
    pub parent_ids: Vec<usize>,
    pub rights: Vec<String>,
}

impl Entity {
    pub fn from_row(row: &Row) -> Self {
        Self {
            id: row.get(0).unwrap(),
            name: row.get(1).unwrap(),
            child_ids: vec![],
            parent_ids: vec![],
            rights: vec![],
        }
    }
}

impl PartialOrd for Entity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Entity {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for Entity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}
