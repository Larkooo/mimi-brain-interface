use crate::brain as db;
use crate::paths;

fn ensure_brain() {
    if !paths::brain_db().exists() {
        eprintln!("Brain not initialized. Run `mimi setup` first.");
        std::process::exit(1);
    }
}

pub fn stats() {
    ensure_brain();
    let conn = db::open();
    let stats = match db::get_stats(&conn) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    println!("=== Brain Stats ===\n");
    println!("  Entities:      {}", stats.entities);
    println!("  Relationships: {}", stats.relationships);
    println!("  Memory refs:   {}", stats.memory_refs);

    if !stats.entity_types.is_empty() {
        println!("\n  Entity types:");
        for (t, c) in &stats.entity_types {
            println!("    {:15} {}", t, c);
        }
    }

    if !stats.relationship_types.is_empty() {
        println!("\n  Relationship types:");
        for (t, c) in &stats.relationship_types {
            println!("    {:15} {}", t, c);
        }
    }
}

pub fn query(sql: &str) {
    ensure_brain();
    let conn = db::open();
    let rows = match db::raw_query(&conn, sql) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("Query error: {e}");
            std::process::exit(1);
        }
    };

    if rows.is_empty() {
        println!("(no results)");
        return;
    }

    // Print header
    let cols: Vec<&str> = rows[0].iter().map(|(k, _)| k.as_str()).collect();
    println!("{}", cols.join("\t"));
    println!("{}", cols.iter().map(|c| "-".repeat(c.len().max(8))).collect::<Vec<_>>().join("\t"));

    // Print rows
    for row in &rows {
        let vals: Vec<&str> = row.iter().map(|(_, v)| v.as_str()).collect();
        println!("{}", vals.join("\t"));
    }
}

pub fn add(entity_type: &str, name: &str, properties: &str) {
    ensure_brain();
    let conn = db::open();
    let id = match db::add_entity(&conn, entity_type, name, properties) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };
    println!("Created entity #{}: {} ({})", id, name, entity_type);
}

pub fn link(source: i64, rel: &str, target: i64) {
    ensure_brain();
    let conn = db::open();
    let id = match db::add_relationship(&conn, source, rel, target) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };
    println!("Created relationship #{}: {} --[{}]--> {}", id, source, rel, target);
}

pub fn search(query: &str) {
    ensure_brain();
    let conn = db::open();
    let results = match db::search_entities(&conn, query) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    if results.is_empty() {
        println!("No results for '{}'", query);
        return;
    }

    for entity in &results {
        println!(
            "  #{:<4} {:12} {}  {}",
            entity.id,
            entity.r#type,
            entity.name,
            if entity.properties.is_object()
                && entity.properties.as_object().unwrap().is_empty()
            {
                String::new()
            } else {
                format!("  {}", entity.properties)
            }
        );
    }
}

pub fn list(entity_type: Option<&str>) {
    ensure_brain();
    let conn = db::open();
    let entities = match db::find_entities(&conn, entity_type) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    if entities.is_empty() {
        println!("No entities found.");
        return;
    }

    for entity in &entities {
        println!(
            "  #{:<4} {:12} {}",
            entity.id, entity.r#type, entity.name
        );
    }
}
