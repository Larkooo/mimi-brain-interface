// Dashboard handlers for the /api/subagents endpoints.
//
// Mirrors the CLI surface in `crate::subagents`:
//   GET    /api/subagents                  list all
//   GET    /api/subagents/:id              meta + recent events
//   GET    /api/subagents/:id/events       SSE stream of redacted events
//   POST   /api/subagents/:id/send         send a user-turn message
//   POST   /api/subagents/:id/stop         SIGTERM supervisor
//   DELETE /api/subagents/:id              remove (only if not running)
//   POST   /api/subagents/spawn            spawn a new subagent

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Json;
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::StreamExt as _;

use crate::subagents;

// ---------- redaction ----------
//
// Mask common secret-looking blobs before any value escapes to a browser.
// Best-effort regex-free scanning — runs once per outbound event.

fn redact_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Try each token-like prefix in turn.
        if let Some((skip, replacement)) = match_secret(&bytes[i..], s, i) {
            out.push_str(&replacement);
            i += skip;
        } else {
            // Push one char (handle utf-8 by walking via &str, not bytes).
            // Find the next char boundary.
            let next = (i + 1..=s.len())
                .find(|&j| s.is_char_boundary(j))
                .unwrap_or(s.len());
            out.push_str(&s[i..next]);
            i = next;
        }
    }
    out
}

/// Returns Some((bytes_to_skip, replacement_text)) if a known secret token
/// starts at position `i`. Otherwise None.
fn match_secret(rest: &[u8], full: &str, i: usize) -> Option<(usize, String)> {
    // sk-... (Anthropic / OpenAI keys)
    if rest.starts_with(b"sk-") {
        let len = ascii_token_len(rest, 3);
        if len >= 20 { return Some((len, format!("sk-***[{}]", len - 3))); }
    }
    // ghp_..., gho_..., ghu_..., ghs_..., ghr_... (GitHub PATs)
    if rest.len() >= 4 && (rest.starts_with(b"ghp_") || rest.starts_with(b"gho_")
        || rest.starts_with(b"ghu_") || rest.starts_with(b"ghs_") || rest.starts_with(b"ghr_"))
    {
        let len = ascii_token_len(rest, 4);
        if len >= 20 {
            let prefix = std::str::from_utf8(&rest[..4]).unwrap_or("ghx_");
            return Some((len, format!("{prefix}***[{}]", len - 4)));
        }
    }
    // xoxb-... / xoxa-... / xoxp-... (Slack)
    if rest.len() >= 5 && rest.starts_with(b"xox") {
        let prefix_byte = rest[3];
        if prefix_byte == b'b' || prefix_byte == b'a' || prefix_byte == b'p' {
            if rest[4] == b'-' {
                let len = ascii_token_len(rest, 5);
                if len >= 25 {
                    let prefix = std::str::from_utf8(&rest[..5]).unwrap_or("xox?-");
                    return Some((len, format!("{prefix}***[{}]", len - 5)));
                }
            }
        }
    }
    // AWS access key id: AKIA + 16 base32-ish chars
    if rest.starts_with(b"AKIA") && rest.len() >= 20 {
        let len = ascii_token_len(rest, 4);
        if len == 20 || (len >= 20 && len <= 24) {
            return Some((len, format!("AKIA***[{}]", len - 4)));
        }
    }
    // Bearer <40+ chars>
    if rest.starts_with(b"Bearer ") {
        let len = ascii_token_len(rest, 7);
        if len >= 47 { return Some((len, format!("Bearer ***[{}]", len - 7))); }
    }
    // Long contiguous base64 blob — only mask if it's not obviously inside a
    // known-context tag. Look for runs of 60+ chars from [A-Za-z0-9+/=_-].
    // Cheap heuristic: only trigger if the previous char is whitespace, ", :,
    // or start-of-string — avoids munging URLs.
    let prev = if i == 0 { b' ' } else {
        full.as_bytes().get(i.saturating_sub(1)).copied().unwrap_or(b' ')
    };
    if matches!(prev, b' ' | b'\t' | b'\n' | b'"' | b':' | b'=' | b',' | b'\'' | b'>') {
        let mut j = 0;
        while j < rest.len() {
            let c = rest[j];
            let ok = c.is_ascii_alphanumeric() || c == b'+' || c == b'/' || c == b'=' || c == b'_' || c == b'-';
            if !ok { break; }
            j += 1;
        }
        if j >= 60 {
            // But: only redact if it doesn't look like a path or URL. (Has no `/` and `=` count makes sense.)
            // Skip if it contains a slash -> probably a path.
            if !rest[..j].iter().any(|&b| b == b'/') {
                return Some((j, format!("[redacted-blob:{j}]")));
            }
        }
    }
    None
}

