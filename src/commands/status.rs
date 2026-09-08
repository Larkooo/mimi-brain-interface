use crate::brain;
use crate::paths;
use std::process::Command;

// Kept in sync with `MANAGED_SERVICES` in src/dashboard/mod.rs — the systemd
// user units that actually run mimi in production. The tmux session above is
// just the optional interactive REPL.
const MANAGED_SERVICES: &[&str] = &["mimi-telegram", "mimi-discord", "mimi-dashboard"];

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

    // Systemd user services — the channel bridges + dashboard that actually
    // run in production. The dashboard surfaces these too; `mimi status` did
    // not, so SSH users couldn't tell whether Mimi was online without
    // running `systemctl --user status` per unit.
    let services = service_states();
    if !services.is_empty() {
        println!("  Services:");
        let all_unknown = services.iter().all(|s| s.active_state == "unknown");
        if all_unknown {
            // systemd --user bus unavailable (typical in containers / fresh
            // SSH without lingering enabled). Print one note instead of N
            // copies of "unknown · unknown".
            println!("    (systemd --user unavailable — `loginctl enable-linger` and start units)");
        } else {
            for s in &services {
                let pid = s.main_pid.map(|p| format!(" · pid {p}")).unwrap_or_default();
                println!(
                    "    {:<16} {} · {}{}",
                    s.name, s.active_state, s.sub_state, pid
                );
            }
        }
        println!();
    }

    // Check tmux session — optional interactive REPL launched via `mimi launch`.
    let tmux_running = Command::new("tmux")
        .args(["has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if tmux_running {
        println!("  Session:  RUNNING (tmux: {})", session);
    } else {
        println!("  Session:  not running (optional REPL, start with `mimi launch`)");
    }

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

struct ServiceState {
    name: String,
    active_state: String,
    sub_state: String,
    main_pid: Option<u32>,
}

fn service_states() -> Vec<ServiceState> {
    MANAGED_SERVICES
        .iter()
        .map(|name| {
            let text = Command::new("systemctl")
                .args(["--user", "show", name, "--no-page"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            let mut active_state = String::from("unknown");
            let mut sub_state = String::from("unknown");
            let mut main_pid: Option<u32> = None;
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("ActiveState=") {
                    active_state = v.to_string();
                } else if let Some(v) = line.strip_prefix("SubState=") {
                    sub_state = v.to_string();
                } else if let Some(v) = line.strip_prefix("MainPID=") {
                    main_pid = v.parse().ok().filter(|p| *p != 0);
                }
            }
            ServiceState {
                name: (*name).to_string(),
                active_state,
                sub_state,
                main_pid,
            }
        })
        .collect()
}
