use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, Mutex};

use crate::paths;

const POLL_TIMEOUT_SECS: u64 = 30;

// Appended to every Telegram turn. Tells Mimi her stdout is not the wire
// anymore — outbound must go through `telegram` Bash-wrapper tool calls.
const OUTBOUND_PROTOCOL: &str = "<system-reminder>\n\
TELEGRAM OUTBOUND PROTOCOL — read before replying.\n\
\n\
This bridge is pure tool-call. Your stdout/assistant text is NOT delivered to Telegram. Anything you say without a tool call is invisible to the chat — only the server logs see it. To send a message you MUST call `Bash` with one of the `telegram` CLI wrappers in `~/.mimi/bin/`:\n\
\n\
- `telegram reply <chat_id> <triggering_msg_id> \"<text>\"` — quote-reply to the triggering message.\n\
- `telegram post <chat_id> \"<text>\"` — plain message, no quote thread.\n\
- `telegram edit <chat_id> <msg_id> \"<text>\"` — edit a message you sent earlier.\n\
- `telegram react <chat_id> <msg_id> <emoji>` — drop a reaction (unicode emoji; Telegram allows only a fixed set).\n\
- `telegram delete <chat_id> <msg_id>` — remove one of your messages.\n\
- `telegram typing <chat_id>` — optional, shows the typing bubble briefly (~5s).\n\
\n\
The triggering `<channel>` tag on every inbound message carries `chat_id`, `message_id`, and `user_id` — read those directly. Never output conversational text without a wrapper call; if you intend to say nothing, say nothing and finish the turn.\n\
</system-reminder>\n";

/// Main entrypoint — blocks until killed.
pub async fn start() -> Result<(), String> {
    let token = load_token()?;
    let allowlist = load_allowlist();
    let session_id = ensure_session_id()?;
    write_pidfile()?;

    eprintln!("telegram: session_id={session_id}");
    eprintln!("telegram: allowlist={:?}", allowlist);

    let (to_claude_tx, to_claude_rx) = mpsc::channel::<UserTurn>(16);
    let (typing_tx, typing_rx) = mpsc::channel::<TypingCmd>(64);

    let mut claude = spawn_claude_with_retry(&session_id).await?;
    let stdin = claude.stdin.take().ok_or("claude stdin not piped")?;
    let stdout = claude.stdout.take().ok_or("claude stdout not piped")?;
    tokio::spawn(async move {
        let _ = claude.wait().await;
        eprintln!("telegram: claude subprocess exited");
        std::process::exit(1);
    });

    tokio::spawn(feed_claude(stdin, to_claude_rx, typing_tx.clone()));
    tokio::spawn(drain_claude(stdout, typing_tx));

    let client = reqwest::Client::new();
    tokio::spawn(typing_loop(client.clone(), token.clone(), typing_rx));
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
///
/// If the recorded pid is already gone (ESRCH) — e.g. the bot crashed before
/// we got here — treat it as a successful stop and clean up the stale pidfile,
/// so the next `mimi channel start telegram` doesn't have to manually clear it.
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
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ESRCH) {
            return Err(format!("kill({pid}, SIGTERM) failed: errno {err}"));
        }
        eprintln!("telegram: pid {pid} already gone — clearing stale pidfile");
    } else {
        eprintln!("telegram: SIGTERM sent to {pid}");
    }
    let _ = std::fs::remove_file(&path);
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
    chat_id: i64,
}

// See discord.rs::TypingCmd — same shape, Telegram chat ids are i64.
enum TypingCmd {
    Start(i64),
    Stop,
}