/// Length of an ASCII token (alphanumeric + `_-`) starting at offset `start`.
fn ascii_token_len(rest: &[u8], start: usize) -> usize {
    let mut j = start;
    while j < rest.len() {
        let c = rest[j];
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' { j += 1; }
        else { break; }
    }
    j
}

/// Walk a serde_json Value recursively and redact every string leaf.
fn redact_value(v: &mut Value) {
    match v {
        Value::String(s) => {
            let r = redact_str(s);
            if r != *s { *s = r; }
        }
        Value::Array(arr) => arr.iter_mut().for_each(redact_value),
        Value::Object(map) => map.values_mut().for_each(redact_value),
        _ => {}
    }
}

fn redact_meta(m: &mut subagents::Meta) {
    m.system_prompt = redact_str(&m.system_prompt);
}

// ---------- handlers ----------

#[derive(Serialize)]
pub struct AgentSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub model: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub pid: Option<i32>,
    pub claude_pid: Option<i32>,
    pub exit_code: Option<i32>,
    pub elapsed_seconds: i64,
    pub last_event_preview: String,
    pub last_event_type: String,
}

fn elapsed_seconds(started: &str, ended: Option<&str>) -> i64 {
    let start = chrono::DateTime::parse_from_rfc3339(started)
        .or_else(|_| chrono::DateTime::parse_from_str(started, "%Y-%m-%dT%H:%M:%SZ"))
        .ok();
    let end = ended.and_then(|t| {
        chrono::DateTime::parse_from_rfc3339(t)
            .or_else(|_| chrono::DateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%SZ"))
            .ok()
    });
    let now = chrono::Utc::now();
    match (start, end) {
        (Some(s), Some(e)) => (e.timestamp() - s.timestamp()).max(0),
        (Some(s), None) => (now.timestamp() - s.timestamp()).max(0),
        _ => 0,
    }
}

fn last_event(id: &str) -> (String, String) {
    let events = subagents::tail_events(id, 5).unwrap_or_default();
    if let Some(e) = events.last() {
        let ty = e.get("type").and_then(|x| x.as_str()).unwrap_or("?").to_string();
        let preview = render_preview(e);
        return (ty, redact_str(&preview));
    }
    (String::new(), String::new())
}

fn render_preview(v: &Value) -> String {
    let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    match ty {
        "assistant" => {
            let blocks = v.pointer("/message/content").and_then(|x| x.as_array());
            if let Some(blocks) = blocks {
                let mut bits: Vec<String> = Vec::new();
                for b in blocks {
                    let bt = b.get("type").and_then(|x| x.as_str()).unwrap_or("");
                    if bt == "text" {
                        let txt = b.get("text").and_then(|x| x.as_str()).unwrap_or("");
                        let one = txt.replace('\n', " ");
                        bits.push(truncate(&one, 120));
                    } else if bt == "tool_use" {
                        let n = b.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                        bits.push(format!("[{n}]"));
                    }
                }
                return bits.join(" ");
            }
            String::new()
        }
        "result" => {
            let dur = v.get("duration_ms").and_then(|x| x.as_u64()).unwrap_or(0);
            format!("turn complete ({dur}ms)")
        }
        _ => truncate(&serde_json::to_string(v).unwrap_or_default(), 120),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() }
    else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

pub async fn api_list() -> Json<Vec<AgentSummary>> {
    let agents = subagents::list_all();
    let summaries = agents.into_iter().map(|m| {
        let elapsed = elapsed_seconds(&m.started_at, m.ended_at.as_deref());
        let (last_type, last_preview) = last_event(&m.id);
        AgentSummary {
            id: m.id,
            name: m.name,
            status: m.status,
            model: m.model,
            started_at: m.started_at,
            ended_at: m.ended_at,
            pid: m.pid,
            claude_pid: m.claude_pid,
            exit_code: m.exit_code,
            elapsed_seconds: elapsed,
            last_event_preview: last_preview,
            last_event_type: last_type,
        }
    }).collect();
    Json(summaries)
}

#[derive(Serialize)]
pub struct AgentDetail {
    pub meta: subagents::Meta,
    pub elapsed_seconds: i64,
    pub events: Vec<Value>,
}

pub async fn api_get(Path(id): Path<String>) -> Result<Json<AgentDetail>, (StatusCode, String)> {
    subagents::reap_if_dead(&id);
    let mut meta = subagents::read_meta(&id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    let elapsed = elapsed_seconds(&meta.started_at, meta.ended_at.as_deref());
    redact_meta(&mut meta);
    let mut events = subagents::tail_events(&id, 200).unwrap_or_default();
    for e in events.iter_mut() { redact_value(e); }
    Ok(Json(AgentDetail { meta, elapsed_seconds: elapsed, events }))
}

#[derive(Deserialize)]
pub struct SpawnBody {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

pub async fn api_spawn(Json(body): Json<SpawnBody>) -> Result<Json<Value>, (StatusCode, String)> {
    if body.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name required".into()));
    }
    if body.prompt.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "prompt required".into()));
    }
    let id = subagents::spawn(&body.name, &body.prompt, body.model.as_deref(), body.cwd.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct SendBody {
    pub message: String,
}

pub async fn api_send(
    Path(id): Path<String>,
    Json(body): Json<SendBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if body.message.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message required".into()));
    }
    subagents::send(&id, &body.message)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn api_stop(Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, String)> {
    subagents::stop(&id).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn api_delete(Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, String)> {
    subagents::rm(&id).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- SSE: live stream.jsonl tail ----------

/// Tail stream.jsonl, emit each new line as an SSE event with the parsed
/// (and redacted) JSON. Closes when the client disconnects.
pub async fn api_events(Path(id): Path<String>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let dir = subagents::agent_dir(&id);
    let stream_path = dir.join("stream.jsonl");

    let stream = async_stream::stream! {
        // Send the recent backlog first so the UI has immediate context.
        if let Ok(mut events) = subagents::tail_events(&id, 50) {
            for e in events.iter_mut() {
                redact_value(e);
                if let Ok(s) = serde_json::to_string(e) {
                    yield Ok::<_, Infallible>(Event::default().event("event").data(s));
                }
            }
        }

        let mut pos: u64 = match std::fs::metadata(&stream_path) {
            Ok(m) => m.len(),
            Err(_) => 0,
        };
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            interval.tick().await;
            let len = match std::fs::metadata(&stream_path) {
                Ok(m) => m.len(),
                Err(_) => { continue; }
            };
            if len < pos {
                // file truncated/rotated — restart from 0.
                pos = 0;
            }
            if len == pos { continue; }
            // Read [pos..len] and emit lines.
            use std::io::{Read, Seek, SeekFrom};
            let mut f = match std::fs::File::open(&stream_path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            if f.seek(SeekFrom::Start(pos)).is_err() { continue; }
            let mut buf = vec![0u8; (len - pos) as usize];
            if f.read_exact(&mut buf).is_err() { continue; }
            pos = len;
            let text = String::from_utf8_lossy(&buf).to_string();
            for line in text.lines() {
                if line.trim().is_empty() { continue; }
                if let Ok(mut v) = serde_json::from_str::<Value>(line) {
                    redact_value(&mut v);
                    if let Ok(s) = serde_json::to_string(&v) {
                        yield Ok(Event::default().event("event").data(s));
                    }
                }
            }
            // Also emit a status snapshot so the UI badges update.
            if let Ok(meta) = subagents::read_meta(&id) {
                if let Ok(s) = serde_json::to_string(&serde_json::json!({
                    "_status": meta.status,
                    "_ended_at": meta.ended_at,
                    "_exit_code": meta.exit_code,
                })) {
                    yield Ok(Event::default().event("status").data(s));
                }
            }
        }
    };

    Sse::new(stream.map(|r| r)).keep_alive(KeepAlive::default())
}
