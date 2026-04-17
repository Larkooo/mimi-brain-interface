use crate::paths;
use std::process::Command;

const REFLECT_PROMPT: &str = r#"You are Mimi's prefrontal cortex — a meta-cognitive process that reflects on Mimi's state, memories, and knowledge.

Your job is to review Mimi's current brain state and perform maintenance:

1. **Review recent memories** — read the memory files in ~/.mimi/memory/
2. **Audit the knowledge graph** — query brain.db for:
   - Duplicate entities (same name or very similar names, different IDs) — merge them
   - Orphaned entities (no relationships) — add relationships or note why they're standalone
   - Stale or contradictory information — update or remove
   - Missing relationships that should exist based on memories — add them
3. **Consolidate** — actually execute the merges, link additions, and cleanups via sqlite3 commands
4. **Reorganize memories** — update MEMORY.md index if it's out of date, archive old reflections
5. **Reflect** — write a reflection memory summarizing:
   - What Mimi has learned recently
   - What patterns or gaps you notice in the knowledge graph
   - What Mimi should pay attention to going forward
   - Self-improvement observations
   - A brief "state of mind" — how coherent and organized is the brain right now?

Save your reflection as: ~/.mimi/memory/reflect_YYYY-MM-DD.md
Update ~/.mimi/memory/MEMORY.md to include the new reflection.

Be thorough but efficient. Actually run the cleanup queries, don't just list what should be done.

Start by reading ~/.mimi/memory/MEMORY.md, then query the brain, then do the work."#;

pub fn run() {
    if !paths::brain_db().exists() {
        eprintln!("Mimi is not set up yet. Run `mimi setup` first.");
        std::process::exit(1);
    }

    let config: serde_json::Value = std::fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    let session = config
        .get("session_name")
        .and_then(|v| v.as_str())
        .unwrap_or("mimi");

    // Step 1: Stop the running session to free context
    println!("Stopping active session to clear context...");
    Command::new("tmux")
        .args(["kill-session", "-t", session])
        .output()
        .ok();

    // Step 2: Run the reflection as a one-shot claude --print
    println!("Running self-reflection cycle...\n");
    let mimi_home = paths::home();
    let status = Command::new("claude")
        .args([
            "--print",
            "--dangerously-skip-permissions",
            REFLECT_PROMPT,
        ])
        .current_dir(&mimi_home)
        .status()
        .expect("failed to run claude — is it installed?");

    if status.success() {
        println!("\nReflection complete.");
    } else {
        eprintln!("Reflection failed.");
    }

    // Step 3: Relaunch Mimi with a fresh context
    println!("Relaunching Mimi with fresh context...");
    if let Err(e) = crate::claude::launch_tmux(session) {
        eprintln!("Failed to relaunch: {e}");
        std::process::exit(1);
    }
}
