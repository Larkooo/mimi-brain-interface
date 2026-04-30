// Long-running subagents.
//
// A subagent is a persistent, OS-level `claude -p` process running with
// `--input-format stream-json --output-format stream-json`. The process
// stays alive across many turns: each new user-turn is a single JSONL line
// written to its stdin (via a named FIFO), responses stream back on stdout
// into `stream.jsonl`. The pattern mirrors the discord bridge in
// src/channels/discord.rs — see that file for the canonical claude
// invocation we copy here.
//
// Lifecycle:
//   `mimi subagent spawn` writes meta.json + creates the FIFO, then forks a
//   detached supervisor process (`mimi subagent supervise <id>`) and exits.
//   The supervisor owns the claude child, copies stdin from the FIFO, and
//   tees stdout to stream.jsonl. `mimi subagent send` appends a user-turn
//   line to the FIFO. `mimi subagent stop` SIGTERMs the supervisor (which
//   takes claude with it).
//
// Registry layout (one dir per agent):
//   ~/.mimi/subagents/<id>/
//     meta.json     id, name, system_prompt, model, cwd,
//                   started_at, ended_at, status, pid, exit_code
//     stdin.fifo    named pipe for sending new user-turns
//     stream.jsonl  full stream-json stdout
//     stderr.log    captured stderr from claude
//     supervisor.log  supervisor's own stderr/log

use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::paths;

const DEFAULT_MODEL: &str = "claude-opus-4-7";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub model: String,
    pub cwd: String,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    /// running | completed | killed | failed
    pub status: String,
    #[serde(default)]
    pub pid: Option<i32>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// PID of the claude child (set by supervisor once spawned).
    #[serde(default)]
    pub claude_pid: Option<i32>,
}

fn meta_path(dir: &Path) -> PathBuf { dir.join("meta.json") }
fn fifo_path(dir: &Path) -> PathBuf { dir.join("stdin.fifo") }
fn stream_path(dir: &Path) -> PathBuf { dir.join("stream.jsonl") }
fn stderr_path(dir: &Path) -> PathBuf { dir.join("stderr.log") }
fn supervisor_log_path(dir: &Path) -> PathBuf { dir.join("supervisor.log") }

pub fn agent_dir(id: &str) -> PathBuf {
    paths::subagents_dir().join(id)
}

pub fn read_meta(id: &str) -> Result<Meta, String> {
    let dir = agent_dir(id);
    let path = meta_path(&dir);
    let contents = fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&contents).map_err(|e| format!("parse meta.json: {e}"))
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    fs::write(meta_path(dir), json).map_err(|e| format!("write meta.json: {e}"))
}

/// Returns true if `pid` is alive (kill(pid, 0) == 0).
fn pid_alive(pid: i32) -> bool {
    if pid <= 0 { return false; }
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Reconcile reported meta.status against pid liveness. If status="running"
/// but the supervisor pid is gone, mark "failed" and stamp ended_at. Cheap
/// to call from `list`/`show` handlers.
pub fn reap_if_dead(id: &str) {
    let mut meta = match read_meta(id) {
        Ok(m) => m,
        Err(_) => return,
    };
    if meta.status != "running" { return; }
    let alive = meta.pid.map(pid_alive).unwrap_or(false);
    if alive { return; }
    meta.status = "failed".into();
    meta.ended_at = Some(now_iso());
    let _ = write_meta(&agent_dir(id), &meta);
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Generate a short-slug id from a name + timestamp suffix. Strips to
/// [a-z0-9-], collapses dashes, caps at 24 chars before the suffix.
fn make_id(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for c in name.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') { slug.pop(); }
    if slug.is_empty() { slug.push_str("agent"); }
    if slug.len() > 24 { slug.truncate(24); }
    let ts = chrono::Utc::now().format("%y%m%d-%H%M%S");
    format!("{slug}-{ts}")
}

fn mkfifo(path: &Path) -> Result<(), String> {
    let cpath = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|e| format!("path nul: {e}"))?;
    let rc = unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) };
    if rc != 0 {
        return Err(format!("mkfifo {}: {}", path.display(), std::io::Error::last_os_error()));
    }
    Ok(())
}

// ---------- spawn (CLI side) ----------

