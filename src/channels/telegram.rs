use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, Mutex};

use crate::paths;

const POLL_TIMEOUT_SECS: u64 = 30;
const DRAFT_FLUSH_INTERVAL_MS: u64 = 200;

/// Main entrypoint — blocks until killed.
pub async fn start() -> Result<(), String> {
    let token = load_token()?;
    let allowlist = load_allowlist();
    let session_id = ensure_session_id()?;
    write_pidfile()?;

    eprintln!("telegram: session_id={session_id}");
    eprintln!("telegram: allowlist={:?}", allowlist);

    let (to_claude_tx, to_claude_rx) = mpsc::channel::<UserTurn>(16);
    let (to_tg_tx, to_tg_rx) = mpsc::channel::<TgOut>(128);

    let mut claude = spawn_claude_with_retry(&session_id).await?;
    let stdin = claude.stdin.take().ok_or("claude stdin not piped")?;
    let stdout = claude.stdout.take().ok_or("claude stdout not piped")?;
    tokio::spawn(async move {
        let _ = claude.wait().await;
        eprintln!("telegram: claude subprocess exited");
        std::process::exit(1);
    });

    tokio::spawn(feed_claude(stdin, to_claude_rx));
    tokio::spawn(read_claude(stdout, to_tg_tx.clone()));

    let client = reqwest::Client::new();
    tokio::spawn(telegram_writer(client.clone(), token.clone(), to_tg_rx));

    telegram_reader(client, token, allowlist, to_claude_tx).await
}

// --- Config / state ---

fn load_token() -> Result<String, String> {
    let path = dirs::home_dir()
        .ok_or("no home dir")?
        .join(".claude/channels/telegram/.env");
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    for line in contents.lines() {
        if let Some(v) = line.strip_prefix("TELEGRAM_BOT_TOKEN=") {
            return Ok(v.trim().to_string());
        }
    }
    Err(format!("TELEGRAM_BOT_TOKEN not found in {}", path.display()))
}

fn load_allowlist() -> Option<HashSet<i64>> {
    let path = dirs::home_dir()?.join(".claude/channels/telegram/access.json");
    let contents = std::fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&contents).ok()?;
    let ids = v.get("allowFrom")?.as_array()?;
    let set: HashSet<i64> = ids
        .iter()
        .filter_map(|x| x.as_str().and_then(|s| s.parse().ok()).or_else(|| x.as_i64()))
        .collect();
    if set.is_empty() { None } else { Some(set) }
}

fn channel_dir() -> PathBuf {
    paths::home().join("channels").join("telegram")
}

fn pidfile() -> PathBuf {
    channel_dir().join("pid")
}

fn write_pidfile() -> Result<(), String> {
    let dir = channel_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = pidfile();
    let pid = std::process::id();
    std::fs::write(&path, pid.to_string()).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

/// Send SIGTERM to a running telegram bot (reads pid from ~/.mimi/channels/telegram/pid).
pub fn stop() -> Result<(), String> {
    let path = pidfile();
    let pid_str = std::fs::read_to_string(&path)
        .map_err(|e| format!("no running bot (missing {}): {e}", path.display()))?;
    let pid: i32 = pid_str
        .trim()
        .parse()
        .map_err(|e| format!("bad pid in {}: {e}", path.display()))?;
    let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    if rc != 0 {
        return Err(format!("kill({pid}, SIGTERM) failed: errno {}", std::io::Error::last_os_error()));
    }
    let _ = std::fs::remove_file(&path);
    eprintln!("telegram: SIGTERM sent to {pid}");
    Ok(())
}

fn ensure_session_id() -> Result<String, String> {
    let dir = channel_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = dir.join("session_id");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::write(&path, &id).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(id)
}

// --- Claude subprocess ---

async fn spawn_claude_with_retry(session_id: &str) -> Result<tokio::process::Child, String> {
    let mut child = spawn_claude(session_id).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
    if let Ok(Some(status)) = child.try_wait() {
        eprintln!("telegram: claude exited {status} on first spawn — rotating session_id");
        let new_id = uuid::Uuid::new_v4().to_string();
        std::fs::write(channel_dir().join("session_id"), &new_id)
            .map_err(|e| format!("write session_id: {e}"))?;
        return spawn_claude(&new_id).await;
    }
    Ok(child)
}

async fn spawn_claude(session_id: &str) -> Result<tokio::process::Child, String> {
    let cwd = paths::home();
    let child = Command::new("claude")
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
            "--session-id",
            session_id,
            "--dangerously-skip-permissions",
        ])
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("failed to spawn claude: {e}"))?;
    Ok(child)
}

