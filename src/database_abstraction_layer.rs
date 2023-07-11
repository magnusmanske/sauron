use std::collections::HashMap;
use std::time::Duration;
use mysql_async::{prelude::*, from_row};
use mysql_async::{Conn, PoolOpts, PoolConstraints, OptsBuilder, Opts};
use serde_json::Value;
use crate::db_tables::{DbTableAccess, DbTableConnection, DbTableEntity};
use crate::error::RingError;
use crate::database_session_store::DatabaseSessionStore;
use crate::entity::{Entity, EntityGroup};
use crate::external_system::{ExternalSystemUser, ExternalSystem, ExternalAccessRequest};


#[derive(Clone, Debug)]
pub struct DatabaseAbstractionLayer {
    pub use_cached: bool,
    pub session_store: DatabaseSessionStore,
    pub db_pool: mysql_async::Pool,
    pub db_access: HashMap<usize,DbTableAccess>,
    pub db_connection: HashMap<usize,DbTableConnection>,
    pub db_entity: HashMap<usize,DbTableEntity>,
    pub db_user: HashMap<usize,ExternalSystemUser>,
    pub db_access_request: HashMap<usize,ExternalAccessRequest>,
}

impl DatabaseAbstractionLayer {
    pub async fn new(config: &Value) -> Result<Self,RingError> {
        let db_pool = Self::create_pool(&config["database"]);
        let mut ret = Self {
            use_cached: config["use_cache"].as_bool().unwrap_or(false),
            session_store: DatabaseSessionStore::new_with_pool(&db_pool).await?,
            db_pool: db_pool.clone(),
            db_access: HashMap::new(), // Not used if use_cache=false
            db_connection: HashMap::new(), // Not used if use_cache=false
            db_entity: HashMap::new(), // Not used if use_cache=false
            db_user: HashMap::new(), // Not used if use_cache=false
            db_access_request: HashMap::new(), // Not used if use_cache=false
            
        };
        ret.init_from_db().await?;
        Ok(ret)
    }

    /// Returns a connection to the GULP tool database
    pub async fn db_conn(&self) -> Result<Conn, mysql_async::Error> {
        self.db_pool.get_conn().await
    }


    // INTERFACE PUBLIC

    pub async fn load_entities(&self, entity_ids: &[usize]) -> Result<EntityGroup,RingError> {
        if self.use_cached {
            return self.load_entities_cached(entity_ids);
        } else {
            return self.load_entities_db(entity_ids).await;
        }
    }

    async fn load_entity_children(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize)>,RingError> {
        if self.use_cached {
            self.load_entity_children_cached(entity_ids)
        } else {
            self.load_entity_children_db(entity_ids).await
        }
    }

    pub async fn get_user(&self, user_id: usize) -> Result<ExternalSystemUser,RingError> {
        if self.use_cached {
            self.get_user_cached(user_id)
        } else {
            self.get_user_db(user_id).await
        }
    }

    pub async fn add_access_rights(&mut self, user_id: usize, entity_ids: Vec<usize>, rights: Vec<String>) -> Result<(),RingError> {
        let existing_rights: Vec<(usize,String)> = self.get_user_rights_for_entities(user_id).await?
            .into_iter()
            .filter(|(entity_id,_right)| entity_ids.contains(entity_id))
            .collect();
        let add_rights: Vec<(usize,String)> = entity_ids.iter()
            .map(|entity_id| rights.iter().map(|right|(*entity_id,right.to_owned())).collect::<Vec<(usize,String)>>() )
            .flatten()
            .filter(|x|!existing_rights.contains(x))
            .collect();
        for (entity_id,right) in &add_rights {
            self.add_right(user_id,*entity_id,right).await?;
        }
        Ok(())
    }

    pub async fn remove_access_rights(&mut self, user_id: usize, entity_ids: Vec<usize>, rights: Vec<String>) -> Result<(),RingError> {
        for entity_id in entity_ids {
            for right in &rights {
                self.remove_right(user_id,entity_id,right).await?;
            }
        }
        Ok(())
    }

