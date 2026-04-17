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
