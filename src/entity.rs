use std::{cmp::Ordering, collections::HashMap};

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

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct EntityGroup {
    entities: HashMap<usize,Entity>,
}

impl EntityGroup {
    pub fn from_vec(entities: Vec<Entity>) -> Self {
        Self {
            entities: entities.iter().map(|e|(e.id,e.to_owned())).collect(),
        }
    }

    pub fn get(&self, id: usize) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut Entity> {
        self.entities.get_mut(&id)
    }

    pub fn as_vec(&self) -> Vec<Entity> {
        self.entities.values().cloned().collect()
    }

    pub fn as_sorted_vec(&self) -> Vec<Entity> {
        let mut ret = self.as_vec();
        ret.sort();
        ret
    }

    pub fn keys(&self) -> Vec<usize> {
        self.entities.keys().cloned().collect()
    }

}