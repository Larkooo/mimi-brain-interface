//! Background task registry.
//!
//! The channel agent (or the CLI) records long-running workflows here so the
//! user can see status without blocking the main conversation.
//!
//! Tasks live in the `tasks` + `task_updates` tables of `~/.mimi/brain.db` —
//! the *same* store the dashboard serves at `/api/tasks` and the
//! `~/.mimi/bin/task` wrapper writes to. A task registered from a channel
//! turn, from this CLI, or from the web UI is therefore visible from all
//! three surfaces.
//!
//! This registry does NOT spawn the work itself. The expectation is that the
//! channel agent uses Claude Code's `Task` tool with `run_in_background: true`
//! (or shells out to a detached process) and records progress here so status
//! is queryable from any surface (other channel, CLI, dashboard).
//!
//! Progress notes and status transitions are appended to `task_updates`, the
//! shared append-only log. `mimi task set-pid` / `mimi task result` stash
//! their values under the row's `metadata` JSON so `mimi task stop` can
//! signal the worker.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::brain;
use crate::paths;

/// Canonical schema for the shared task store. Idempotent — every statement
/// is `IF NOT EXISTS`, so it is safe to run on every command.
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

/// Create the task tables if they're missing. Cheap enough to call per
/// request; the dashboard handlers do exactly that.
pub fn ensure_schema() {
    let db = brain::open();
    db.execute_batch(SCHEMA).ok();
}

fn open() -> Result<Connection, String> {
    if !paths::brain_db().exists() {
        return Err("Mimi is not set up yet. Run `mimi setup` first.".into());
    }
    let db = brain::open();
    db.execute_batch(SCHEMA)
        .map_err(|e| format!("failed to ensure task schema: {e}"))?;
    Ok(db)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Running,
    Blocked,
    Done,
    Failed,
    Cancelled,
}

