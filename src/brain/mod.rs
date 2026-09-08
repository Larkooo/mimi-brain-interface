use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use crate::paths;

const SCHEMA: &str = include_str!("schema.sql");

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

pub fn open() -> Connection {
    let db = Connection::open(paths::brain_db()).expect("failed to open brain.db");
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").ok();
    migrate(&db);
    db
}

/// Apply schema migrations that may be missing from older databases.
/// Each statement uses IF NOT EXISTS so it's safe to run repeatedly.
fn migrate(db: &Connection) {
    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS entities_update_timestamp \
         AFTER UPDATE ON entities \
         FOR EACH ROW \
         WHEN NEW.updated_at = OLD.updated_at \
         BEGIN \
             UPDATE entities SET updated_at = datetime('now') WHERE id = NEW.id; \
         END;"
    ).ok();
}

pub fn init() -> Connection {
    let db = open();
    db.execute_batch(SCHEMA).expect("failed to init schema");
    db
}

pub fn add_entity(db: &Connection, entity_type: &str, name: &str, properties: &str) -> Result<i64, String> {
    // Validate JSON
    let _: serde_json::Value =
        serde_json::from_str(properties).map_err(|e| format!("invalid JSON for properties: {e}"))?;

    db.execute(
        "INSERT INTO entities (type, name, properties) VALUES (?1, ?2, ?3)",
        params![entity_type, name, properties],
    )
    .map_err(|e| format!("failed to insert entity: {e}"))?;
    Ok(db.last_insert_rowid())
}

pub fn delete_entity(db: &Connection, id: i64) -> Result<(), String> {
    let changes = db
        .execute("DELETE FROM entities WHERE id = ?1", params![id])
        .map_err(|e| format!("failed to delete entity: {e}"))?;
    if changes == 0 {
        return Err(format!("entity #{} not found", id));
    }
    Ok(())
}

pub fn add_relationship(db: &Connection, source: i64, rel_type: &str, target: i64) -> Result<i64, String> {
    db.execute(
        "INSERT INTO relationships (source_id, target_id, type) VALUES (?1, ?2, ?3)",
        params![source, target, rel_type],
    )
    .map_err(|e| format!("failed to insert relationship: {e}"))?;
    Ok(db.last_insert_rowid())
}

