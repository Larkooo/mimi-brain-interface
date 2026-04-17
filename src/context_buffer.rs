//! Cross-channel short-term context buffer.
//!
//! Every channel bridge appends user turns and assistant responses to a single
//! rolling JSONL file. When a new turn arrives, the bridge prepends a
//! `<recent_context>` block listing entries from *other* channels (or very
//! recent same-channel turns) so the assistant has a coherent view of what the
//! user just said elsewhere.
//!
//! The file is capped by line count; on write we truncate from the head if the
//! cap is exceeded. Access is serialized via a blocking mutex — throughput
//! here is tiny (a handful of lines per minute) so lock contention is a
//! non-issue.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::paths;

const MAX_LINES: usize = 200;
const CONTEXT_LOOKBACK: usize = 20;
const SAME_CHANNEL_WINDOW_SECS: i64 = 60;

static FILE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub ts: DateTime<Utc>,
    pub source: String,
    pub chat_id: String,
    #[serde(default)]
    pub user_name: String,
    pub kind: Kind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    User,
    Assistant,
}

pub fn append_user(source: &str, chat_id: &str, user_name: &str, text: &str) {
    append(Entry {
        ts: Utc::now(),
        source: source.into(),
        chat_id: chat_id.into(),
        user_name: user_name.into(),
        kind: Kind::User,
        text: text.into(),
    });
}

pub fn append_assistant(source: &str, chat_id: &str, text: &str) {
    append(Entry {
        ts: Utc::now(),
        source: source.into(),
        chat_id: chat_id.into(),
        user_name: String::new(),
        kind: Kind::Assistant,
        text: text.into(),
    });
}

fn append(entry: Entry) {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    paths::ensure_dirs();
    let path = paths::recent_context_file();

    let line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("context_buffer: serialize failed: {e}");
            return;
        }
    };

    // Read current lines to enforce cap; drop oldest if over.
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<&str> = existing.lines().collect();
    lines.push(&line);
    if lines.len() > MAX_LINES {
        let drop = lines.len() - MAX_LINES;
        lines.drain(..drop);
    }
    let mut out = lines.join("\n");
    out.push('\n');

    if let Err(e) = fs::write(&path, out) {
        eprintln!("context_buffer: write failed: {e}");
    }
}

pub fn recent() -> Vec<Entry> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = paths::recent_context_file();
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .collect()
}

/// Render a `<recent_context>` preamble for a turn arriving on
/// `(current_source, current_chat_id)`. Returns `None` if nothing relevant is
/// available — so the caller can skip prepending the block entirely.
pub fn preamble_for(current_source: &str, current_chat_id: &str) -> Option<String> {
    let entries = recent();
    let now = Utc::now();

    let mut picks: Vec<&Entry> = entries
        .iter()
        .rev()
        .filter(|e| {
            let same_channel = e.source == current_source && e.chat_id == current_chat_id;
            if same_channel {
                // Skip same-channel history unless it's very recent — the
                // claude subprocess already has its own conversation memory
                // for its own channel; we only want to surface cross-channel
                // or brand-new context here.
                (now - e.ts).num_seconds() < SAME_CHANNEL_WINDOW_SECS
            } else {
                true
            }
        })
        .take(CONTEXT_LOOKBACK)
        .collect();
    picks.reverse();

    if picks.is_empty() {
        return None;
    }

    let mut out = String::from("<recent_context>\n");
    for e in picks {
        let age = format_age(now - e.ts);
        let who = match e.kind {
            Kind::User => {
                if e.user_name.is_empty() {
                    format!("user@{}", e.source)
                } else {
                    format!("{}@{}", e.user_name, e.source)
                }
            }
            Kind::Assistant => format!("assistant@{}", e.source),
        };
        let text = truncate(&e.text, 400);
        out.push_str(&format!("[{age} · {who}] {text}\n"));
    }
    out.push_str("</recent_context>\n");
    Some(out)
}

fn format_age(d: chrono::Duration) -> String {
    let s = d.num_seconds().max(0);
    if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else if s < 86_400 {
        format!("{}h ago", s / 3600)
    } else {
        format!("{}d ago", s / 86_400)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.replace('\n', " ");
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out.replace('\n', " ")
}

/// CLI helper: print the last N entries as JSONL to stdout.
pub fn print_recent(limit: usize) {
    let entries = recent();
    let start = entries.len().saturating_sub(limit);
    for e in &entries[start..] {
        if let Ok(s) = serde_json::to_string(e) {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{s}");
        }
    }
}

/// CLI helper: wipe the buffer.
pub fn clear() -> std::io::Result<()> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = paths::recent_context_file();
    if path.exists() {
        OpenOptions::new().write(true).truncate(true).open(&path)?;
    }
    Ok(())
}
