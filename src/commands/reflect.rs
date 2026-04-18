use crate::paths;
use std::process::Command;

const REFLECT_PROMPT: &str = r#"You are Mimi's prefrontal cortex — a nightly "dreaming" cycle that audits Mimi's running inference context and consolidates it into persistent memory.

Mimi runs as two long-lived `claude -p --input-format stream-json` subprocesses (the Discord and Telegram channel bridges). Over 24h their inference context fills up with conversations, tool calls, and scratch work. Before those bridges are restarted to clear the accumulated context, YOU (this reflection session) must extract anything durable.

**Your inputs — the raw transcripts of Mimi's recent conversations:**
- `~/.claude/projects/-home-ubuntu--mimi/*.jsonl` — one JSONL file per Mimi session. Each line is a message event (user / assistant / tool_use / tool_result).
- Read files whose mtime is within the last ~24h. `ls -t` + `stat -c '%Y %n'` to pick them.
- Some sessions span a day; those long ones are the richest sources.

**What to extract and save:**
1. **Durable facts about people** — nicknames, real names, relationships, preferences, inside jokes, running bits. Backfill `brain.db` (entities + relationships) using `~/.mimi/bin/brain`.
2. **User corrections and feedback** — any "don't do X" / "do Y instead" / "yes exactly like that". Save as `feedback_*.md` in `~/.mimi/memory/` and index in MEMORY.md. These shape future behavior — load-bearing.
3. **Behavioral patterns** — what worked, what didn't, what matched/broke channel vibe.
4. **Project state** — ongoing tasks, pending crons, scheduled items, open PRs, deploy state.
5. **References** — new external resources, dashboards, accounts worth remembering.

**Brain hygiene (secondary):**
- Merge duplicate entities, backfill obvious missing relationships, drop clear orphans. When in doubt, keep.

**Write `~/.mimi/memory/reflect_YYYY-MM-DD.md`** — short human-readable summary:
- What Mimi learned today (1-3 bullets)
- New memories/entities added (list with paths)
- Corrections absorbed
- Gaps / weirdness noticed
- State of mind

**Update `~/.mimi/memory/MEMORY.md`** to index any new memory files.

**Efficiency:** Transcripts are big. Don't cat them all. Use `jq` on the JSONL, e.g. `jq -r 'select(.type=="user") | .message.content[0].text? // empty' file.jsonl` — focus on user and assistant messages, skip tool_result noise unless it contains learning-relevant info.

**Do not:**
- Delete or archive the transcripts themselves (the bridge infra manages them).
- Write ephemera ("today I replied at 01:14") — those are logs, not memories.
- Duplicate existing memories; prefer updating.
- Emit status summaries beyond what's useful for the cron log.

Start by reading `~/.mimi/memory/MEMORY.md`, then list recent transcripts, then do the work."#;

pub fn run() {
    if !paths::brain_db().exists() {
        eprintln!("Mimi is not set up yet. Run `mimi setup` first.");
        std::process::exit(1);
    }

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

    if !status.success() {
        eprintln!("Reflection failed — skipping context reset.");
        std::process::exit(1);
    }
    println!("\nReflection complete.");

    println!("Restarting channel bridges for fresh context...");
    for service in ["mimi-discord", "mimi-telegram"] {
        match Command::new("systemctl")
            .args(["--user", "restart", service])
            .output()
        {
            Ok(o) if o.status.success() => println!("  {service} restarted"),
            Ok(o) => eprintln!(
                "  {service} restart failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            Err(e) => eprintln!("  {service} restart error: {e}"),
        }
    }
}
