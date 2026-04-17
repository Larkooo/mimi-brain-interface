//! Background task registry.
//!
//! The channel agent (or the CLI) records long-running workflows here so the
//! user can see status without blocking the main conversation. A task is a
//! pair of files under `~/.mimi/tasks/`:
//!
//!   <id>.json   — metadata (title, status, timestamps, spawner, optional pid)
//!   <id>.log    — append-only plain-text progress log
//!
//! This registry does NOT spawn the work itself. The expectation is that the
//! channel agent uses Claude Code's `Task` tool with `run_in_background: true`
//! (or shells out to a detached process) and writes progress entries here so
//! status is queryable from any surface (other channel, CLI, dashboard).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::paths;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl Status {
    fn parse(s: &str) -> Result<Status, String> {
        match s.to_ascii_lowercase().as_str() {
            "pending" => Ok(Status::Pending),
            "running" => Ok(Status::Running),
            "done" => Ok(Status::Done),
            "failed" => Ok(Status::Failed),
            "cancelled" | "canceled" => Ok(Status::Cancelled),
            other => Err(format!("unknown status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub spawner: String,
    #[serde(default)]
    pub pid: Option<i32>,
    #[serde(default)]
    pub result: Option<String>,
}

fn meta_path(id: &str) -> PathBuf {
    paths::tasks_dir().join(format!("{id}.json"))
}

fn log_path(id: &str) -> PathBuf {
    paths::tasks_dir().join(format!("{id}.log"))
}

fn short_id() -> String {
    // 8 hex chars is unique enough for a user-visible task id in practice and
    // fits comfortably in a Discord message.
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

pub fn new(title: &str, spawner: &str) -> Result<Task, String> {
    paths::ensure_dirs();
    let id = short_id();
    let now = Utc::now();
    let task = Task {
        id: id.clone(),
        title: title.into(),
        status: Status::Pending,
        created_at: now,
        updated_at: now,
        spawner: spawner.into(),
        pid: None,
        result: None,
    };
    save(&task)?;
    append_log(&id, "created")?;
    Ok(task)
}

// NOTE: save() is a read-modify-write replace without locking. Safe under the
// intended use (single spawner owns a task), racy if two writers hit the same
// id — last write wins. If we ever spawn concurrent updaters per task, swap
// to write-to-tmp + rename and a file lock.
pub fn save(task: &Task) -> Result<(), String> {
    paths::ensure_dirs();
    let json = serde_json::to_string_pretty(task).map_err(|e| e.to_string())?;
    fs::write(meta_path(&task.id), json).map_err(|e| e.to_string())
}

pub fn load(id: &str) -> Result<Task, String> {
    let raw = fs::read_to_string(meta_path(id))
        .map_err(|e| format!("task {id} not found: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("corrupt metadata for {id}: {e}"))
}

pub fn update_status(id: &str, status: Status) -> Result<Task, String> {
    let mut task = load(id)?;
    task.status = status;
    task.updated_at = Utc::now();
    save(&task)?;
    append_log(id, &format!("status → {:?}", status).to_lowercase())?;
    Ok(task)
}

pub fn set_pid(id: &str, pid: i32) -> Result<(), String> {
    let mut task = load(id)?;
    task.pid = Some(pid);
    task.updated_at = Utc::now();
    save(&task)
}

pub fn set_result(id: &str, result: &str) -> Result<(), String> {
    let mut task = load(id)?;
    task.result = Some(result.into());
    task.updated_at = Utc::now();
    save(&task)
}

pub fn append_log(id: &str, message: &str) -> Result<(), String> {
    paths::ensure_dirs();
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(id))
        .map_err(|e| e.to_string())?;
    let ts = Utc::now().to_rfc3339();
    writeln!(f, "[{ts}] {message}").map_err(|e| e.to_string())
}

pub fn read_log(id: &str) -> Result<String, String> {
    fs::read_to_string(log_path(id)).map_err(|e| format!("no log for {id}: {e}"))
}

pub fn list() -> Vec<Task> {
    let dir = paths::tasks_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut tasks: Vec<Task> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "json"))
        .filter_map(|e| {
            let raw = fs::read_to_string(e.path()).ok()?;
            serde_json::from_str::<Task>(&raw).ok()
        })
        .collect();
    tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    tasks
}

pub fn stop(id: &str) -> Result<(), String> {
    let task = load(id)?;
    if let Some(pid) = task.pid {
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

pub fn cli_new(title: &str, spawner: &str) {
    match new(title, spawner) {
        Ok(task) => println!("{}", task.id),
        Err(e) => {
            eprintln!("task new failed: {e}");
            std::process::exit(1);
        }
    }
}

pub fn cli_list() {
    let tasks = list();
    if tasks.is_empty() {
        println!("(no tasks)");
        return;
    }
    println!("{:<10} {:<10} {:<20} {}", "ID", "STATUS", "UPDATED", "TITLE");
    for t in tasks {
        let status = format!("{:?}", t.status).to_lowercase();
        println!(
            "{:<10} {:<10} {:<20} {}",
            t.id,
            status,
            t.updated_at.format("%Y-%m-%d %H:%M:%S"),
            t.title
        );
    }
}

pub fn cli_status(id: &str) {
    match load(id) {
        Ok(t) => match serde_json::to_string_pretty(&t) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("serialize failed: {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

pub fn cli_logs(id: &str) {
    match read_log(id) {
        Ok(s) => print!("{s}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

pub fn cli_log(id: &str, message: &str) {
    if let Err(e) = append_log(id, message) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

pub fn cli_update(id: &str, status: &str) {
    let s = match Status::parse(status) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = update_status(id, s) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

pub fn cli_stop(id: &str) {
    if let Err(e) = stop(id) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

pub fn cli_set_pid(id: &str, pid: i32) {
    if let Err(e) = set_pid(id, pid) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

pub fn cli_result(id: &str, text: &str) {
    if let Err(e) = set_result(id, text) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