impl Status {
    fn parse(s: &str) -> Result<Status, String> {
        match s.to_ascii_lowercase().as_str() {
            "pending" => Ok(Status::Pending),
            "running" => Ok(Status::Running),
            "blocked" => Ok(Status::Blocked),
            "done" => Ok(Status::Done),
            "failed" => Ok(Status::Failed),
            "cancelled" | "canceled" => Ok(Status::Cancelled),
            other => Err(format!(
                "unknown status: {other} (expected pending|running|blocked|done|failed|cancelled)"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Running => "running",
            Status::Blocked => "blocked",
            Status::Done => "done",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub title: String,
    pub description: String,
    pub status: String,
    pub origin_channel: Option<String>,
    pub origin_chat_id: Option<String>,
    pub origin_user: Option<String>,
    pub assignee: Option<String>,
    pub progress: i64,
    pub metadata: String,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

impl Task {
    /// The pid recorded by `mimi task set-pid`, if any.
    pub fn pid(&self) -> Option<i32> {
        serde_json::from_str::<Value>(&self.metadata)
            .ok()?
            .get("pid")?
            .as_i64()
            .map(|p| p as i32)
    }
}

const COLS: &str = "id, parent_id, title, description, status, origin_channel, \
    origin_chat_id, origin_user, assignee, progress, metadata, created_at, \
    updated_at, started_at, completed_at";

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
        progress: row.get(9)?,
        metadata: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        started_at: row.get(13)?,
        completed_at: row.get(14)?,
    })
}

fn fetch(db: &Connection, id: i64) -> Result<Task, String> {
    db.query_row(
        &format!("SELECT {COLS} FROM tasks WHERE id = ?1"),
        params![id],
        row_to_task,
    )
    .map_err(|_| format!("task {id} not found"))
}

/// Append a row to the shared update log. `before`/`after` are only set on
/// real status transitions so the log stays readable.
fn log_update(
    db: &Connection,
    id: i64,
    before: Option<&str>,
    after: Option<&str>,
    note: Option<&str>,
) -> Result<(), String> {
    db.execute(
        "INSERT INTO task_updates(task_id, status_before, status_after, author, note) \
         VALUES (?1, ?2, ?3, 'cli', ?4)",
        params![id, before, after, note],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to record task update: {e}"))
}

/// Merge a single key into the row's `metadata` JSON object, preserving any
/// keys written by the other surfaces.
fn merge_metadata(db: &Connection, id: i64, key: &str, value: Value) -> Result<(), String> {
    let raw: String = db
        .query_row(
            "SELECT COALESCE(metadata, '{}') FROM tasks WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|_| format!("task {id} not found"))?;
    let mut obj = serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    obj.insert(key.to_string(), value);
    let encoded = Value::Object(obj).to_string();
    db.execute(
        "UPDATE tasks SET metadata = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![encoded, id],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to update task metadata: {e}"))
}

pub fn new(title: &str, spawner: &str) -> Result<Task, String> {
    let db = open()?;
    let id: i64 = db
        .query_row(
            "INSERT INTO tasks(title, origin_channel) VALUES (?1, ?2) RETURNING id",
            params![title, spawner],
            |r| r.get(0),
        )
        .map_err(|e| format!("failed to create task: {e}"))?;
    log_update(&db, id, None, Some("pending"), Some("created"))?;
    fetch(&db, id)
}

pub fn load(id: i64) -> Result<Task, String> {
    let db = open()?;
    fetch(&db, id)
}

pub fn update_status(id: i64, status: Status) -> Result<Task, String> {
    let db = open()?;
    let before: String = db
        .query_row("SELECT status FROM tasks WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .map_err(|_| format!("task {id} not found"))?;

    let mut sets = vec!["status = ?1".to_string(), "updated_at = datetime('now')".to_string()];
    if status == Status::Running && before != "running" {
        sets.push("started_at = COALESCE(started_at, datetime('now'))".into());
    }
    if matches!(status, Status::Done | Status::Failed | Status::Cancelled) {
        sets.push("completed_at = datetime('now')".into());
    }
    if status == Status::Done {
        sets.push("progress = 100".into());
    }
    db.execute(
        &format!("UPDATE tasks SET {} WHERE id = ?2", sets.join(", ")),
        params![status.as_str(), id],
    )
    .map_err(|e| format!("failed to update task {id}: {e}"))?;

    log_update(&db, id, Some(&before), Some(status.as_str()), None)?;
    fetch(&db, id)
}

pub fn set_pid(id: i64, pid: i32) -> Result<(), String> {
    let db = open()?;
    merge_metadata(&db, id, "pid", Value::from(pid))
}

pub fn set_result(id: i64, result: &str) -> Result<(), String> {
    let db = open()?;
    merge_metadata(&db, id, "result", Value::from(result))?;
    log_update(&db, id, None, None, Some(&format!("result: {result}")))
}

pub fn append_log(id: i64, message: &str) -> Result<(), String> {
    let db = open()?;
    // Fail loudly on an unknown id rather than letting the FK error surface
    // as an opaque "constraint failed".
    fetch(&db, id)?;
    log_update(&db, id, None, None, Some(message))
}

/// Render the task's update history as plain text, one entry per line.
pub fn read_log(id: i64) -> Result<String, String> {
    let db = open()?;
    fetch(&db, id)?;
    let mut stmt = db
        .prepare(
            "SELECT created_at, author, status_before, status_after, note \
             FROM task_updates WHERE task_id = ?1 ORDER BY created_at ASC, id ASC",
        )
        .map_err(|e| format!("failed to read task log: {e}"))?;
    let rows = stmt
        .query_map(params![id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| format!("failed to read task log: {e}"))?;

    let mut out = String::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (created_at, author, before, after, note) = row;
        let who = author.unwrap_or_else(|| "?".into());
        let transition = match (before, after) {
            (Some(b), Some(a)) if b != a => format!("{b} → {a}"),
            (None, Some(a)) => format!("→ {a}"),
            _ => String::new(),
        };
        let body = match (transition.is_empty(), note) {
            (true, Some(n)) => n,
            (false, Some(n)) => format!("{transition}: {n}"),
            (false, None) => transition,
            (true, None) => continue,
        };
        out.push_str(&format!("[{created_at}] ({who}) {body}\n"));
    }
    Ok(out)
}

/// Every task, most actionable first (running → blocked → pending → …), then
/// most recently touched. Same ordering the dashboard and `bin/task` use.
pub fn list() -> Result<Vec<Task>, String> {
    let db = open()?;
    let sql = format!(
        "SELECT {COLS} FROM tasks ORDER BY \
         CASE status WHEN 'running' THEN 0 WHEN 'blocked' THEN 1 WHEN 'pending' THEN 2 \
                     WHEN 'done' THEN 3 WHEN 'cancelled' THEN 4 WHEN 'failed' THEN 5 END, \
         updated_at DESC"
    );
    let mut stmt = db
        .prepare(&sql)
        .map_err(|e| format!("failed to list tasks: {e}"))?;
    let rows = stmt
        .query_map([], row_to_task)
        .map_err(|e| format!("failed to list tasks: {e}"))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn stop(id: i64) -> Result<(), String> {
    let task = load(id)?;
    if let Some(pid) = task.pid() {
        // SIGTERM — the spawned subagent is expected to handle cleanup.
        unsafe {
            if libc::kill(pid, libc::SIGTERM) != 0 {
                let err = std::io::Error::last_os_error();
                // ESRCH (no such process) is fine — the task probably already
                // exited and just hasn't been marked cancelled yet.
                if err.raw_os_error() != Some(libc::ESRCH) {
                    return Err(format!("kill({pid}) failed: {err}"));
                }
            }
        }
    }
    update_status(id, Status::Cancelled).map(|_| ())
}

// --- CLI entry points ---

/// Task ids are `tasks.id` rowids, matching the dashboard and
/// `~/.mimi/bin/task`. Clap hands them to us as strings.
fn parse_id(raw: &str) -> i64 {
    match raw.trim().parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            eprintln!("invalid task id: {raw} (expected a number, e.g. 42)");
            std::process::exit(1);
        }
    }
}

fn die(e: String) -> ! {
    eprintln!("{e}");
    std::process::exit(1);
}

pub fn cli_new(title: &str, spawner: &str) {
    match new(title, spawner) {
        Ok(task) => println!("{}", task.id),
        Err(e) => die(format!("task new failed: {e}")),
    }
}

pub fn cli_list() {
    let tasks = match list() {
        Ok(t) => t,
        Err(e) => die(e),
    };
    if tasks.is_empty() {
        println!("(no tasks)");
        return;
    }
    println!("{:<6} {:<10} {:<20} {}", "ID", "STATUS", "UPDATED", "TITLE");
    for t in tasks {
        println!(
            "{:<6} {:<10} {:<20} {}",
            t.id, t.status, t.updated_at, t.title
        );
    }
}

pub fn cli_status(id: &str) {
    match load(parse_id(id)) {
        Ok(t) => match serde_json::to_string_pretty(&t) {
            Ok(s) => println!("{s}"),
            Err(e) => die(format!("serialize failed: {e}")),
        },
        Err(e) => die(e),
    }
}

pub fn cli_logs(id: &str) {
    match read_log(parse_id(id)) {
        Ok(s) => print!("{s}"),
        Err(e) => die(e),
    }
}

pub fn cli_log(id: &str, message: &str) {
    if let Err(e) = append_log(parse_id(id), message) {
        die(e);
    }
}

pub fn cli_update(id: &str, status: &str) {
    let s = match Status::parse(status) {
        Ok(s) => s,
        Err(e) => die(e),
    };
    if let Err(e) = update_status(parse_id(id), s) {
        die(e);
    }
}

pub fn cli_stop(id: &str) {
    if let Err(e) = stop(parse_id(id)) {
        die(e);
    }
}

pub fn cli_set_pid(id: &str, pid: i32) {
    if let Err(e) = set_pid(parse_id(id), pid) {
        die(e);
    }
}

pub fn cli_result(id: &str, text: &str) {
    if let Err(e) = set_result(parse_id(id), text) {
        die(e);
    }
}
