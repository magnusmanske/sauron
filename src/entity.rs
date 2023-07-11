use std::{cmp::Ordering, collections::HashMap};
use serde::{Serialize, Deserialize};
use crate::db_tables::DbTableEntity;

#[derive(Clone, Debug, Serialize, Deserialize, Eq)]
pub struct Entity {
    pub id: usize,
    pub name: String,
    pub external_id: String,
    pub child_ids: Vec<usize>,
    pub parent_ids: Vec<usize>,
    pub rights: Vec<String>,
}

impl Entity {
    pub fn from_table_entity(e: &DbTableEntity) -> Self {
        Self {
            id: e.id,
            name: e.name.to_owned(),
            external_id: e.external_id.to_owned(),
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

    pub fn empty() -> Self {
        Self {
            entities: HashMap::new(),
        }
    }

    pub fn has(&self, id: usize) -> bool {
        self.entities.contains_key(&id)
    }

    pub fn get(&self, id: usize) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut Entity> {
        self.entities.get_mut(&id)
    }

    pub fn get_create_mut(&mut self, entity: Entity) -> &mut Entity {
        self.entities.entry(entity.id).or_insert(entity)
    }

    pub fn as_vec(&self) -> Vec<Entity> {
        self.entities.values().cloned().collect()
    }

    pub fn as_sorted_vec(&self) -> Vec<Entity> {
        let mut ret = self.as_vec();
        ret.sort();
        ret
    }

    pub fn ids(&self) -> Vec<usize> {
        self.entities.keys().cloned().collect()
    }

    pub fn merge_from(&mut self, other: EntityGroup) {
        for (id,mut entity) in other.entities.into_iter() {
            match self.get_mut(id) {
                Some(original) => {
                    original.child_ids.append(&mut entity.child_ids);
                    original.child_ids.sort();
                    original.child_ids.dedup();
                    original.parent_ids.append(&mut entity.parent_ids);
                    original.parent_ids.sort();
                    original.parent_ids.dedup();
                    original.rights.append(&mut entity.rights);
                    original.rights.sort();
                    original.rights.dedup();
                }
                None => {
                    self.entities.insert(id, entity);
                }
            }
        }
    }

}