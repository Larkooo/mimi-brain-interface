//! Task manager endpoints.
//!
//! Backed by `tasks` + `task_updates` tables in brain.db (also managed by
//! `~/.mimi/bin/task`). Infinite parent/child depth, status lifecycle, and
//! an append-only update log per task.

use axum::{
    Json,
    extract::{Path, Query},
    http::StatusCode,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::brain;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  parent_id INTEGER REFERENCES tasks(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  description TEXT DEFAULT '',
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending','running','blocked','done','failed','cancelled')),
  origin_channel TEXT,
  origin_chat_id TEXT,
  origin_user TEXT,
  assignee TEXT,
  depth INTEGER DEFAULT 0,
  progress INTEGER DEFAULT 0,
  metadata TEXT DEFAULT '{}',
  created_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now')),
  started_at TEXT,
  completed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);

CREATE TABLE IF NOT EXISTS task_updates (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  note TEXT,
  status_before TEXT,
  status_after TEXT,
  author TEXT,
  created_at TEXT DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_updates_task ON task_updates(task_id);
"#;

pub fn ensure_schema() {
    let db = brain::open();
    db.execute_batch(SCHEMA).ok();
}

#[derive(Serialize)]
pub struct Task {
    id: i64,
    parent_id: Option<i64>,
    title: String,
    description: String,
    status: String,
    origin_channel: Option<String>,
    origin_chat_id: Option<String>,
    origin_user: Option<String>,
    assignee: Option<String>,
    depth: i64,
    progress: i64,
    metadata: String,
    created_at: String,
    updated_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
}

#[derive(Serialize)]
pub struct TaskTreeNode {
    #[serde(flatten)]
    task: Task,
    path: String,
}

#[derive(Serialize)]
pub struct Update {
    id: i64,
    task_id: i64,
    note: Option<String>,
    status_before: Option<String>,
    status_after: Option<String>,
    author: Option<String>,
    created_at: String,
}

#[derive(Serialize)]
pub struct TaskDetail {
    #[serde(flatten)]
    task: Task,
    updates: Vec<Update>,
    children: Vec<Task>,
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        status: row.get(4)?,
        origin_channel: row.get(5)?,
        origin_chat_id: row.get(6)?,
        origin_user: row.get(7)?,
        assignee: row.get(8)?,
        depth: row.get(9)?,
        progress: row.get(10)?,
        metadata: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        started_at: row.get(14)?,
        completed_at: row.get(15)?,
    })
}

const TASK_COLS: &str = "id, parent_id, title, description, status, origin_channel, \
    origin_chat_id, origin_user, assignee, depth, progress, metadata, created_at, \
    updated_at, started_at, completed_at";

// --------------------------------------------------------------------
// GET /api/tasks
// --------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct ListParams {
    status: Option<String>,
    channel: Option<String>,
    parent: Option<String>,
    limit: Option<i64>,
}

pub async fn api_list(Query(params): Query<ListParams>) -> Result<Json<Vec<Task>>, (StatusCode, String)> {
    ensure_schema();
    let db = brain::open();
    let mut sql = format!("SELECT {TASK_COLS} FROM tasks WHERE 1=1");
    let mut args: Vec<String> = Vec::new();
    if let Some(s) = &params.status  { sql.push_str(" AND status=?");         args.push(s.clone()); }
    if let Some(c) = &params.channel { sql.push_str(" AND origin_channel=?"); args.push(c.clone()); }
    if let Some(p) = &params.parent {
        if p == "root" {
            sql.push_str(" AND parent_id IS NULL");
        } else if let Ok(pid) = p.parse::<i64>() {
            sql.push_str(" AND parent_id=?");
            args.push(pid.to_string());
        }
    }
    sql.push_str(
        " ORDER BY \
         CASE status WHEN 'running' THEN 0 WHEN 'blocked' THEN 1 WHEN 'pending' THEN 2 \
                     WHEN 'done' THEN 3 WHEN 'cancelled' THEN 4 WHEN 'failed' THEN 5 END, \
         updated_at DESC",
    );
    let limit = params.limit.unwrap_or(100).max(1).min(1000);
    sql.push_str(&format!(" LIMIT {limit}"));

    let mut stmt = db.prepare(&sql).map_err(to_err)?;
    let arg_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(arg_refs.as_slice(), row_to_task)
        .map_err(to_err)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(rows))
}