async fn feed_claude(
    mut stdin: ChildStdin,
    mut rx: mpsc::Receiver<UserTurn>,
    typing_tx: mpsc::Sender<TypingCmd>,
) {
    while let Some(turn) = rx.recv().await {
        let _ = typing_tx.send(TypingCmd::Start(turn.chat_id)).await;
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

// --- Claude stdout drainer ---
//
// The bridge no longer interprets Claude's stdout. Every outbound message
// goes through `telegram` Bash-wrapper tool calls that Claude makes
// herself. We still drain stdout so Claude's pipe doesn't fill and block,
// and we eprintln! a one-line heartbeat on `result` for debugging.
async fn drain_claude(
    stdout: tokio::process::ChildStdout,
    typing_tx: mpsc::Sender<TypingCmd>,
) {
    let mut reader = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if ty == "result" {
            let duration = v.get("duration_ms").and_then(|x| x.as_u64()).unwrap_or(0);
            let num_turns = v.get("num_turns").and_then(|x| x.as_u64()).unwrap_or(0);
            let subtype = v.get("subtype").and_then(|x| x.as_str()).unwrap_or("");
            eprintln!(
                "telegram: turn result subtype={subtype} duration_ms={duration} num_turns={num_turns}"
            );
            let _ = typing_tx.send(TypingCmd::Stop).await;
        }
    }
}

// See discord.rs::typing_loop. Telegram's sendChatAction lights the bubble
// for ~5s, so we re-fire every 4s. Same refcount + safety-cap semantics.
async fn typing_loop(
    client: reqwest::Client,
    token: String,
    mut rx: mpsc::Receiver<TypingCmd>,
) {
    const TICK: Duration = Duration::from_secs(4);
    const SAFETY_CAP: Duration = Duration::from_secs(300);

    let mut active: Option<i64> = None;
    let mut pending: u32 = 0;
    let mut started_at: Option<std::time::Instant> = None;
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;

    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(TypingCmd::Start(chan)) => {
                        pending = pending.saturating_add(1);
                        let switching = active != Some(chan);
                        active = Some(chan);
                        if started_at.is_none() || switching {
                            started_at = Some(std::time::Instant::now());
                        }
                        send_typing_once(&client, &token, chan).await;
                        interval.reset();
                    }
                    Some(TypingCmd::Stop) => {
                        pending = pending.saturating_sub(1);
                        if pending == 0 {
                            active = None;
                            started_at = None;
                        }
                    }
                    None => return,
                }
            }
            _ = interval.tick() => {
                if let Some(chan) = active {
                    if started_at.map(|t| t.elapsed() > SAFETY_CAP).unwrap_or(false) {
                        eprintln!(
                            "telegram: typing heartbeat hit 5min safety cap chan={chan} pending={pending} — clearing"
                        );
                        active = None;
                        pending = 0;
                        started_at = None;
                        continue;
                    }
                    send_typing_once(&client, &token, chan).await;
                }
            }
        }
    }
}

async fn send_typing_once(client: &reqwest::Client, token: &str, chat_id: i64) {
    let url = format!("https://api.telegram.org/bot{token}/sendChatAction");
    let body = json!({ "chat_id": chat_id, "action": "typing" });
    if let Err(e) = client.post(&url).json(&body).send().await {
        eprintln!("telegram: typing heartbeat POST failed chat={chat_id}: {e}");
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
            let user_name = msg.from.as_ref()
                .and_then(|u| u.username.clone().or_else(|| u.first_name.clone()))
                .unwrap_or_default();
            let chat_type = msg.chat.chat_type.as_deref().unwrap_or("private");
            let chat_id_str = msg.chat.id.to_string();
            let preamble = crate::context_buffer::preamble_for("telegram", &chat_id_str)
                .unwrap_or_default();
            let time_ctx = crate::channels::time_context_preamble();
            let wrapped = format!(
                "{}{}{}<channel source=\"telegram\" chat_id=\"{}\" chat_type=\"{}\" user_id=\"{}\" user_name=\"{}\" message_id=\"{}\">\n{}\n</channel>",
                time_ctx, OUTBOUND_PROTOCOL, preamble, msg.chat.id, chat_type, from_id, user_name, msg.message_id, text
            );
            let tg_msg_id_str = msg.message_id.to_string();
            crate::context_buffer::append_user(
                "telegram",
                &chat_id_str,
                &user_name,
                &text,
                Some(&tg_msg_id_str),
            );
            eprintln!(
                "telegram: dispatch chat={} msg={} user={}",
                msg.chat.id, msg.message_id, from_id
            );
            let turn = UserTurn { text: wrapped, chat_id: msg.chat.id };
            if tx.send(turn).await.is_err() {
                return Err("claude pipe closed".into());
            }
        }
    }
}
