use mysql_async::Row;

#[derive(Clone, Debug)]
pub struct DbTableAccess {
    pub id: usize,
    pub user_id: usize,
    pub entity_id: usize,
    pub right: String,
}

impl DbTableAccess {
    pub fn from_row(row: &Row) -> Self {
        Self {
            id: row.get(0).unwrap(),
            user_id: row.get(1).unwrap(),
            entity_id: row.get(2).unwrap(),
            right: row.get(3).unwrap(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DbTableConnection {
    pub id: usize,
    pub parent_id: usize,
    pub child_id: usize,
}

impl DbTableConnection {
    pub fn from_row(row: &Row) -> Self {
        Self {
            id: row.get(0).unwrap(),
            parent_id: row.get(1).unwrap(),
            child_id: row.get(2).unwrap(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DbTableEntity {
    pub id: usize,
    pub name: String,
}

impl DbTableEntity {
    pub fn from_row(row: &Row) -> Self {
        Self {
            id: row.get(0).unwrap(),
            name: row.get(1).unwrap(),
        }
    }
}



#[derive(Clone, Debug)]
pub struct DbTableSession {
    pub id: usize,
    pub id_string: String,
    pub json: String,
}

impl DbTableSession {
    pub fn from_row(row: &Row) -> Self {
        Self {
            id: row.get(0).unwrap(),
            id_string: row.get(1).unwrap(),
            json: row.get(2).unwrap(),
        }
    }
}