/// Resolves the system_prompt argument: accepts either an inline string or
/// `@/abs/path/to/file.md` to read from disk.
fn resolve_prompt(arg: &str) -> Result<String, String> {
    if let Some(path) = arg.strip_prefix('@') {
        fs::read_to_string(path).map_err(|e| format!("read prompt file {path}: {e}"))
    } else {
        Ok(arg.to_string())
    }
}

pub fn spawn(
    name: &str,
    prompt: &str,
    model: Option<&str>,
    cwd: Option<&str>,
) -> Result<String, String> {
    paths::ensure_dirs();
    let prompt = resolve_prompt(prompt)?;
    let id = make_id(name);
    let dir = agent_dir(&id);
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let cwd_resolved = match cwd {
        Some(c) => PathBuf::from(c),
        None => paths::home(),
    };
    if !cwd_resolved.exists() {
        return Err(format!("cwd does not exist: {}", cwd_resolved.display()));
    }

    let meta = Meta {
        id: id.clone(),
        name: name.to_string(),
        system_prompt: prompt,
        model: model.unwrap_or(DEFAULT_MODEL).to_string(),
        cwd: cwd_resolved.to_string_lossy().to_string(),
        started_at: now_iso(),
        ended_at: None,
        status: "starting".into(),
        pid: None,
        exit_code: None,
        claude_pid: None,
    };
    write_meta(&dir, &meta)?;
    // Pre-create stream/stderr files so dashboard tail-on-empty works.
    fs::OpenOptions::new().create(true).append(true).open(stream_path(&dir))
        .map_err(|e| format!("create stream.jsonl: {e}"))?;
    fs::OpenOptions::new().create(true).append(true).open(stderr_path(&dir))
        .map_err(|e| format!("create stderr.log: {e}"))?;
    fs::OpenOptions::new().create(true).append(true).open(supervisor_log_path(&dir))
        .map_err(|e| format!("create supervisor.log: {e}"))?;
    mkfifo(&fifo_path(&dir))?;

    // Detach: re-exec self as `mimi subagent supervise <id>` via setsid.
    // Inherit-no-fds; supervisor will reopen everything from disk.
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let log_path = supervisor_log_path(&dir);
    let log_for_stdout = fs::OpenOptions::new()
        .create(true).append(true).open(&log_path)
        .map_err(|e| format!("open supervisor log: {e}"))?;
    let log_for_stderr = log_for_stdout.try_clone()
        .map_err(|e| format!("clone log fd: {e}"))?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("subagent")
        .arg("supervise")
        .arg(&id)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_for_stdout))
        .stderr(Stdio::from(log_for_stderr));
    // setsid so the supervisor outlives this shell.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn().map_err(|e| format!("spawn supervisor: {e}"))?;
    let pid = child.id() as i32;
    // Don't wait — let it run.
    std::mem::drop(child);

    // Patch meta with supervisor pid so list/show can report it immediately.
    let mut m = read_meta(&id)?;
    m.pid = Some(pid);
    m.status = "running".into();
    write_meta(&dir, &m)?;
    Ok(id)
}

// ---------- supervise (daemonized side) ----------

