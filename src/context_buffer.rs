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

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::paths;

const MAX_LINES: usize = 300;
const CONTEXT_LOOKBACK: usize = 20;
const SAME_CHANNEL_WINDOW_SECS: i64 = 60;
const FIRST_TURN_LOOKBACK: usize = 40;
/// Passive-awareness raw tail: include up to N recent same-channel entries
/// in every preamble, regardless of age, so mimi catches up on chatter she
/// wasn't triggered on. Bigger = more context tokens per turn.
const SAME_CHANNEL_TAIL: usize = 8;

static FILE_LOCK: Mutex<()> = Mutex::new(());

fn seen_channels() -> &'static Mutex<HashSet<String>> {
    static INSTANCE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub ts: DateTime<Utc>,
    pub source: String,
    pub chat_id: String,
    #[serde(default)]
    pub user_name: String,
    pub kind: Kind,
    pub text: String,
    // Native message id on the source platform (Discord msg id, Telegram
    // msg id). Stored as string for portability. Used so mimi can see
    // `msg=<id>` in recent_context and route her reply to a specific older
    // message via `[reply:<id>]` prefix. Optional — old entries and
    // assistant/reaction entries may lack it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    User,
    Assistant,
    Reaction,
}

pub fn append_user(
    source: &str,
    chat_id: &str,
    user_name: &str,
    text: &str,
    message_id: Option<&str>,
) {
    append(Entry {
        ts: Utc::now(),
        source: source.into(),
        chat_id: chat_id.into(),
        user_name: user_name.into(),
        kind: Kind::User,
        text: text.into(),
        message_id: message_id.map(str::to_string),
    });
}

pub fn append_assistant(
    source: &str,
    chat_id: &str,
    text: &str,
    message_id: Option<&str>,
) {
    append(Entry {
        ts: Utc::now(),
        source: source.into(),
        chat_id: chat_id.into(),
        user_name: String::new(),
        kind: Kind::Assistant,
        text: text.into(),
        message_id: message_id.map(str::to_string),
    });
}

/// Record that someone added an emoji reaction to one of our assistant
/// messages. Surfaces in the next turn's `<recent_context>` preamble so mimi
/// sees inline feedback like `[2m ago · splitterr@discord 👍] reacted
/// <:roflmao:> to my msg: "…"` and can calibrate register in real time.
pub fn append_reaction(
    source: &str,
    chat_id: &str,
    reactor_name: &str,
    emoji: &str,
    target_excerpt: &str,
) {
    let text = format!("reacted {emoji} to my msg: \"{target_excerpt}\"");
    append(Entry {
        ts: Utc::now(),
        source: source.into(),
        chat_id: chat_id.into(),
        user_name: reactor_name.into(),
        kind: Kind::Reaction,
        text,
        message_id: None,
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

    // After a binary restart, the claude subprocess loses its in-memory
    // conversation state. On the first turn seen per (source, chat_id) since
    // this process started, surface the full rolling log for that channel so
    // mimi can pick up the conversation instead of waking up amnesiac.
    let key = format!("{current_source}:{current_chat_id}");
    let is_first_turn = seen_channels()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key);
    let lookback = if is_first_turn { FIRST_TURN_LOOKBACK } else { CONTEXT_LOOKBACK };

    // Two streams feed the preamble:
    //   • cross_channel: msgs from OTHER channels — capped at `lookback`
    //   • same_channel_tail: latest N msgs from the SAME channel, unconditional,
    //     so mimi has a passive window on unmentioned chatter.
    // We merge them by timestamp for natural reading order. Bump the cap to
    // `FIRST_TURN_LOOKBACK` on first-turn-after-restart to recover the full log.
    let same_channel_cap = if is_first_turn { FIRST_TURN_LOOKBACK } else { SAME_CHANNEL_TAIL };

    let mut cross_channel: Vec<&Entry> = entries
        .iter()
        .rev()
        .filter(|e| e.source != current_source || e.chat_id != current_chat_id)
        .take(lookback)
        .collect();

    let mut same_channel: Vec<&Entry> = entries
        .iter()
        .rev()
        .filter(|e| e.source == current_source && e.chat_id == current_chat_id)
        .take(same_channel_cap)
        .collect();

    // Merge by ts ascending. Entries are naturally sorted in the log, so
    // reversing each slice then walking them once is enough.
    cross_channel.reverse();
    same_channel.reverse();
    let mut picks: Vec<&Entry> = Vec::with_capacity(cross_channel.len() + same_channel.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < cross_channel.len() && j < same_channel.len() {
        if cross_channel[i].ts <= same_channel[j].ts {
            picks.push(cross_channel[i]); i += 1;
        } else {
            picks.push(same_channel[j]); j += 1;
        }
    }
    picks.extend_from_slice(&cross_channel[i..]);
    picks.extend_from_slice(&same_channel[j..]);

    // Drop SAME_CHANNEL_WINDOW_SECS filter — the explicit `same_channel` slice
    // above already gives mimi the recent tail she needs. (Kept the const in
    // scope for future time-gated variants; unused for now.)
    let _ = SAME_CHANNEL_WINDOW_SECS;

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
            Kind::Reaction => {
                if e.user_name.is_empty() {
                    format!("reaction@{}", e.source)
                } else {
                    format!("{}@{} 👍", e.user_name, e.source)
                }
            }
        };
        let text = truncate(&e.text, 400);
        let id_tag = match &e.message_id {
            Some(id) if !id.is_empty() => format!(" msg={id}"),
            _ => String::new(),
        };
        out.push_str(&format!("[{age} · {who}{id_tag}] {text}\n"));
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
