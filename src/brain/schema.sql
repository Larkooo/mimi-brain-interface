-- Entities: people, companies, services, concepts, anything
CREATE TABLE IF NOT EXISTS entities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    name TEXT NOT NULL,
    properties TEXT DEFAULT '{}',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

-- Relationships between entities
CREATE TABLE IF NOT EXISTS relationships (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    properties TEXT DEFAULT '{}',
    created_at TEXT DEFAULT (datetime('now'))
);

-- Memory index: links markdown files to entities they mention
CREATE TABLE IF NOT EXISTS memory_refs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    entity_id INTEGER REFERENCES entities(id) ON DELETE SET NULL,
    created_at TEXT DEFAULT (datetime('now'))
);

-- Full-text search over entities
CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
    name, type, properties,
    content=entities, content_rowid=id
);

-- Keep FTS in sync
CREATE TRIGGER IF NOT EXISTS entities_ai AFTER INSERT ON entities BEGIN
    INSERT INTO entities_fts(rowid, name, type, properties)
    VALUES (new.id, new.name, new.type, new.properties);
END;

CREATE TRIGGER IF NOT EXISTS entities_ad AFTER DELETE ON entities BEGIN
    INSERT INTO entities_fts(entities_fts, rowid, name, type, properties)
    VALUES ('delete', old.id, old.name, old.type, old.properties);
END;

CREATE TRIGGER IF NOT EXISTS entities_au AFTER UPDATE ON entities BEGIN
    INSERT INTO entities_fts(entities_fts, rowid, name, type, properties)
    VALUES ('delete', old.id, old.name, old.type, old.properties);
    INSERT INTO entities_fts(rowid, name, type, properties)
    VALUES (new.id, new.name, new.type, new.properties);
END;

-- Indexes
CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(type);
CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);
CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_id);
CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_id);
CREATE INDEX IF NOT EXISTS idx_relationships_type ON relationships(type);
CREATE INDEX IF NOT EXISTS idx_memory_refs_file ON memory_refs(file_path);
CREATE INDEX IF NOT EXISTS idx_memory_refs_entity ON memory_refs(entity_id);