// --------------------------------------------------------------------
// GET /api/tasks/tree
// --------------------------------------------------------------------

pub async fn api_tree() -> Result<Json<Vec<TaskTreeNode>>, (StatusCode, String)> {
    ensure_schema();
    let db = brain::open();
    let sql = format!(
        "WITH RECURSIVE tree(id, d, path) AS (
           SELECT id, 0, printf('%08d', id) FROM tasks WHERE parent_id IS NULL
           UNION ALL
           SELECT t.id, tree.d+1, tree.path || '.' || printf('%08d', t.id)
             FROM tasks t JOIN tree ON t.parent_id = tree.id
         )
         SELECT {TASK_COLS}, tree.path FROM tree
           JOIN tasks USING (id)
         ORDER BY tree.path"
    );
    let mut stmt = db.prepare(&sql).map_err(to_err)?;
    let rows = stmt
        .query_map([], |row| {
            let task = row_to_task(row)?;
            let path: String = row.get(16)?;
            Ok(TaskTreeNode { task, path })
        })
        .map_err(to_err)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(rows))
}

// --------------------------------------------------------------------
// GET /api/tasks/summary
// --------------------------------------------------------------------

pub async fn api_summary() -> Result<Json<HashMap<String, i64>>, (StatusCode, String)> {
    ensure_schema();
    let db = brain::open();
    let mut stmt = db
        .prepare("SELECT status, COUNT(*) FROM tasks GROUP BY status")
        .map_err(to_err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(to_err)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(rows))
}

// --------------------------------------------------------------------
// GET /api/tasks/:id
// --------------------------------------------------------------------