    pub async fn request_access_rights(&mut self, user_id: usize, entity_ids: Vec<usize>, note: &str) -> Result<(),RingError> {
        for entity_id in entity_ids {
            self.request_right(user_id,entity_id,note).await?;
        }
        Ok(())
    }

    async fn request_right(&mut self, user_id: usize, entity_id: usize, note: &str) -> Result<(),RingError> {
        let sql = "REPLACE INTO `access_request` (`entity_id`,`user_id`,`note`) VALUES (:entity_id,:user_id,:note)";
        let mut conn = self.db_conn().await?;
        conn.exec_drop(sql, params!{entity_id,user_id,note}).await?;
        if self.use_cached {
            let id = match conn.last_insert_id() {
                Some(id) => id as usize,
                None => return Err(RingError::String("Failed to create new access request".into())),
            };
            let request = ExternalAccessRequest {
                id,
                user_id,
                entity_id,
                note: note.to_owned(),
            };
            self.db_access_request.insert(id,request);
        }
        Ok(())
    }

    pub async fn set_access_rights(&mut self, user_id: usize, entity_ids: Vec<usize>, rights: Vec<String>) -> Result<(),RingError> {
        let existing_rights: Vec<(usize,String)> = self.get_user_rights_for_entities(user_id).await?
            .into_iter()
            .filter(|(entity_id,_right)| entity_ids.contains(entity_id))
            .collect();
        let remove_rights: Vec<(usize,String)> = existing_rights.iter()
            .filter(|(_entity_id,right)| !rights.contains(right))
            .cloned()
            .collect();
        let new_rights: Vec<(usize,String)> = entity_ids.iter()
            .map(|entity_id| rights.iter().map(|right|(*entity_id,right.to_owned())).collect::<Vec<(usize,String)>>() )
            .flatten()
            .collect();
        let add_rights: Vec<_> = new_rights.into_iter()
            .filter(|x|!existing_rights.contains(x))
            .filter(|x|!remove_rights.contains(x)) // Paranoia
            .collect();
        for (entity_id,right) in &remove_rights {
            self.remove_right(user_id,*entity_id,right).await?;
        }
        for (entity_id,right) in &add_rights {
            self.add_right(user_id,*entity_id,right).await?;
        }
        Ok(())
    }


    /// Returns (user_id,entity_id,right)
    pub async fn get_all_direct_access_for_entities(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize,String)>,RingError> {
        if self.use_cached {
            self.get_all_direct_access_for_entities_cached(entity_ids)
        } else {
            self.get_all_direct_access_for_entities_db(entity_ids).await
        }
    }



    pub async fn get_all_rights_for_entity(&self, entity_id: usize) -> Result<Vec<(usize,String)>,RingError> {
        let mut all_parents = vec![entity_id];
        let mut todo = vec![entity_id];
        while !todo.is_empty() {
            todo = self.load_entity_parents(&todo).await?.iter().map(|(parent,_child)|*parent).collect();
            todo.retain(|entity_id|!all_parents.contains(entity_id));
            all_parents.append(&mut todo.clone());
            all_parents.sort();
            all_parents.dedup();
        }
        let mut access: Vec<(usize,String)> = self.get_all_direct_access_for_entities(&all_parents).await?
            .into_iter()
            .map(|(user_id,_entity_id,right)|(user_id,right.to_string()))
            .collect();
        access.sort();
        access.dedup();
        Ok(access)
    }

    pub async fn search_user_name(&self, query: &str) -> Result<Vec<usize>,RingError> {
        if self.use_cached {
            let query = query.to_lowercase();
            Ok(self.db_user.iter()
                .filter(|(_id,user)| user.name.to_lowercase().contains(&query))
                .map(|(id,_)|*id)
                .take(10)
                .collect())
        } else {
            let query = format!("%{query}%");
            let sql = "SELECT `id` FROM `user` WHERE `name` LIKE :query";
            Ok(self.db_conn().await?.exec_iter(sql,params!{query}).await?.map_and_drop( from_row::<usize>).await?)
        }
    }
    