pub fn find_entities(db: &Connection, entity_type: Option<&str>) -> Result<Vec<Entity>, String> {
    let mut sql = "SELECT id, type, name, properties, created_at, updated_at FROM entities"
        .to_string();
    if entity_type.is_some() {
        sql.push_str(" WHERE type = ?1");
    }
    sql.push_str(" ORDER BY updated_at DESC");

    let mut stmt = db.prepare(&sql).map_err(|e| format!("failed to prepare query: {e}"))?;
    let rows = if let Some(t) = entity_type {
        stmt.query_map(params![t], row_to_entity)
    } else {
        stmt.query_map([], row_to_entity)
    };
    Ok(rows
        .map_err(|e| format!("query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect())
}

/// Full-text search over entities.
///
/// `query` is whatever the caller typed. FTS5 has its own query grammar, so
/// everyday input — `nacer@x.com`, `mimi's laptop`, `who is alice?`, `e-mail`
/// — is a *syntax error*, not a search for those words. Handing such a query
/// straight to MATCH therefore finds nothing, which reads as "Mimi doesn't
/// know this person" rather than "the query was malformed".
///
/// So: run the query as written first (deliberate FTS5 syntax like
/// `alice OR bob`, `name:alice` or `ali*` keeps working), and if FTS5 rejects
/// it, retry it as a plain bag of words.
pub fn search_entities(db: &Connection, query: &str) -> Result<Vec<Entity>, String> {
    match run_fts(db, query) {
        Ok(hits) => Ok(hits),
        Err(raw_err) => match plain_text_query(query) {
            Some(fallback) => run_fts(db, &fallback).map_err(|_| raw_err),
            // Nothing searchable in the input (empty or pure punctuation).
            None => Ok(Vec::new()),
        },
    }
}

/// Rewrite arbitrary text as an FTS5 expression: each run of word characters
/// becomes a quoted term, all required (FTS5's implicit AND). Effectively
/// "treat every punctuation mark as a space". `None` when the input holds no
/// word characters at all.
///
/// Single-character terms are dropped when longer ones exist — they're nearly
/// always tokenizer debris (the `s` left by `nacer's`, the `x` in `a@x.com`)
/// and, being ANDed, would veto an otherwise good match.
fn plain_text_query(input: &str) -> Option<String> {
    let words: Vec<&str> = input
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();
    let meaningful: Vec<&str> = words.iter().copied().filter(|t| t.chars().count() > 1).collect();
    let terms = if meaningful.is_empty() { words } else { meaningful };
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn run_fts(db: &Connection, match_expr: &str) -> Result<Vec<Entity>, String> {
    let mut stmt = db
        .prepare(
            "SELECT e.id, e.type, e.name, e.properties, e.created_at, e.updated_at \
             FROM entities_fts fts JOIN entities e ON fts.rowid = e.id \
             WHERE entities_fts MATCH ?1 ORDER BY rank",
        )
        .map_err(|e| format!("failed to prepare search query: {e}"))?;
    let rows = stmt
        .query_map(params![match_expr], row_to_entity)
        .map_err(|e| format!("search query failed: {e}"))?;
    // Collected eagerly rather than with `filter_map(Result::ok)`: FTS5 reports
    // a malformed MATCH expression on the first step, not at prepare time, so
    // discarding row errors here is what silently turned a broken query into an
    // empty (but confident) result set.
    let mut hits = Vec::new();
    for row in rows {
        hits.push(row.map_err(|e| format!("search query failed: {e}"))?);
    }
    Ok(hits)
}

pub fn get_stats(db: &Connection) -> Result<Stats, String> {
    let entities: usize = db
        .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
        .unwrap_or(0);
    let relationships: usize = db
        .query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))
        .unwrap_or(0);
    let memory_refs: usize = db
        .query_row("SELECT COUNT(*) FROM memory_refs", [], |r| r.get(0))
        .unwrap_or(0);

    let mut stmt = db
        .prepare("SELECT type, COUNT(*) FROM entities GROUP BY type")
        .map_err(|e| format!("failed to query entity types: {e}"))?;
    let entity_types: Vec<(String, usize)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| format!("failed to query entity types: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt = db
        .prepare("SELECT type, COUNT(*) FROM relationships GROUP BY type")
        .map_err(|e| format!("failed to query relationship types: {e}"))?;
    let relationship_types: Vec<(String, usize)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| format!("failed to query relationship types: {e}"))?
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

#[derive(Debug, Serialize)]
pub struct GraphNode {
    pub id: i64,
    pub name: String,
    pub r#type: String,
    pub properties: serde_json::Value,
    pub connections: usize,
}

#[derive(Debug, Serialize)]
pub struct GraphLink {
    pub source: i64,
    pub target: i64,
    pub r#type: String,
}

#[derive(Debug, Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
}

pub fn get_graph(db: &Connection) -> Result<GraphData, String> {
    let mut stmt = db
        .prepare(
            "SELECT e.id, e.name, e.type, e.properties, \
             (SELECT COUNT(*) FROM relationships WHERE source_id = e.id OR target_id = e.id) AS connections \
             FROM entities e ORDER BY connections DESC",
        )
        .map_err(|e| format!("failed to prepare graph node query: {e}"))?;

    let nodes: Vec<GraphNode> = stmt
        .query_map([], |row| {
            let props_str: String = row.get(3)?;
            let properties = serde_json::from_str(&props_str)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            Ok(GraphNode {
                id: row.get(0)?,
                name: row.get(1)?,
                r#type: row.get(2)?,
                properties,
                connections: row.get(4)?,
            })
        })
        .map_err(|e| format!("graph node query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt = db
        .prepare("SELECT source_id, target_id, type FROM relationships")
        .map_err(|e| format!("failed to prepare graph link query: {e}"))?;

    let links: Vec<GraphLink> = stmt
        .query_map([], |row| {
            Ok(GraphLink {
                source: row.get(0)?,
                target: row.get(1)?,
                r#type: row.get(2)?,
            })
        })
        .map_err(|e| format!("graph link query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(GraphData { nodes, links })
}

pub fn raw_query(db: &Connection, sql: &str) -> Result<Vec<Vec<(String, String)>>, String> {
    let mut stmt = db.prepare(sql).map_err(|e| format!("invalid SQL: {e}"))?;
    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
        .collect();

    let rows = stmt
        .query_map([], |row| {
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
        })
        .map_err(|e| format!("query failed: {e}"))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A brain with a couple of people in it, on the schema `mimi setup` builds.
    fn fixture() -> Connection {
        let db = Connection::open_in_memory().expect("open in-memory db");
        db.execute_batch(SCHEMA).expect("init schema");
        add_entity(
            &db,
            "person",
            "Nacer Djaghmoum",
            r#"{"email": "nacer@example.com", "role": "e-mail admin"}"#,
        )
        .expect("insert entity");
        add_entity(&db, "person", "Alice", r#"{"note": "who is she"}"#).expect("insert entity");
        db
    }

    fn names(db: &Connection, query: &str) -> Vec<String> {
        search_entities(db, query)
            .unwrap_or_else(|e| panic!("search({query:?}) errored: {e}"))
            .into_iter()
            .map(|e| e.name)
            .collect()
    }

    #[test]
    fn plain_words_still_match() {
        let db = fixture();
        assert_eq!(names(&db, "nacer"), ["Nacer Djaghmoum"]);
        assert_eq!(names(&db, "nacer djaghmoum"), ["Nacer Djaghmoum"]);
    }

    #[test]
    fn punctuation_no_longer_swallows_the_match() {
        // Every one of these is an FTS5 syntax error verbatim, so before the
        // fallback they came back as a confident empty result set.
        let db = fixture();
        for query in [
            "nacer@example.com",
            "nacer's email",
            "e-mail admin",
            "\"nacer",
            "djaghmoum, nacer",
        ] {
            assert!(run_fts(&db, query).is_err(), "{query:?} should be invalid FTS5");
            assert_eq!(names(&db, query), ["Nacer Djaghmoum"], "query: {query:?}");
        }
    }

    #[test]
    fn punctuated_query_that_matches_nothing_stays_empty() {
        // Terms are ANDed, so a stranger's address at a known domain is a miss,
        // not a loose hit on "example"/"com".
        let db = fixture();
        assert!(names(&db, "tobias@example.com").is_empty());
    }

    #[test]
    fn deliberate_fts_syntax_is_preserved() {
        let db = fixture();
        assert_eq!(names(&db, "nacer OR nobody"), ["Nacer Djaghmoum"]);
        assert_eq!(names(&db, "name:nacer"), ["Nacer Djaghmoum"]);
        assert_eq!(names(&db, "nac*"), ["Nacer Djaghmoum"]);
        // Valid syntax that genuinely matches nothing stays empty.
        assert!(names(&db, "nacer AND nobody").is_empty());
    }

    #[test]
    fn unsearchable_input_is_empty_not_an_error() {
        let db = fixture();
        assert!(names(&db, "").is_empty());
        assert!(names(&db, "  ?! ").is_empty());
    }

    #[test]
    fn plain_text_query_quotes_each_word() {
        assert_eq!(plain_text_query("nacer@x.com").as_deref(), Some(r#""nacer" "com""#));
        assert_eq!(plain_text_query("mimi's laptop").as_deref(), Some(r#""mimi" "laptop""#));
        // Nothing but short terms — keep them rather than searching for nothing.
        assert_eq!(plain_text_query("x y").as_deref(), Some(r#""x" "y""#));
        assert_eq!(plain_text_query("  ?! "), None);
        assert_eq!(plain_text_query(""), None);
    }
}
