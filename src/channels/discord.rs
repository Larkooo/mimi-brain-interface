use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::paths;

// Hardcoded: only this user may talk to Mimi over Discord.
const ALLOWED_USER_ID: u64 = 445355215013806081;

// Intents: GUILD_MESSAGES | DIRECT_MESSAGES. MESSAGE_CONTENT is privileged
// and not required — in DMs Discord always sends content, and in guilds
// content is provided whenever the bot is @mentioned or replied to, which
// are the only cases we react to.
const INTENTS: u64 = (1 << 9) | (1 << 12);

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const EDIT_THROTTLE_MS: u64 = 1500;

// Track the active channel ID (DM or guild channel) so the writer knows
// where to send. Stored as AtomicU64; 0 = none.
static ACTIVE_CHANNEL: AtomicU64 = AtomicU64::new(0);

// Set from the READY event payload. Used to detect @mentions and replies
// directed at us in guild channels.
static BOT_USER_ID: AtomicU64 = AtomicU64::new(0);

/// Main entrypoint — blocks until killed.
pub async fn start() -> Result<(), String> {
    let token = load_token()?;
    let session_id = ensure_session_id()?;
    write_pidfile()?;

    eprintln!("discord: session_id={session_id}");
    eprintln!("discord: allowed_user_id={ALLOWED_USER_ID}");

    let (to_claude_tx, to_claude_rx) = mpsc::channel::<UserTurn>(16);
    let (to_dc_tx, to_dc_rx) = mpsc::channel::<DcOut>(128);

    let mut claude = spawn_claude(&session_id).await?;
    let stdin = claude.stdin.take().ok_or("claude stdin not piped")?;
    let stdout = claude.stdout.take().ok_or("claude stdout not piped")?;
    tokio::spawn(async move {
        let _ = claude.wait().await;
        eprintln!("discord: claude subprocess exited");
        std::process::exit(1);
    });

    tokio::spawn(feed_claude(stdin, to_claude_rx));
    tokio::spawn(read_claude(stdout, to_dc_tx.clone()));

    let client = reqwest::Client::new();
    tokio::spawn(discord_writer(client.clone(), token.clone(), to_dc_rx));

    // Gateway reader loop — reconnects forever on disconnect.
    loop {
        if let Err(e) = run_gateway(&token, &to_claude_tx, &client).await {
            eprintln!("discord: gateway error: {e} — reconnecting in 5s");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

// --- Config / state ---

fn load_token() -> Result<String, String> {
    let path = dirs::home_dir()
        .ok_or("no home dir")?
        .join(".claude/channels/discord/.env");
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    for line in contents.lines() {
        if let Some(v) = line.strip_prefix("DISCORD_BOT_TOKEN=") {
            return Ok(v.trim().to_string());
        }
    }
    Err(format!("DISCORD_BOT_TOKEN not found in {}", path.display()))
}

fn channel_dir() -> PathBuf {
    paths::home().join("channels").join("discord")
}

fn pidfile() -> PathBuf {
    channel_dir().join("pid")
}

fn write_pidfile() -> Result<(), String> {
    let dir = channel_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    std::fs::write(pidfile(), std::process::id().to_string())
        .map_err(|e| format!("write pidfile: {e}"))
}

pub fn stop() -> Result<(), String> {
    let path = pidfile();
    let pid_str = std::fs::read_to_string(&path)
        .map_err(|e| format!("no running bot (missing {}): {e}", path.display()))?;
    let pid: i32 = pid_str.trim().parse().map_err(|e| format!("bad pid: {e}"))?;
    let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    if rc != 0 {
        return Err(format!("kill({pid}) failed: {}", std::io::Error::last_os_error()));
    }
    let _ = std::fs::remove_file(&path);
    eprintln!("discord: SIGTERM sent to {pid}");
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
    std::fs::write(&path, &id).map_err(|e| format!("write session_id: {e}"))?;
    Ok(id)
}

// --- Claude subprocess (same pattern as telegram.rs) ---

async fn spawn_claude(session_id: &str) -> Result<tokio::process::Child, String> {
    let cwd = paths::home();
    Command::new("claude")
        .args([
            "-p",
            "--input-format", "stream-json",
            "--output-format", "stream-json",
            "--include-partial-messages",
            "--verbose",
            "--session-id", session_id,
            "--dangerously-skip-permissions",
        ])
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("failed to spawn claude: {e}"))
}

struct UserTurn { text: String }

async fn feed_claude(mut stdin: ChildStdin, mut rx: mpsc::Receiver<UserTurn>) {
    while let Some(turn) = rx.recv().await {
        let payload = json!({
            "type": "user",
            "message": { "role": "user", "content": turn.text }
        });
        let line = format!("{}\n", payload);
        if stdin.write_all(line.as_bytes()).await.is_err() { return; }
        if stdin.flush().await.is_err() { return; }
    }
}

// --- Claude stdout → Discord pipeline ---

enum DcOut {
    Chunk { text: String },
    Finalize { text: String },
}

async fn read_claude(stdout: tokio::process::ChildStdout, tx: mpsc::Sender<DcOut>) {
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
                    let _ = tx.send(DcOut::Chunk { text: accumulated.clone() }).await;
                }
            }
            "assistant" => {
                if let Some(text) = extract_full_text(&v) {
                    accumulated = text;
                }
            }
            "result" => {
                if !accumulated.is_empty() {
                    let _ = tx.send(DcOut::Finalize { text: std::mem::take(&mut accumulated) }).await;
                }
            }
            _ => {}
        }
    }
}