    pub async fn search_access_rights(&self, query: &str) -> Result<Vec<String>,RingError> {
        if self.use_cached {
            let query = query.to_lowercase();
            let mut tmp: Vec<String> = self.db_access.iter()
                .filter(|(_id,access)| access.right.contains(&query))
                .map(|(_id,access)|access.right.to_owned())
                .collect();
            tmp.sort();
            tmp.dedup();
            Ok(tmp.into_iter().take(10).collect())
        } else {
            let query = format!("%{query}%");
            let sql = "SELECT DISTINCT `right` FROM `access` WHERE `right` LIKE :query";
            Ok(self.db_conn().await?.exec_iter(sql,params!{query}).await?.map_and_drop( from_row::<String>).await?)
        }
    }

    // ________________ DB PRIVATE

    async fn get_user_db(&self, user_id: usize) -> Result<ExternalSystemUser,RingError> {
        let sql = format!("SELECT `id`,`system`,`name`,`external_id`,`email`,`bespoke_data` FROM `user` WHERE `id`={user_id}");
        let res: Vec<ExternalSystemUser> = self.db_conn().await?.exec_iter(sql,()).await?.map_and_drop(|row|ExternalSystemUser::from_row(&row)).await?;
        res.get(0).map(|x|x.to_owned()).ok_or_else(||RingError::String("No such user".into()))
    }

    async fn load_entities_db(&self, entity_ids: &[usize]) -> Result<EntityGroup,RingError> {
        if entity_ids.is_empty() {
            return Ok(EntityGroup::from_vec(vec![]));
        }
        let entity_ids_str = entity_ids.iter().map(|s|format!("{s}")).collect::<Vec<String>>().join(",");
        let sql = format!("SELECT `id`,`name`,`external_id` FROM `entity` WHERE id IN ({})",entity_ids_str);
        let ret: Vec<Entity> = self.db_conn().await?
            .exec_iter(sql,()).await?
            .map_and_drop(|row| DbTableEntity::from_row(&row) ).await?
            .into_iter()
            .map(|e|Entity::from_table_entity(&e))
            .collect();
        Ok(EntityGroup::from_vec(ret))
    }

    async fn load_entity_children_db(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize)>,RingError> {
        if entity_ids.is_empty() {
            return Ok(vec![]);
        }
        let entity_ids_str = entity_ids.iter().map(|s|format!("{s}")).collect::<Vec<String>>().join(",");
        let sql = format!("SELECT `parent_id`,`child_id` FROM `connection` WHERE `parent_id` IN ({})",entity_ids_str);
        Ok(self.db_conn().await?.exec_iter(sql,()).await?.map_and_drop( from_row::<(usize,usize)>).await?)
    }

    async fn load_entity_parents_db(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize)>,RingError> {
        if entity_ids.is_empty() {
            return Ok(vec![]);
        }
        let entity_ids_str = entity_ids.iter().map(|s|format!("{s}")).collect::<Vec<String>>().join(",");
        let sql = format!("SELECT `parent_id`,`child_id` FROM `connection` WHERE `child_id` IN ({})",entity_ids_str);
        Ok(self.db_conn().await?.exec_iter(sql,()).await?.map_and_drop( from_row::<(usize,usize)>).await?)
    }

    async fn get_user_rights_for_entities_db(&self, user_id: usize) -> Result<Vec<(usize,String)>,RingError> {
        let sql = format!("SELECT `entity_id`,`right` FROM `access` WHERE `user_id`={user_id}");
        Ok(self.db_conn().await?.exec_iter(sql,()).await?.map_and_drop( from_row::<(usize,String)>).await?)
    }

    /// Returns (user_id,entity_id,right)
    async fn get_all_direct_access_for_entities_db(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize,String)>,RingError> {
        if entity_ids.is_empty() {
            return Ok(vec![]);
        }
        let entity_ids_str = entity_ids.iter().map(|s|format!("{s}")).collect::<Vec<String>>().join(",");
        let sql = format!("SELECT `user_id`,`entity_id`,`right` FROM `access` WHERE `entity_id` IN ({entity_ids_str})");
        Ok(self.db_conn().await?.exec_iter(sql,()).await?.map_and_drop( from_row::<(usize,usize,String)>).await?)
    }