pub async fn api_get(Path(id): Path<i64>) -> Result<Json<TaskDetail>, (StatusCode, String)> {
    ensure_schema();
    let db = brain::open();

    let task: Task = db
        .query_row(
            &format!("SELECT {TASK_COLS} FROM tasks WHERE id=?1"),
            params![id],
            row_to_task,
        )
        .map_err(|_| (StatusCode::NOT_FOUND, format!("task {id} not found")))?;

    let mut us = db
        .prepare(
            "SELECT id, task_id, note, status_before, status_after, author, created_at \
             FROM task_updates WHERE task_id=?1 ORDER BY created_at ASC, id ASC",
        )
        .map_err(to_err)?;
    let updates = us
        .query_map(params![id], |row| {
            Ok(Update {
                id: row.get(0)?,
                task_id: row.get(1)?,
                note: row.get(2)?,
                status_before: row.get(3)?,
                status_after: row.get(4)?,
                author: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(to_err)?
        .filter_map(|r| r.ok())
        .collect();

    let mut cs = db
        .prepare(&format!(
            "SELECT {TASK_COLS} FROM tasks WHERE parent_id=?1 ORDER BY created_at"
        ))
        .map_err(to_err)?;
    let children = cs
        .query_map(params![id], row_to_task)
        .map_err(to_err)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(TaskDetail { task, updates, children }))
}

// --------------------------------------------------------------------
// POST /api/tasks
// --------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateReq {
    title: String,
    description: Option<String>,
    parent_id: Option<i64>,
    origin_channel: Option<String>,
    origin_chat_id: Option<String>,
    origin_user: Option<String>,
    assignee: Option<String>,
}

pub async fn api_create(Json(req): Json<CreateReq>) -> Result<Json<Task>, (StatusCode, String)> {
    ensure_schema();
    let db = brain::open();
    let depth: i64 = match req.parent_id {
        Some(pid) => db
            .query_row("SELECT COALESCE(depth,0)+1 FROM tasks WHERE id=?1", params![pid], |r| r.get(0))
            .unwrap_or(0),
        None => 0,
    };
    let channel = req.origin_channel.as_deref().unwrap_or("web");
    let id: i64 = db
        .query_row(
            &format!(
                "INSERT INTO tasks(title, description, parent_id, origin_channel, origin_chat_id, \
                 origin_user, assignee, depth) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 RETURNING id"
            ),
            params![
                req.title,
                req.description.unwrap_or_default(),
                req.parent_id,
                channel,
                req.origin_chat_id,
                req.origin_user,
                req.assignee,
                depth,
            ],
            |r| r.get(0),
        )
        .map_err(to_err)?;
    db.execute(
        "INSERT INTO task_updates(task_id, status_before, status_after, author, note) \
         VALUES (?1, NULL, 'pending', ?2, 'created')",
        params![id, req.origin_user.as_deref().unwrap_or("mimi")],
    )
    .map_err(to_err)?;

    let task: Task = db
        .query_row(
            &format!("SELECT {TASK_COLS} FROM tasks WHERE id=?1"),
            params![id],
            row_to_task,
        )
        .map_err(to_err)?;
    Ok(Json(task))
}

// --------------------------------------------------------------------
// PATCH /api/tasks/:id
// --------------------------------------------------------------------

#[derive(Deserialize)]
pub struct UpdateReq {
    status: Option<String>,
    progress: Option<i64>,
    assignee: Option<String>,
    note: Option<String>,
    author: Option<String>,
}

pub async fn api_update(
    Path(id): Path<i64>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<Task>, (StatusCode, String)> {
    ensure_schema();
    let db = brain::open();

    let current_status: Option<String> = db
        .query_row("SELECT status FROM tasks WHERE id=?1", params![id], |r| r.get(0))
        .ok();
    if current_status.is_none() {
        return Err((StatusCode::NOT_FOUND, format!("task {id} not found")));
    }
    let current_status = current_status.unwrap();

    let mut sets: Vec<String> = Vec::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = &req.status {
        sets.push(format!("status=?{}", args.len() + 1));
        args.push(Box::new(s.clone()));
        if s == "running" && current_status != "running" {
            sets.push("started_at=COALESCE(started_at, datetime('now'))".to_string());
        }
        if matches!(s.as_str(), "done" | "failed" | "cancelled") {
            sets.push("completed_at=datetime('now')".to_string());
        }
    }
    if let Some(p) = req.progress {
        sets.push(format!("progress=?{}", args.len() + 1));
        args.push(Box::new(p));
    }
    if let Some(a) = &req.assignee {
        sets.push(format!("assignee=?{}", args.len() + 1));
        args.push(Box::new(a.clone()));
    }
    sets.push("updated_at=datetime('now')".to_string());

    let sql = format!(
        "UPDATE tasks SET {} WHERE id=?{}",
        sets.join(", "),
        args.len() + 1
    );
    args.push(Box::new(id));

    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    db.execute(&sql, refs.as_slice()).map_err(to_err)?;

    db.execute(
        "INSERT INTO task_updates(task_id, status_before, status_after, author, note) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id,
            current_status,
            req.status.clone().unwrap_or_else(|| current_status.clone()),
            req.author.as_deref().unwrap_or("web"),
            req.note,
        ],
    )
    .map_err(to_err)?;

    let task: Task = db
        .query_row(
            &format!("SELECT {TASK_COLS} FROM tasks WHERE id=?1"),
            params![id],
            row_to_task,
        )
        .map_err(to_err)?;
    Ok(Json(task))
}

// --------------------------------------------------------------------
// DELETE /api/tasks/:id
// --------------------------------------------------------------------

pub async fn api_delete(Path(id): Path<i64>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    ensure_schema();
    let db = brain::open();
    db.execute("DELETE FROM tasks WHERE id=?1", params![id])
        .map_err(to_err)?;
    Ok(Json(serde_json::json!({"ok": true, "id": id})))
}

fn to_err<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
