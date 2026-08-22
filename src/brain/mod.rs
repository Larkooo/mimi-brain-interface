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

/// Split an arbitrary user string into FTS5-safe quoted terms.
///
/// FTS5's MATCH argument is a query language, not a plain string: punctuation
/// is syntax and bare `AND`/`OR`/`NOT` are operators. So the things mimi
/// actually searches for — `alice@corp.com`, `O'Brien`, `who is alice?`,
/// `mimi-brain-interface` — all failed with an fts5 syntax error instead of
/// returning results. Tokenizing on non-alphanumerics and wrapping each token
/// in double quotes makes every term a literal, which is always valid syntax.
///
/// Returns an empty vec when there is nothing searchable (e.g. `"???"`).
fn fts_terms(query: &str) -> Vec<String> {
    // Splitting on non-alphanumerics means a token can never contain a double
    // quote, so wrapping is enough — no escaping needed.
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect()
}

fn run_fts(db: &Connection, match_expr: &str) -> Result<Vec<Entity>, String> {
    let mut stmt = db
        .prepare(
            "SELECT e.id, e.type, e.name, e.properties, e.created_at, e.updated_at \
             FROM entities_fts fts JOIN entities e ON fts.rowid = e.id \
             WHERE entities_fts MATCH ?1 ORDER BY rank",
        )
        .map_err(|e| format!("failed to prepare search query: {e}"))?;
    Ok(stmt
        .query_map(params![match_expr], row_to_entity)
        .map_err(|e| format!("search query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect())
}

pub fn search_entities(db: &Connection, query: &str) -> Result<Vec<Entity>, String> {
    let terms = fts_terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    // Require every term first — that's the precise answer when the caller
    // knows what they're looking for. If nothing matches, fall back to any
    // term so a conversational query ("who is alice?", where "who" and "is"
    // appear nowhere in the graph) still finds Alice. `ORDER BY rank` puts the
    // entities matching the most terms on top either way.
    let hits = run_fts(db, &terms.join(" "))?;
    if !hits.is_empty() || terms.len() < 2 {
        return Ok(hits);
    }
    run_fts(db, &terms.join(" OR "))
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

    fn db_with(rows: &[(&str, &str)]) -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        for (name, properties) in rows {
            db.execute(
                "INSERT INTO entities (type, name, properties) VALUES ('person', ?1, ?2)",
                params![name, properties],
            )
            .unwrap();
        }
        db
    }

    fn names(entities: &[Entity]) -> Vec<&str> {
        entities.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn terms_are_quoted() {
        assert_eq!(fts_terms("alice smith"), ["\"alice\"", "\"smith\""]);
    }

    #[test]
    fn punctuation_is_a_term_boundary() {
        assert_eq!(fts_terms("alice@corp.com"), ["\"alice\"", "\"corp\"", "\"com\""]);
        assert_eq!(fts_terms("O'Brien"), ["\"O\"", "\"Brien\""]);
        assert_eq!(fts_terms("mimi-brain"), ["\"mimi\"", "\"brain\""]);
    }

    #[test]
    fn fts_operators_are_neutralized() {
        // Bare AND/OR/NOT are FTS5 operators; quoting makes them literals.
        assert_eq!(fts_terms("cats AND"), ["\"cats\"", "\"AND\""]);
    }

    #[test]
    fn tokenless_query_has_no_terms() {
        assert!(fts_terms("???").is_empty());
        assert!(fts_terms("   ").is_empty());
    }

    #[test]
    fn punctuated_queries_return_results_instead_of_erroring() {
        let db = db_with(&[("Alice Smith", r#"{"email": "alice@corp.com"}"#)]);

        // Each of these was an `fts5: syntax error` before.
        for q in ["alice@corp.com", "O'Brien or alice", "alice-smith", "alice?"] {
            let hits = search_entities(&db, q).unwrap_or_else(|e| panic!("{q}: {e}"));
            assert_eq!(names(&hits), ["Alice Smith"], "query: {q}");
        }
    }

    #[test]
    fn conversational_query_falls_back_to_any_term() {
        let db = db_with(&[("Alice Smith", "{}")]);
        // "who"/"is" appear nowhere, so requiring every term finds nothing.
        assert_eq!(names(&search_entities(&db, "who is alice?").unwrap()), ["Alice Smith"]);
    }

    #[test]
    fn all_terms_wins_over_any_term_when_both_could_match() {
        let db = db_with(&[("Alice Smith", "{}"), ("Bob Smith", "{}")]);
        // Bob also matches "smith", but the all-terms pass succeeds so the
        // fallback never runs.
        assert_eq!(names(&search_entities(&db, "alice smith").unwrap()), ["Alice Smith"]);
    }

    #[test]
    fn tokenless_search_is_empty_not_an_error() {
        let db = db_with(&[("Alice Smith", "{}")]);
        assert!(search_entities(&db, "???").unwrap().is_empty());
    }
}