    // ________________ CACHED PRIVATE

    fn get_user_cached(&self, user_id: usize) -> Result<ExternalSystemUser,RingError> {
        match self.db_user.get(&user_id) {
            Some(user) => Ok(user.to_owned()),
            None => Err(format!("No user with ID {user_id}").into()),
        }
    }

    fn load_entities_cached(&self, entity_ids: &[usize]) -> Result<EntityGroup,RingError> {
        let v: Vec<Entity> = entity_ids
            .iter()
            .filter_map(|id|self.db_entity.get(id))
            .map(|e|Entity::from_table_entity(e))
            .collect();
        Ok(EntityGroup::from_vec(v))
    }

    /// Returns Vec<(parent,child)>
    fn load_entity_children_cached(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize)>,RingError> {
        Ok(self.db_connection
            .iter()
            .filter(|(_id,c)|entity_ids.contains(&c.parent_id))
            .map(|(_id,c)|(c.parent_id,c.child_id))
            .collect())
    }

    /// Returns Vec<(parent,child)>
    fn load_entity_parents_cached(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize)>,RingError> {
        Ok(self.db_connection
            .iter()
            .filter(|(_id,c)|entity_ids.contains(&c.child_id))
            .map(|(_id,c)|(c.parent_id,c.child_id))
            .collect())
    }

    fn get_user_rights_for_entities_cached(&self, user_id: usize) -> Result<Vec<(usize,String)>,RingError> {
        Ok(self.db_access
            .iter()
            .filter(|(_id,a)|a.user_id==user_id)
            .map(|(_id,a)|(a.entity_id,a.right.to_owned()))
            .collect())
    }

    /// Returns (user_id,entity_id,right)
    fn get_all_direct_access_for_entities_cached(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize,String)>,RingError> {
        Ok(self.db_access.iter()
            .filter(|(_row_id,access)| entity_ids.contains(&access.entity_id))
            .map(|(_row_id,access)|(access.user_id,access.entity_id,access.right.to_owned()))
            .collect())
    }


    // MISC PUBLIC METHODS

    // Adds child and parent nodes
    pub async fn annotate_entities(&self, entities: &mut EntityGroup) -> Result<(),RingError> {
        self.load_entity_children(&entities.ids()).await?
            .iter()
            .for_each(|(parent,child)|{
                if let Some(entity) = entities.get_mut(*parent) {
                    entity.child_ids.push(*child)
                }
            });
        self.load_entity_parents(&entities.ids()).await?
            .iter()
            .for_each(|(parent,child)|{
                if let Some(entity) = entities.get_mut(*child) {
                    entity.parent_ids.push(*parent)
                }
            });
        Ok(())
    }

    pub async fn add_user(&mut self, system: &str, external_id: &str, name: &str, email: &str, bespoke_data: &str) -> Result<Option<u64>,RingError> {
        let sql = r#"INSERT INTO `user` (`system`,`external_id`,`name`,`bespoke_data`) 
            VALUES (:system,:external_id,:name,:bespoke_data) 
            ON DUPLICATE KEY UPDATE `bespoke_data`=:bespoke_data"# ;
        let mut conn = self.db_conn().await?;
        conn.exec_drop(sql, params!{system,external_id,name,bespoke_data}).await?;
        let user_id = conn.last_insert_id();
        match user_id {
            Some(user_id) => {
                let user = ExternalSystemUser{
                    id: Some(user_id),
                    system: ExternalSystem::from_str(system),
                    name: name.to_string(),
                    external_id: external_id.to_string(),
                    email: email.to_string(),
                    bespoke_data: serde_json::from_str(&bespoke_data).unwrap_or(Value::Null)
                };
                self.db_user.insert(user_id as usize,user);
            }
            None => {},
        }
        Ok(user_id)

    }  