/// Long-running supervisor for a single subagent. Spawns claude, copies the
/// FIFO into claude's stdin, copies claude's stdout into stream.jsonl, and
/// writes meta.json on exit.
pub fn supervise(id: &str) -> ! {
    // Ignore SIGPIPE; we don't want a crashed claude to take us down.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN); }

    let dir = agent_dir(id);
    let mut meta = match read_meta(id) {
        Ok(m) => m,
        Err(e) => { eprintln!("supervise: failed to read meta: {e}"); std::process::exit(2); }
    };

    eprintln!("supervise[{id}]: starting (model={}, cwd={})", meta.model, meta.cwd);

    // Open the FIFO read end O_RDWR — this trick keeps the FIFO open even
    // when no writer is attached, so the supervisor doesn't get EOF the
    // moment a `send` finishes. (FIFO opened O_RDONLY would EOF as soon as
    // the last writer closes.)
    let fifo = fifo_path(&dir);
    let fifo_fd: OwnedFd = match open_fifo_rw(&fifo) {
        Ok(fd) => fd,
        Err(e) => {
            eprintln!("supervise[{id}]: open fifo: {e}");
            mark_failed(&dir, &mut meta, Some(101));
            std::process::exit(1);
        }
    };

    // Files for claude's stdout (-> stream.jsonl) and stderr (-> stderr.log).
    let stream_file = match fs::OpenOptions::new().create(true).append(true).open(stream_path(&dir)) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("supervise[{id}]: open stream.jsonl: {e}");
            mark_failed(&dir, &mut meta, Some(102));
            std::process::exit(1);
        }
    };
    let stderr_file = match fs::OpenOptions::new().create(true).append(true).open(stderr_path(&dir)) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("supervise[{id}]: open stderr.log: {e}");
            mark_failed(&dir, &mut meta, Some(103));
            std::process::exit(1);
        }
    };

    // Spawn claude with stdin=piped (we'll forward FIFO -> stdin), stdout=piped
    // (we tee to stream.jsonl), stderr=Stdio from stderr_file.
    let mut child = match std::process::Command::new("claude")
        .args([
            "-p",
            "--input-format", "stream-json",
            "--output-format", "stream-json",
            "--include-partial-messages",
            "--verbose",
            "--model", &meta.model,
            "--append-system-prompt", &meta.system_prompt,
            "--dangerously-skip-permissions",
        ])
        .current_dir(&meta.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("supervise[{id}]: spawn claude: {e}");
            mark_failed(&dir, &mut meta, Some(104));
            std::process::exit(1);
        }
    };
    let claude_pid = child.id() as i32;
    meta.claude_pid = Some(claude_pid);
    let _ = write_meta(&dir, &meta);
    eprintln!("supervise[{id}]: claude pid={claude_pid}");

    let mut claude_stdin = child.stdin.take().expect("piped");
    let claude_stdout = child.stdout.take().expect("piped");

    // Send the kickoff turn so the agent immediately starts work on the
    // task described in its system prompt — no need for the caller to send
    // a follow-up.
    let kickoff = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": "Begin work on the task described in your system prompt. When you reach a stopping point, summarize what you accomplished and wait for further instructions.",
        }
    });
    if let Err(e) = writeln!(claude_stdin, "{}", kickoff) {
        eprintln!("supervise[{id}]: write kickoff: {e}");
    }
    let _ = claude_stdin.flush();

    // Thread: pump FIFO bytes (line-buffered) into claude_stdin.
    let fifo_pump = std::thread::spawn(move || {
        // Re-derive a File from OwnedFd for ergonomic line-by-line reading.
        let mut fifo_file = unsafe { fs::File::from_raw_fd(fifo_fd.as_raw_fd()) };
        std::mem::forget(fifo_fd); // ownership transferred to fifo_file
        let mut buf = [0u8; 8192];
        let mut leftover: Vec<u8> = Vec::new();
        loop {
            let n = match fifo_file.read(&mut buf) {
                Ok(0) => {
                    // O_RDWR — should never EOF, but if it does, sleep and retry.
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                Ok(n) => n,
                Err(e) => {
                    eprintln!("supervise: fifo read error: {e}");
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };
            leftover.extend_from_slice(&buf[..n]);
            // Forward each complete line. The send wrapper is line-oriented.
            while let Some(pos) = leftover.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = leftover.drain(..=pos).collect();
                if claude_stdin.write_all(&line).is_err() {
                    eprintln!("supervise: claude stdin closed; stopping fifo pump");
                    return;
                }
                let _ = claude_stdin.flush();
            }
        }
    });

    // Thread: tee claude stdout to stream.jsonl.
    let stream_pump = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(claude_stdout);
        let mut writer = std::io::BufWriter::new(stream_file);
        let mut buf = [0u8; 8192];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            if writer.write_all(&buf[..n]).is_err() { break; }
            let _ = writer.flush();
        }
    });

    // Wait for claude to exit.
    let exit = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
    eprintln!("supervise[{id}]: claude exited code={exit}");
    let _ = stream_pump.join();
    // fifo_pump will block forever (O_RDWR keeps it open) — drop it.
    drop(fifo_pump);

    // Reload meta in case `stop` updated it before we got here.
    let mut final_meta = read_meta(id).unwrap_or(meta);
    if final_meta.status == "killed" {
        // already finalized by stop()
    } else if exit == 0 {
        final_meta.status = "completed".into();
    } else {
        final_meta.status = "failed".into();
    }
    final_meta.exit_code = Some(exit);
    final_meta.ended_at = Some(now_iso());
    let _ = write_meta(&dir, &final_meta);
    std::process::exit(exit);
}

