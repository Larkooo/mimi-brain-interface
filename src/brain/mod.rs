use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fmt;
use crate::paths;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug)]
pub enum BrainError {
    Db(rusqlite::Error),
    Json(serde_json::Error),
}

impl fmt::Display for BrainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrainError::Db(e) => write!(f, "database error: {e}"),
            BrainError::Json(e) => write!(f, "invalid JSON: {e}"),
        }
    }
}

impl From<rusqlite::Error> for BrainError {
    fn from(e: rusqlite::Error) -> Self { BrainError::Db(e) }
}

impl From<serde_json::Error> for BrainError {
    fn from(e: serde_json::Error) -> Self { BrainError::Json(e) }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entity {
    pub id: i64,
    pub r#type: String,
    pub name: String,
    pub properties: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Relationship {
    pub id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub r#type: String,
    pub properties: serde_json::Value,
    pub source_name: String,
    pub target_name: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Stats {
    pub entities: usize,
    pub relationships: usize,
    pub memory_refs: usize,
    pub entity_types: Vec<(String, usize)>,
    pub relationship_types: Vec<(String, usize)>,
}

pub fn open() -> Result<Connection, BrainError> {
    let db = Connection::open(paths::brain_db())?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(db)
}

pub fn init() -> Result<Connection, BrainError> {
    let db = open()?;
    db.execute_batch(SCHEMA)?;
    Ok(db)
}

pub fn add_entity(db: &Connection, entity_type: &str, name: &str, properties: &str) -> Result<i64, BrainError> {
    let _: serde_json::Value = serde_json::from_str(properties)?;
    db.execute(
        "INSERT INTO entities (type, name, properties) VALUES (?1, ?2, ?3)",
        params![entity_type, name, properties],
    )?;
    Ok(db.last_insert_rowid())
}

pub fn add_relationship(db: &Connection, source: i64, rel_type: &str, target: i64) -> Result<i64, BrainError> {
    db.execute(
        "INSERT INTO relationships (source_id, target_id, type) VALUES (?1, ?2, ?3)",
        params![source, target, rel_type],
    )?;
    Ok(db.last_insert_rowid())
}

pub fn find_entities(db: &Connection, entity_type: Option<&str>) -> Result<Vec<Entity>, BrainError> {
    let mut sql = "SELECT id, type, name, properties, created_at, updated_at FROM entities"
        .to_string();
    if entity_type.is_some() {
        sql.push_str(" WHERE type = ?1");
    }
    sql.push_str(" ORDER BY updated_at DESC");

    let mut stmt = db.prepare(&sql)?;
    let rows = if let Some(t) = entity_type {
        stmt.query_map(params![t], row_to_entity)?
    } else {
        stmt.query_map([], row_to_entity)?
    };
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn search_entities(db: &Connection, query: &str) -> Result<Vec<Entity>, BrainError> {
    let mut stmt = db.prepare(
        "SELECT e.id, e.type, e.name, e.properties, e.created_at, e.updated_at \
         FROM entities_fts fts JOIN entities e ON fts.rowid = e.id \
         WHERE entities_fts MATCH ?1 ORDER BY rank",
    )?;
    let rows = stmt.query_map(params![query], row_to_entity)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_stats(db: &Connection) -> Result<Stats, BrainError> {
    let entities: usize = db
        .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
        .unwrap_or(0);
    let relationships: usize = db
        .query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))
        .unwrap_or(0);
    let memory_refs: usize = db
        .query_row("SELECT COUNT(*) FROM memory_refs", [], |r| r.get(0))
        .unwrap_or(0);

    let mut stmt = db.prepare("SELECT type, COUNT(*) FROM entities GROUP BY type")?;
    let entity_types: Vec<(String, usize)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt = db.prepare("SELECT type, COUNT(*) FROM relationships GROUP BY type")?;
    let relationship_types: Vec<(String, usize)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Stats {
        entities,
        relationships,
        memory_refs,
        entity_types,
        relationship_types,
    })
}

pub fn raw_query(db: &Connection, sql: &str) -> Result<Vec<Vec<(String, String)>>, BrainError> {
    let mut stmt = db.prepare(sql)?;
    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
        .collect();

    let rows = stmt.query_map([], |row| {
        let mut cols = Vec::new();
        for i in 0..col_count {
            let val: String = row.get::<_, rusqlite::types::Value>(i).map(|v| match v {
                rusqlite::types::Value::Null => "NULL".to_string(),
                rusqlite::types::Value::Integer(i) => i.to_string(),
                rusqlite::types::Value::Real(f) => f.to_string(),
                rusqlite::types::Value::Text(s) => s,
                rusqlite::types::Value::Blob(b) => format!("<blob {} bytes>", b.len()),
            }).unwrap_or_else(|_| "?".to_string());
            cols.push((col_names[i].clone(), val));
        }
        Ok(cols)
    })?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn row_to_entity(row: &rusqlite::Row) -> rusqlite::Result<Entity> {
    let props_str: String = row.get(3)?;
    let properties =
        serde_json::from_str(&props_str).unwrap_or(serde_json::Value::Object(Default::default()));
    Ok(Entity {
        id: row.get(0)?,
        r#type: row.get(1)?,
        name: row.get(2)?,
        properties,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}