    pub async fn get_entities_with_user_access(&self, user_id: usize, special_right: Option<String>) -> Result<EntityGroup,RingError> {
        let mut res: Vec<(usize,String)> = self.get_user_rights_for_entities(user_id).await?;
        if let Some(right) = special_right {
            res = res.into_iter().filter(|(_id,r)|*r==right).collect()
        }
        
        let mut entity_ids: Vec<usize> = res
            .iter()
            .map(|(id,_right)|*id)
            .collect();
        entity_ids.sort();
        entity_ids.dedup();

        let mut entities = self.load_entities(&entity_ids).await?;
        res
            .iter()
            .for_each(|(id,right)|{
                if let Some(entity) = entities.get_mut(*id) {
                    entity.rights.push(right.to_owned())
                }
            });
        self.annotate_entities(&mut entities).await?;
        Ok(entities)
    }

    pub async fn get_all_user_rights_for_entities(&self, user_id: usize, special_right: Option<String>) -> Result<EntityGroup,RingError> {
        let mut entities = self.get_entities_with_user_access(user_id,special_right).await?; // Seed
        let mut last_ids = entities.ids();
        while !last_ids.is_empty() {
            let parent_child = self.load_entity_children(&last_ids).await?;
            let child_ids: Vec<usize> = parent_child.iter().map(|(_parent,child)|*child).collect();
            let new_child_ids: Vec<usize> = child_ids.into_iter().filter(|id|!entities.has(*id)).collect();
            let new_entities = self.load_entities(&new_child_ids).await?;
            last_ids.clear();

            for (parent_id,child_id) in parent_child.into_iter() {
                let parent = entities.get(parent_id);
                let new_entity = new_entities.get(child_id);
                match (parent,new_entity) {
                    (Some(parent),Some(new_entity)) => {
                        let mut rights = parent.rights.clone();
                        let child = entities.get_create_mut(new_entity.to_owned());
                        child.rights.append(&mut rights);
                        child.rights.sort();
                        child.rights.dedup();
                        last_ids.push(child.id);
                    },
                    _ => {},
                }
            }

            last_ids.sort();
            last_ids.dedup();
        }
        Ok(entities)
    }

    // PRIVATE METHODS

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

    async fn init_from_db(&mut self) -> Result<(),RingError> {
        if !self.use_cached {
            return Ok(());
        }
        let mut conn = self.db_conn().await?;
        self.db_access = conn
            .exec_iter("SELECT `id`,`user_id`,`entity_id`,`right` FROM `access`",()).await?
            .map_and_drop(|row| DbTableAccess::from_row(&row) ).await?.into_iter().map(|x|(x.id,x)).collect();
        self.db_connection = conn
            .exec_iter("SELECT `id`,`parent_id`,`child_id` FROM `connection`",()).await?
            .map_and_drop(|row| DbTableConnection::from_row(&row) ).await?.into_iter().map(|x|(x.id,x)).collect();
        self.db_entity = conn
            .exec_iter("SELECT `id`,`name`,`external_id` FROM `entity`",()).await?
            .map_and_drop(|row| DbTableEntity::from_row(&row) ).await?.into_iter().map(|x|(x.id,x)).collect();
        self.db_user = conn
            .exec_iter("SELECT `id`,`system`,`name`,`external_id`,`email`,`bespoke_data` FROM `user`",()).await?
            .map_and_drop(|row| ExternalSystemUser::from_row(&row) ).await?.into_iter().map(|x|(x.id.unwrap() as usize,x)).collect();
        self.db_access_request = conn
            .exec_iter("SELECT `id`,`user_id`,`entity_id`,`note` FROM `access_request`",()).await?
            .map_and_drop(|row| ExternalAccessRequest::from_row(&row) ).await?.into_iter().map(|x|(x.id,x)).collect();
        Ok(())
    }

