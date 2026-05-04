use crate::paths;
use crate::brain;
use std::fs;

const CLAUDE_MD_TEMPLATE: &str = include_str!("../../CLAUDE.md.template");

pub fn run() {
    println!("Setting up Mimi...");

    // Create directories
    paths::ensure_dirs();
    println!("  Created ~/.mimi/");

    // Initialize brain
    if !paths::brain_db().exists() {
        brain::init();
        println!("  Initialized brain.db");
    } else {
        println!("  brain.db already exists");
    }

    // Create memory index
    if !paths::memory_index().exists() {
        fs::write(paths::memory_index(), "# Memory Index\n").ok();
        println!("  Created memory/MEMORY.md");
    }

    // Copy CLAUDE.md template
    if !paths::claude_md().exists() {
        fs::write(paths::claude_md(), CLAUDE_MD_TEMPLATE).ok();
        println!("  Created CLAUDE.md");
    } else {
        println!("  CLAUDE.md already exists (not overwritten)");
    }

    // Create default config
    if !paths::config_file().exists() {
        let default_config = serde_json::json!({
            "name": "Mimi",
            "model": "sonnet",
            "session_name": "mimi",
            "dashboard_port": 3131,
            "owner_tz": "America/Chicago",
        });
        fs::write(
            paths::config_file(),
            serde_json::to_string_pretty(&default_config).unwrap(),
        )
        .ok();
        println!("  Created config.json");
    }

    println!("\nSetup complete. Run `mimi` to launch.");
}
