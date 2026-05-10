use crate::brain;
use crate::paths;
use std::process::Command;

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

    // Check tmux session
    let tmux_running = Command::new("tmux")
        .args(["has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if tmux_running {
        println!("  Session:  RUNNING (tmux: {})", session);
    } else {
        println!("  Session:  NOT RUNNING");
    }

    // Claude version
    let version = crate::claude::version();
    if !version.is_empty() {
        println!("  Claude:   {}", version);
    }

    // Systemd user-services that run a deployed Mimi. Mirrors the unit list
    // that `mimi update` restarts after a deploy. We probe via `systemctl
    // --user is-active` and surface three buckets: active (the bot is up),
    // inactive (unit exists but is stopped — could be intentional or a
    // crash), or missing (no such unit on this host — fine for hosts that
    // don't run that channel).
    println!("\n  Services:");
    let mut any_unit = false;
    for unit in ["mimi-dashboard", "mimi-discord", "mimi-telegram", "mimi-presence"] {
        match service_state(unit) {
            ServiceState::Active => {
                any_unit = true;
                println!("    {:18} active", unit);
            }
            ServiceState::Inactive(state) => {
                any_unit = true;
                println!("    {:18} {} (run: systemctl --user start {})", unit, state, unit);
            }
            ServiceState::Missing => {}
        }
    }
    if !any_unit {
        println!("    (no mimi-* user services installed)");
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

/// Active = unit running. Inactive(state) = unit exists but `is-active`
/// returned a non-success state (e.g. "inactive", "failed", "activating").
/// Missing = `is-active` couldn't find the unit at all (different exit on
/// `LoadState=not-found`), so we hide it from output rather than nag the
/// user about a service they never installed.
enum ServiceState {
    Active,
    Inactive(String),
    Missing,
}

fn service_state(unit: &str) -> ServiceState {
    let out = match Command::new("systemctl")
        .args(["--user", "is-active", unit])
        .output()
    {
        Ok(o) => o,
        // systemctl unavailable (non-systemd host, no DBus): treat as missing
        // so we don't print a confusing error for every unit.
        Err(_) => return ServiceState::Missing,
    };
    let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && state == "active" {
        return ServiceState::Active;
    }
    // `is-active` prints "inactive" with exit 3 for stopped units AND for
    // unknown units. Disambiguate via `show -p LoadState`: "not-found" means
    // the unit isn't installed; anything else means it exists but isn't up.
    let load = Command::new("systemctl")
        .args(["--user", "show", "-p", "LoadState", "--value", unit])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    // Empty load means systemctl couldn't determine LoadState (unreachable
    // bus, no user manager). "not-found" is the explicit "no such unit"
    // signal. Either way, hide the unit rather than mislabel it inactive.
    if load.is_empty() || load == "not-found" {
        ServiceState::Missing
    } else {
        ServiceState::Inactive(if state.is_empty() { "inactive".into() } else { state })
    }
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