    pub async fn get_access_requests(&self, entity_id: usize) -> Result<Vec<ExternalAccessRequest>,RingError> {
        let ret = if self.use_cached {
            self.db_access_request.iter()
                .filter(|(_id,ar)| ar.entity_id==entity_id)
                .map(|(_id,ar)| ar)
                .cloned()
                .collect()
        } else {
            let sql = "SELECT `id`,`user_id`,`entity_id`,`note` FROM `access_request` WHERE `entity_id`=:entity_id" ;
            self.db_conn().await?
                .exec_iter(sql,params!{entity_id}).await?
                .map_and_drop(|row| ExternalAccessRequest::from_row(&row) ).await?.into_iter().collect()
        };
        Ok(ret)
    }

    async fn remove_right(&mut self, user_id: usize, entity_id: usize, right: &str) -> Result<(),RingError> {
        // Delete from database
        let sql = "DELETE FROM `access` WHERE `user_id`=:user_id AND `entity_id`=:entity_id AND `right`=:right";
        self.db_conn().await?.exec_drop(sql, params!{user_id,entity_id,right}).await?;

        // Delete from cache
        if self.use_cached {
            let id = self.db_access.iter()
                .find(|(_id,entry)| entry.user_id==user_id && entry.entity_id==entity_id && entry.right==right)
                .map(|(id,_entry)| *id);
            if let Some(id) = id {
                self.db_access.remove(&id);
            }
        }

        Ok(())
    }

    async fn add_right(&mut self, user_id: usize, entity_id: usize, right: &str) -> Result<(),RingError> {
        let mut conn = self.db_conn().await?;

        // Add access
        let sql = "INSERT IGNORE INTO `access` (`user_id`,`entity_id`,`right`) VALUES (:user_id,:entity_id,:right)";
        conn.exec_drop(sql, params!{user_id,entity_id,right}).await?;
        let access_id_opt = conn.last_insert_id();

        // Remove request, if exists
        let sql = "DELETE FROM `access_request` WHERE `user_id`=:user_id AND `entity_id`=:entity_id" ;
        conn.exec_drop(sql, params!{user_id,entity_id}).await?;

        if self.use_cached {
            // Add to cache
            if let Some(id) = access_id_opt {
                let id = id as usize;
                self.db_access.insert(id,DbTableAccess{ id: id, user_id, entity_id, right: right.to_string() });
            }
            // Remove request from cache
            self.db_access_request.retain(|_id,ar| ar.user_id!=user_id || ar.entity_id!=entity_id);
        }

        Ok(())
    }

    pub async fn create_child_entity(&mut self, parent_id: usize, name: &str, ext_id: &str) -> Result<usize,RingError> {
        let sql = "INSERT INTO `entity` (`name`,`external_id`) VALUES (:name,:ext_id)" ;
        let mut conn = self.db_conn().await?;
        conn.exec_drop(sql, params!{name,ext_id}).await?;

        let child_id = match conn.last_insert_id() {
            Some(id) => id as usize,
            None => return Err(RingError::String("Failed to create new entity".into())),
        };

        // Add to cache
        if self.use_cached {
            self.db_entity.insert(child_id,DbTableEntity{ id: child_id, name: name.to_owned(), external_id: ext_id.to_owned() });
        }

        let sql = "INSERT INTO `connection` (`parent_id`,`child_id`) VALUES (:parent_id,:child_id)";
        conn.exec_drop(sql, params!{parent_id,child_id}).await?;

        // Add to cache
        if self.use_cached {
            if let Some(id) = conn.last_insert_id() {
                let id = id as usize;
                self.db_connection.insert(id,DbTableConnection { id: id, parent_id, child_id});
            }
        }        

        Ok(child_id)
    }

    /// Returns Vec<(parent,child)>
    async fn load_entity_parents(&self, entity_ids: &[usize]) -> Result<Vec<(usize,usize)>,RingError> {
        if self.use_cached {
            self.load_entity_parents_cached(entity_ids)
        } else {
            self.load_entity_parents_db(entity_ids).await
        }
    }

    async fn get_user_rights_for_entities(&self, user_id: usize) -> Result<Vec<(usize,String)>,RingError> {
        if self.use_cached {
            self.get_user_rights_for_entities_cached(user_id)
        } else {
            self.get_user_rights_for_entities_db(user_id).await
        }
    }

}