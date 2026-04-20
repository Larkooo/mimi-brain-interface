use crate::brain;
use crate::paths;
use std::process::Command;

/// systemd user services that make up a running Mimi deployment. Kept in
/// sync with MANAGED_SERVICES in src/dashboard/mod.rs.
const MANAGED_SERVICES: &[&str] = &["mimi-telegram", "mimi-discord", "mimi-dashboard"];

struct ServiceStatus {
    active_state: String,
    sub_state: String,
    main_pid: Option<u32>,
    enabled: bool,
}

fn systemctl_user(args: &[&str]) -> Option<String> {
    let out = Command::new("systemctl").arg("--user").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn service_status(name: &str) -> ServiceStatus {
    let show = systemctl_user(&["show", name, "--no-page"]).unwrap_or_default();
    let mut s = ServiceStatus {
        active_state: "unknown".into(),
        sub_state: "unknown".into(),
        main_pid: None,
        enabled: false,
    };
    for line in show.lines() {
        if let Some(v) = line.strip_prefix("ActiveState=") {
            s.active_state = v.into();
        } else if let Some(v) = line.strip_prefix("SubState=") {
            s.sub_state = v.into();
        } else if let Some(v) = line.strip_prefix("MainPID=") {
            s.main_pid = v.parse().ok().filter(|p| *p != 0);
        }
    }
    s.enabled = systemctl_user(&["is-enabled", name])
        .map(|o| o.trim() == "enabled")
        .unwrap_or(false);
    s
}

pub fn run() {
    if !paths::brain_db().exists() {
        eprintln!("Mimi is not set up yet. Run `mimi setup` first.");
        std::process::exit(1);
    }

    let config: serde_json::Value = std::fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    let name = config
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Mimi");
    let session = config
        .get("session_name")
        .and_then(|v| v.as_str())
        .unwrap_or("mimi");

    println!("=== {} Status ===\n", name);

    println!("  Services:");
    for svc in MANAGED_SERVICES {
        let s = service_status(svc);
        let state = format!("{} ({})", s.active_state, s.sub_state);
        let enabled = if s.enabled { "enabled" } else { "disabled" };
        let pid = s.main_pid.map(|p| format!(" · pid {p}")).unwrap_or_default();
        println!("    {:<18} {:<22} · {}{}", svc, state, enabled, pid);
    }

    // tmux interactive session (only used by `mimi` with no subcommand).
    let tmux_running = Command::new("tmux")
        .args(["has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let tmux_state = if tmux_running { "running" } else { "not running" };
    println!("    {:<18} {}", format!("tmux: {session}"), tmux_state);
    println!();

    // Claude version
    let version = crate::claude::version();
    if !version.is_empty() {
        println!("  Claude:   {}", version);
    }

    // Brain stats
    let db = brain::open();
    match brain::get_stats(&db) {
        Ok(stats) => {
            println!("  Entities: {}", stats.entities);
            println!("  Links:    {}", stats.relationships);
            println!("  Mem refs: {}", stats.memory_refs);

            if !stats.entity_types.is_empty() {
                println!("\n  Entity types:");
                for (t, c) in &stats.entity_types {
                    println!("    {}: {}", t, c);
                }
            }
        }
        Err(e) => {
            eprintln!("  Brain stats unavailable: {}", e);
        }
    }

    // Memory files
    let mem_count = std::fs::read_dir(paths::memory_dir())
        .map(|d| d.filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                .unwrap_or(false)
        }).count())
        .unwrap_or(0);
    println!("\n  Memory files: {}", mem_count);

    // Data dir size
    let size = dir_size(paths::home());
    println!("  Data size:    {}", format_bytes(size));
}

fn dir_size(path: std::path::PathBuf) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = entry.metadata();
            if let Ok(m) = meta {
                if m.is_dir() {
                    total += dir_size(entry.path());
                } else {
                    total += m.len();
                }
            }
        }
    }
    total
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