struct UserTurn {
    text: String,
}

async fn feed_claude(mut stdin: ChildStdin, mut rx: mpsc::Receiver<UserTurn>) {
    while let Some(turn) = rx.recv().await {
        let payload = json!({
            "type": "user",
            "message": { "role": "user", "content": turn.text }
        });
        let line = format!("{}\n", payload);
        if let Err(e) = stdin.write_all(line.as_bytes()).await {
            eprintln!("telegram: failed writing to claude stdin: {e}");
            return;
        }
        if let Err(e) = stdin.flush().await {
            eprintln!("telegram: failed flushing claude stdin: {e}");
            return;
        }
    }
}

// --- Claude → Telegram pipeline ---

enum TgOut {
    DraftChunk { text: String },
    Finalize { text: String },
}

static ACTIVE_CHAT: AtomicI64 = AtomicI64::new(0);

async fn read_claude(stdout: tokio::process::ChildStdout, tx: mpsc::Sender<TgOut>) {
    let mut reader = BufReader::new(stdout).lines();
    let mut accumulated = String::new();

    while let Ok(Some(line)) = reader.next_line().await {
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");

        match ty {
            "stream_event" => {
                if let Some(text) = extract_delta_text(&v) {
                    accumulated.push_str(&text);
                    let _ = tx
                        .send(TgOut::DraftChunk {
                            text: accumulated.clone(),
                        })
                        .await;
                }
            }
            "assistant" => {
                if let Some(text) = extract_full_text(&v) {
                    accumulated = text;
                }
            }
            "result" => {
                if !accumulated.is_empty() {
                    let _ = tx
                        .send(TgOut::Finalize {
                            text: std::mem::take(&mut accumulated),
                        })
                        .await;
                }
            }
            _ => {}
        }
    }
}

fn extract_delta_text(v: &Value) -> Option<String> {
    let event = v.get("event")?;
    let t = event.get("type").and_then(|x| x.as_str())?;
    if t != "content_block_delta" {
        return None;
    }
    let delta = event.get("delta")?;
    if delta.get("type").and_then(|x| x.as_str())? != "text_delta" {
        return None;
    }
    Some(delta.get("text")?.as_str()?.to_string())
}