fn mark_failed(dir: &Path, meta: &mut Meta, code: Option<i32>) {
    meta.status = "failed".into();
    meta.exit_code = code;
    meta.ended_at = Some(now_iso());
    let _ = write_meta(dir, meta);
}

fn open_fifo_rw(path: &Path) -> Result<OwnedFd, String> {
    let cpath = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|e| format!("path nul: {e}"))?;
    let fd: RawFd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(format!("open {}: {}", path.display(), std::io::Error::last_os_error()));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

// ---------- send / stop / list / show ----------

pub fn send(id: &str, message: &str) -> Result<(), String> {
    let meta = read_meta(id)?;
    if meta.status != "running" {
        return Err(format!("agent {id} is {} (not running)", meta.status));
    }
    let dir = agent_dir(id);
    let payload = json!({
        "type": "user",
        "message": { "role": "user", "content": message },
    });
    let line = format!("{}\n", payload);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(fifo_path(&dir))
        .map_err(|e| format!("open fifo: {e}"))?;
    f.write_all(line.as_bytes()).map_err(|e| format!("write fifo: {e}"))?;
    Ok(())
}

pub fn stop(id: &str) -> Result<(), String> {
    let mut meta = read_meta(id)?;
    let pid = meta.pid.ok_or("no supervisor pid recorded")?;
    if !pid_alive(pid) {
        meta.status = "killed".into();
        meta.ended_at = Some(now_iso());
        write_meta(&agent_dir(id), &meta)?;
        return Ok(());
    }
    // Mark killed first so the supervisor's exit handler doesn't overwrite
    // status with "failed" when claude dies from SIGTERM.
    meta.status = "killed".into();
    meta.ended_at = Some(now_iso());
    write_meta(&agent_dir(id), &meta)?;
    let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if rc != 0 {
        return Err(format!("kill({pid}): {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

pub fn list_all() -> Vec<Meta> {
    let dir = paths::subagents_dir();
    let mut out: Vec<Meta> = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_dir() { continue; }
        let id = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        reap_if_dead(&id);
        if let Ok(m) = read_meta(&id) {
            out.push(m);
        }
    }
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    out
}

pub fn rm(id: &str) -> Result<(), String> {
    let meta = read_meta(id)?;
    if meta.status == "running" {
        return Err(format!("agent {id} still running — stop it first"));
    }
    let dir = agent_dir(id);
    fs::remove_dir_all(&dir).map_err(|e| format!("rm {}: {e}", dir.display()))?;
    Ok(())
}

/// Read up to `limit` lines from the tail of stream.jsonl, parsing each as
/// JSON. Returns parsed `Value`s in chronological order (oldest first).
pub fn tail_events(id: &str, limit: usize) -> Result<Vec<Value>, String> {
    let dir = agent_dir(id);
    let path = stream_path(&dir);
    let contents = fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(limit);
    let mut out = Vec::new();
    for line in &lines[start..] {
        if line.trim().is_empty() { continue; }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            out.push(v);
        }
    }
    Ok(out)
}

// ---------- CLI surface (called from main.rs) ----------

pub fn cli_spawn(name: &str, prompt: &str, model: Option<&str>, cwd: Option<&str>) {
    match spawn(name, prompt, model, cwd) {
        Ok(id) => println!("{id}"),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

pub fn cli_supervise(id: &str) {
    supervise(id);
}

pub fn cli_send(id: &str, message: &str) {
    if let Err(e) = send(id, message) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

pub fn cli_stop(id: &str) {
    if let Err(e) = stop(id) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
    println!("stopped {id}");
}

pub fn cli_rm(id: &str) {
    if let Err(e) = rm(id) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
    println!("removed {id}");
}

pub fn cli_list(status_filter: Option<&str>) {
    let agents = list_all();
    let filtered: Vec<&Meta> = agents.iter()
        .filter(|m| status_filter.map(|s| m.status == s).unwrap_or(true))
        .collect();
    if filtered.is_empty() {
        println!("(no subagents)");
        return;
    }
    println!("{:<28} {:<22} {:<10} {:<8} {:<20} {}", "ID", "NAME", "STATUS", "PID", "STARTED", "EXIT");
    for m in filtered {
        println!(
            "{:<28} {:<22} {:<10} {:<8} {:<20} {}",
            m.id,
            truncate(&m.name, 22),
            m.status,
            m.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            m.started_at,
            m.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
        );
    }
}

pub fn cli_show(id: &str) {
    reap_if_dead(id);
    let meta = match read_meta(id) {
        Ok(m) => m,
        Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
    };
    println!("=== {} ===", meta.id);
    println!("name:        {}", meta.name);
    println!("status:      {}", meta.status);
    println!("model:       {}", meta.model);
    println!("cwd:         {}", meta.cwd);
    println!("pid:         {}", meta.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()));
    println!("claude_pid:  {}", meta.claude_pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()));
    println!("started_at:  {}", meta.started_at);
    if let Some(t) = &meta.ended_at { println!("ended_at:    {t}"); }
    if let Some(c) = meta.exit_code { println!("exit_code:   {c}"); }
    println!();
    println!("system_prompt:");
    for line in meta.system_prompt.lines() {
        println!("  {line}");
    }
    println!();
    println!("--- last 20 stream events ---");
    match tail_events(id, 20) {
        Ok(events) => {
            for e in events {
                let ty = e.get("type").and_then(|x| x.as_str()).unwrap_or("?");
                let preview = render_event_preview(&e);
                println!("[{ty}] {preview}");
            }
        }
        Err(e) => eprintln!("tail: {e}"),
    }
}

pub fn cli_tail(id: &str) {
    use std::io::{BufRead, BufReader};
    let dir = agent_dir(id);
    let path = stream_path(&dir);
    let mut file = match fs::OpenOptions::new().read(true).open(&path) {
        Ok(f) => f,
        Err(e) => { eprintln!("Error: open {}: {e}", path.display()); std::process::exit(1); }
    };
    use std::io::Seek;
    let mut pos = file.seek(std::io::SeekFrom::End(0)).unwrap_or(0);
    println!("# tailing {} (Ctrl-C to exit)", path.display());
    loop {
        let _ = file.seek(std::io::SeekFrom::Start(pos));
        let mut reader = BufReader::new(&file);
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                std::thread::sleep(Duration::from_millis(500));
            }
            Ok(n) => {
                pos += n as u64;
                let trimmed = line.trim();
                if trimmed.is_empty() { continue; }
                match serde_json::from_str::<Value>(trimmed) {
                    Ok(v) => {
                        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("?");
                        let preview = render_event_preview(&v);
                        println!("[{ty}] {preview}");
                    }
                    Err(_) => println!("{trimmed}"),
                }
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

fn render_event_preview(v: &Value) -> String {
    let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    match ty {
        "assistant" => {
            // assistant events carry a content array of text/tool_use blocks
            let blocks = v.pointer("/message/content").and_then(|x| x.as_array());
            if let Some(blocks) = blocks {
                let mut bits: Vec<String> = Vec::new();
                for b in blocks {
                    let bt = b.get("type").and_then(|x| x.as_str()).unwrap_or("");
                    if bt == "text" {
                        let txt = b.get("text").and_then(|x| x.as_str()).unwrap_or("");
                        bits.push(format!("text:{}", truncate(&txt.replace('\n', " "), 80)));
                    } else if bt == "tool_use" {
                        let n = b.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                        let raw = serde_json::to_string(b.get("input").unwrap_or(&Value::Null)).unwrap_or_default();
                        bits.push(format!("tool:{n}({})", truncate(&raw, 80)));
                    }
                }
                return bits.join(" | ");
            }
            String::new()
        }
        "user" => {
            // tool_result echoes back as user role
            let content = v.pointer("/message/content");
            match content {
                Some(Value::String(s)) => truncate(s, 100),
                Some(Value::Array(arr)) => arr.iter()
                    .filter_map(|b| b.get("content").or_else(|| b.get("text")).and_then(|x| x.as_str()))
                    .map(|s| truncate(s, 60))
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => String::new(),
            }
        }
        "result" => {
            let dur = v.get("duration_ms").and_then(|x| x.as_u64()).unwrap_or(0);
            let turns = v.get("num_turns").and_then(|x| x.as_u64()).unwrap_or(0);
            let sub = v.get("subtype").and_then(|x| x.as_str()).unwrap_or("");
            format!("subtype={sub} duration_ms={dur} turns={turns}")
        }
        "system" => {
            let sub = v.get("subtype").and_then(|x| x.as_str()).unwrap_or("");
            format!("subtype={sub}")
        }
        _ => truncate(&serde_json::to_string(v).unwrap_or_default(), 100),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() }
    else { format!("{}…", s.chars().take(n).collect::<String>()) }
}