fn extract_delta_text(v: &Value) -> Option<String> {
    let event = v.get("event")?;
    if event.get("type").and_then(|x| x.as_str())? != "content_block_delta" {
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

// Discord has no sendMessageDraft equivalent. We stream by editing a single
// message in place every EDIT_THROTTLE_MS as content grows.
async fn discord_writer(
    client: reqwest::Client,
    token: String,
    mut rx: mpsc::Receiver<DcOut>,
) {
    let mut active_message_id: Option<u64> = None;
    let mut last_sent_text = String::new();
    let mut pending: Option<String> = None;
    let throttle = Duration::from_millis(EDIT_THROTTLE_MS);

    loop {
        let next = tokio::time::timeout(throttle, rx.recv()).await;
        match next {
            Ok(Some(DcOut::Chunk { text })) => {
                pending = Some(text);
            }
            Ok(Some(DcOut::Finalize { text })) => {
                let chan = ACTIVE_CHANNEL.load(Ordering::SeqCst);
                if chan != 0 {
                    if let Some(msg_id) = active_message_id.take() {
                        let _ = edit_message(&client, &token, chan, msg_id, &text).await;
                    } else {
                        let _ = send_message(&client, &token, chan, &text).await;
                    }
                }
                pending = None;
                last_sent_text.clear();
            }
            Ok(None) => break,
            Err(_) => {
                // Throttle tick — flush pending draft if changed
                if let Some(text) = pending.take() {
                    if text == last_sent_text { continue; }
                    let chan = ACTIVE_CHANNEL.load(Ordering::SeqCst);
                    if chan == 0 { continue; }
                    last_sent_text = text.clone();
                    match active_message_id {
                        Some(msg_id) => {
                            let _ = edit_message(&client, &token, chan, msg_id, &text).await;
                        }
                        None => {
                            if let Ok(id) = send_message(&client, &token, chan, &text).await {
                                active_message_id = Some(id);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn truncate(text: &str) -> String {
    // Discord max: 2000 chars.
    let max = 2000;
    if text.chars().count() <= max { text.to_string() }
    else { text.chars().take(max).collect() }
}

async fn send_message(
    client: &reqwest::Client,
    token: &str,
    channel_id: u64,
    text: &str,
) -> Result<u64, String> {
    let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages");
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bot {token}"))
        .json(&json!({ "content": truncate(text) }))
        .send()
        .await
        .map_err(|e| format!("send_message: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("send_message {} {}", status, body));
    }
    let v: Value = resp.json().await.map_err(|e| format!("parse: {e}"))?;
    v.get("id").and_then(|x| x.as_str()).and_then(|s| s.parse().ok())
        .ok_or_else(|| "no id in message response".to_string())
}

async fn edit_message(
    client: &reqwest::Client,
    token: &str,
    channel_id: u64,
    message_id: u64,
    text: &str,
) -> Result<(), String> {
    let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages/{message_id}");
    let resp = client
        .patch(&url)
        .header("Authorization", format!("Bot {token}"))
        .json(&json!({ "content": truncate(text) }))
        .send()
        .await
        .map_err(|e| format!("edit_message: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eprintln!("discord: edit_message {} {}", status, body);
    }
    Ok(())
}

// --- Gateway loop ---

async fn run_gateway(
    token: &str,
    to_claude: &mpsc::Sender<UserTurn>,
    _client: &reqwest::Client,
) -> Result<(), String> {
    let (ws, _) = connect_async(GATEWAY_URL).await.map_err(|e| format!("connect: {e}"))?;
    let (mut write, mut read) = ws.split();

    // Receive HELLO
    let hello = read.next().await
        .ok_or("gateway closed before HELLO")?
        .map_err(|e| format!("ws read: {e}"))?;
    let hello_text = match hello {
        Message::Text(t) => t.to_string(),
        _ => return Err("unexpected first gateway frame".into()),
    };
    let hello_json: Value = serde_json::from_str(&hello_text).map_err(|e| format!("parse hello: {e}"))?;
    let heartbeat_ms = hello_json.pointer("/d/heartbeat_interval").and_then(|x| x.as_u64())
        .ok_or("no heartbeat_interval in HELLO")?;

    // IDENTIFY
    let identify = json!({
        "op": 2,
        "d": {
            "token": token,
            "intents": INTENTS,
            "properties": { "os": "linux", "browser": "mimi", "device": "mimi" }
        }
    });
    write.send(Message::Text(identify.to_string().into())).await
        .map_err(|e| format!("send identify: {e}"))?;

    let write = std::sync::Arc::new(tokio::sync::Mutex::new(write));

    // Heartbeat task
    let hb_write = std::sync::Arc::clone(&write);
    let last_seq = std::sync::Arc::new(tokio::sync::RwLock::new(None::<u64>));
    let hb_seq = std::sync::Arc::clone(&last_seq);
    let hb_task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(heartbeat_ms));
        ticker.tick().await; // fire the immediate tick
        loop {
            ticker.tick().await;
            let seq = *hb_seq.read().await;
            let msg = json!({ "op": 1, "d": seq });
            let mut w = hb_write.lock().await;
            if w.send(Message::Text(msg.to_string().into())).await.is_err() {
                return;
            }
        }
    });

    // Main event loop
    while let Some(msg) = read.next().await {
        let msg = msg.map_err(|e| format!("ws read: {e}"))?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => { hb_task.abort(); return Err("gateway closed".into()); }
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => continue };
        if let Some(s) = v.get("s").and_then(|x| x.as_u64()) {
            *last_seq.write().await = Some(s);
        }
        let op = v.get("op").and_then(|x| x.as_u64()).unwrap_or(0);
        if op != 0 { continue; } // 0 = dispatched event
        let event = v.get("t").and_then(|x| x.as_str()).unwrap_or("");

        if event == "READY" {
            if let Some(id) = v.pointer("/d/user/id").and_then(|x| x.as_str()).and_then(|s| s.parse::<u64>().ok()) {
                BOT_USER_ID.store(id, Ordering::SeqCst);
                eprintln!("discord: ready, bot_user_id={id}");
            }
            continue;
        }

        if event != "MESSAGE_CREATE" {
            if event != "TYPING_START" && event != "PRESENCE_UPDATE" {
                eprintln!("discord: event={event}");
            }
            continue;
        }
        let d = match v.get("d") { Some(d) => d, None => continue };

        let author_id: u64 = d.pointer("/author/id").and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok()).unwrap_or(0);
        let is_bot = d.pointer("/author/bot").and_then(|x| x.as_bool()).unwrap_or(false);
        let channel_id: u64 = d.get("channel_id").and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok()).unwrap_or(0);
        let content = d.get("content").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let in_guild = d.get("guild_id").is_some();
        let mentions_count = d.get("mentions").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
        eprintln!(
            "discord: MSG author={author_id} bot={is_bot} guild={in_guild} channel={channel_id} mentions={mentions_count} content_len={}",
            content.len()
        );

        if is_bot { continue; }
        if author_id != ALLOWED_USER_ID {
            eprintln!("discord: blocked user {author_id}");
            continue;
        }
        if content.is_empty() || channel_id == 0 { continue; }

        // Guild messages: only respond if the bot is @mentioned or the
        // message is a reply to one of the bot's messages.
        if in_guild {
            let bot_id = BOT_USER_ID.load(Ordering::SeqCst);
            let mentioned = d.get("mentions").and_then(|x| x.as_array())
                .map(|arr| arr.iter().any(|m| {
                    m.get("id").and_then(|x| x.as_str())
                        .and_then(|s| s.parse::<u64>().ok()) == Some(bot_id)
                }))
                .unwrap_or(false);
            let replied_to_us = d.pointer("/referenced_message/author/id")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse::<u64>().ok()) == Some(bot_id);
            if !mentioned && !replied_to_us {
                eprintln!("discord: guild msg ignored (not mentioned, not reply to us)");
                continue;
            }
        }

        ACTIVE_CHANNEL.store(channel_id, Ordering::SeqCst);
        let _ = to_claude.send(UserTurn { text: content }).await;
    }

    hb_task.abort();
    Err("gateway stream ended".into())
}