fn extract_full_text(v: &Value) -> Option<String> {
    let content = v.get("message")?.get("content")?.as_array()?;
    let mut out = String::new();
    for block in content {
        if block.get("type").and_then(|x| x.as_str()) == Some("text") {
            if let Some(s) = block.get("text").and_then(|x| x.as_str()) {
                out.push_str(s);
            }
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

async fn telegram_writer(
    client: reqwest::Client,
    token: String,
    mut rx: mpsc::Receiver<TgOut>,
) {
    let mut draft_id: i64 = 1;
    let mut last_draft_text = String::new();
    let mut pending: Option<String> = None;
    let flush = tokio::time::Duration::from_millis(DRAFT_FLUSH_INTERVAL_MS);

    loop {
        let next = tokio::time::timeout(flush, rx.recv()).await;
        match next {
            Ok(Some(TgOut::DraftChunk { text })) => {
                pending = Some(text);
            }
            Ok(Some(TgOut::Finalize { text })) => {
                let chat_id = ACTIVE_CHAT.load(Ordering::SeqCst);
                if chat_id != 0 {
                    let _ = send_message(&client, &token, chat_id, &text).await;
                    if !text.trim().is_empty() {
                        crate::context_buffer::append_assistant(
                            "telegram",
                            &chat_id.to_string(),
                            &text,
                        );
                    }
                }
                pending = None;
                last_draft_text.clear();
                draft_id = draft_id.wrapping_add(1).max(1);
            }
            Ok(None) => break,
            Err(_) => {
                // timeout — flush pending draft
                if let Some(text) = pending.take() {
                    if text != last_draft_text {
                        let chat_id = ACTIVE_CHAT.load(Ordering::SeqCst);
                        if chat_id != 0 {
                            let _ = send_draft(&client, &token, chat_id, draft_id, &text).await;
                            last_draft_text = text;
                        }
                    }
                }
            }
        }
    }
}

async fn send_draft(
    client: &reqwest::Client,
    token: &str,
    chat_id: i64,
    draft_id: i64,
    text: &str,
) -> Result<(), String> {
    let truncated = truncate_for_telegram(text);
    let url = format!("https://api.telegram.org/bot{}/sendMessageDraft", token);
    let body = json!({
        "chat_id": chat_id,
        "draft_id": draft_id,
        "text": truncated,
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("sendMessageDraft: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eprintln!("telegram: sendMessageDraft {} {}", status, body);
    }
    Ok(())
}

async fn send_message(
    client: &reqwest::Client,
    token: &str,
    chat_id: i64,
    text: &str,
) -> Result<(), String> {
    let truncated = truncate_for_telegram(text);
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    let body = json!({ "chat_id": chat_id, "text": truncated });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("sendMessage: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eprintln!("telegram: sendMessage {} {}", status, body);
    }
    Ok(())
}

fn truncate_for_telegram(text: &str) -> String {
    // Telegram max is 4096 chars for text messages.
    let max = 4096;
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect()
    }
}

// --- Telegram → Claude pipeline ---

#[derive(Deserialize)]
struct Update {
    update_id: i64,
    message: Option<TgMessage>,
}

#[derive(Deserialize)]
struct TgMessage {
    message_id: i64,
    chat: TgChat,
    from: Option<TgUser>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct TgChat {
    id: i64,
    #[serde(default, rename = "type")]
    chat_type: Option<String>,
}

#[derive(Deserialize)]
struct TgUser {
    id: i64,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    first_name: Option<String>,
}

async fn telegram_reader(
    client: reqwest::Client,
    token: String,
    allowlist: Option<HashSet<i64>>,
    tx: mpsc::Sender<UserTurn>,
) -> Result<(), String> {
    let offset = Arc::new(Mutex::new(0i64));
    loop {
        let off = *offset.lock().await;
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates?timeout={}&offset={}&allowed_updates=[\"message\"]",
            token, POLL_TIMEOUT_SECS, off
        );
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("telegram: getUpdates error: {e}");
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }
        };
        let body: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("telegram: parse error: {e}");
                continue;
            }
        };
        let results = match body.get("result").and_then(|x| x.as_array()) {
            Some(r) => r,
            None => {
                eprintln!("telegram: unexpected response: {body}");
                let wait = if body.get("error_code").and_then(|x| x.as_i64()) == Some(409) {
                    35
                } else {
                    2
                };
                tokio::time::sleep(tokio::time::Duration::from_secs(wait)).await;
                continue;
            }
        };
        for u in results {
            let upd: Update = match serde_json::from_value(u.clone()) {
                Ok(x) => x,
                Err(_) => continue,
            };
            *offset.lock().await = upd.update_id + 1;
            let Some(msg) = upd.message else { continue };
            let Some(text) = msg.text else { continue };
            let from_id = msg.from.as_ref().map(|u| u.id).unwrap_or(0);
            if let Some(allow) = &allowlist
                && !allow.contains(&from_id)
            {
                eprintln!("telegram: blocked user {from_id}");
                continue;
            }
            ACTIVE_CHAT.store(msg.chat.id, Ordering::SeqCst);
            let user_name = msg.from.as_ref()
                .and_then(|u| u.username.clone().or_else(|| u.first_name.clone()))
                .unwrap_or_default();
            let chat_type = msg.chat.chat_type.as_deref().unwrap_or("private");
            let chat_id_str = msg.chat.id.to_string();
            let preamble = crate::context_buffer::preamble_for("telegram", &chat_id_str)
                .unwrap_or_default();
            let wrapped = format!(
                "{}<channel source=\"telegram\" chat_id=\"{}\" chat_type=\"{}\" user_id=\"{}\" user_name=\"{}\" message_id=\"{}\">\n{}\n</channel>",
                preamble, msg.chat.id, chat_type, from_id, user_name, msg.message_id, text
            );
            crate::context_buffer::append_user("telegram", &chat_id_str, &user_name, &text);
            let turn = UserTurn { text: wrapped };
            if tx.send(turn).await.is_err() {
                return Err("claude pipe closed".into());
            }
        }
    }
}
