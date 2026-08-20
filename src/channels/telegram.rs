use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{Mutex, mpsc};

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
    let _lock = acquire_instance_lock()?;
    let allowlist = load_allowlist();
    let session_id = ensure_session_id()?;
    write_pidfile()?;

    eprintln!("telegram: session_id={session_id}");
    eprintln!("telegram: allowlist={:?}", allowlist);

    let client = reqwest::Client::new();
    clear_webhook_for_polling(&client, &token).await?;

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

    tokio::spawn(typing_loop(client.clone(), token.clone(), typing_rx));
    telegram_reader(client, token, allowlist, to_claude_tx).await
}

// --- Config / state ---

fn load_token() -> Result<String, String> {
    if let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") {
        let token = token.trim();
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }

    let path = channel_dir().join(".env");
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    for line in contents.lines() {
        if let Some(v) = line.strip_prefix("TELEGRAM_BOT_TOKEN=") {
            let token = v.trim().trim_matches('"');
            if !token.is_empty() {
                return Ok(token.to_string());
            }
        }
    }
    Err(format!(
        "TELEGRAM_BOT_TOKEN not configured in env or {}",
        path.display()
    ))
}

fn load_allowlist() -> Option<HashSet<i64>> {
    let path = channel_dir().join("access.json");
    let contents = std::fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&contents).ok()?;
    let ids = v.get("allowFrom")?.as_array()?;
    let set: HashSet<i64> = ids
        .iter()
        .filter_map(|x| {
            x.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| x.as_i64())
        })
        .collect();
    if set.is_empty() { None } else { Some(set) }
}

fn channel_dir() -> PathBuf {
    paths::home().join("channels").join("telegram")
}

fn pidfile() -> PathBuf {
    channel_dir().join("pid")
}

fn lockfile() -> PathBuf {
    channel_dir().join("bridge.lock")
}

struct InstanceLock {
    _file: File,
}

fn acquire_instance_lock() -> Result<InstanceLock, String> {
    let dir = channel_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = lockfile();
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return Err(format!(
            "telegram bridge lock is already held at {}; another polling instance is running ({})",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    file.set_len(0)
        .map_err(|e| format!("truncate {}: {e}", path.display()))?;
    writeln!(file, "{}", std::process::id())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(InstanceLock { _file: file })
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
        return Err(format!(
            "kill({pid}, SIGTERM) failed: errno {}",
            std::io::Error::last_os_error()
        ));
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
    images: Vec<InlineImage>,
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
        // Same shape as discord.rs::feed_claude — a turn carrying images
        // becomes a content-block array instead of a bare string.
        let content_val = if turn.images.is_empty() {
            Value::String(turn.text)
        } else {
            let mut blocks: Vec<Value> = Vec::with_capacity(turn.images.len() + 1);
            blocks.push(json!({ "type": "text", "text": turn.text }));
            for img in turn.images {
                blocks.push(json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": img.media_type,
                        "data": img.data_b64,
                    }
                }));
            }
            Value::Array(blocks)
        };
        let payload = json!({
            "type": "user",
            "message": { "role": "user", "content": content_val }
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
async fn drain_claude(stdout: tokio::process::ChildStdout, typing_tx: mpsc::Sender<TypingCmd>) {
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
async fn typing_loop(client: reqwest::Client, token: String, mut rx: mpsc::Receiver<TypingCmd>) {
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
        eprintln!(
            "telegram: typing heartbeat POST failed chat={chat_id}: {}",
            e.without_url()
        );
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
    // Media messages carry their text in `caption`, never in `text`.
    caption: Option<String>,
    photo: Option<Vec<TgPhotoSize>>,
    document: Option<TgFile>,
    voice: Option<TgFile>,
    video: Option<TgFile>,
    audio: Option<TgFile>,
}

/// One resolution of a photo. Telegram sends the same image several times at
/// different sizes; the largest one is last.
#[derive(Deserialize)]
struct TgPhotoSize {
    file_id: String,
    file_size: Option<u64>,
}

/// Shared shape of `document` / `voice` / `video` / `audio`. Voice and video
/// notes have no `file_name`, so every field past `file_id` is optional.
#[derive(Deserialize)]
struct TgFile {
    file_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
    file_size: Option<u64>,
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

// --- Inbound media ---
//
// Telegram has no flat attachments array like Discord: media arrives in typed
// fields (`photo`, `document`, `voice`, `video`, `audio`) and is referenced by
// a `file_id` that has to be resolved through `getFile` before it can be
// downloaded. Images are inlined as base64 blocks on the turn; anything else
// is dumped to /tmp and surfaced as `attachment_file_path` so mimi can Read it
// — mirroring the conventions the discord bridge already established.

/// Claude API caps inline images at 5MB each.
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Cap for files pulled to /tmp, so a fat video can't swallow memory or disk.
/// (Telegram's own Bot API download limit is 20MB.)
const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

struct InlineImage {
    media_type: String,
    data_b64: String,
}

/// The single downloadable file attached to an inbound message.
struct Media {
    file_id: String,
    file_name: String,
    mime_type: String,
    file_size: Option<u64>,
}

fn claude_supported_image_mime(ct: &str) -> Option<&'static str> {
    match ct {
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        _ => None,
    }
}

fn ext_from_name(name: &str) -> Option<String> {
    let ext = name.rsplit_once('.')?.1;
    if !ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(ext.to_ascii_lowercase())
    } else {
        None
    }
}

fn ext_from_mime(mime: &str) -> String {
    match mime {
        "application/pdf" => "pdf".into(),
        "application/json" => "json".into(),
        "application/zip" => "zip".into(),
        "audio/ogg" => "ogg".into(),
        "audio/mpeg" => "mp3".into(),
        "video/mp4" => "mp4".into(),
        "text/markdown" => "md".into(),
        "text/csv" => "csv".into(),
        m if m.starts_with("text/") => "txt".into(),
        _ => "bin".into(),
    }
}

/// Pick the one file worth forwarding. Telegram splits media groups into one
/// message per file, so a message never carries more than one of these.
fn pick_media(msg: &TgMessage) -> Option<Media> {
    if let Some(sizes) = msg.photo.as_ref().filter(|s| !s.is_empty()) {
        // Biggest resolution that still fits the inline-image cap; if every
        // size is oversized, take the smallest and let the caller decide.
        let best = sizes
            .iter()
            .filter(|p| p.file_size.is_none_or(|s| s as usize <= MAX_IMAGE_BYTES))
            .max_by_key(|p| p.file_size.unwrap_or(0))
            .unwrap_or(&sizes[0]);
        return Some(Media {
            file_id: best.file_id.clone(),
            file_name: "photo.jpg".into(),
            mime_type: "image/jpeg".into(),
            file_size: best.file_size,
        });
    }
    let (file, fallback_name) = [
        (msg.document.as_ref(), "document"),
        (msg.voice.as_ref(), "voice.ogg"),
        (msg.video.as_ref(), "video.mp4"),
        (msg.audio.as_ref(), "audio"),
    ]
    .into_iter()
    .find_map(|(f, name)| f.map(|f| (f, name)))?;
    Some(Media {
        file_id: file.file_id.clone(),
        file_name: file
            .file_name
            .clone()
            .unwrap_or_else(|| fallback_name.to_string()),
        mime_type: file.mime_type.clone().unwrap_or_default(),
        file_size: file.file_size,
    })
}

/// Resolve a `file_id` through `getFile` and download the bytes. Returns the
/// bytes plus Telegram's own storage path (useful for an extension hint when
/// the message carried no filename).
async fn download_media(
    client: &reqwest::Client,
    token: &str,
    media: &Media,
) -> Result<(Vec<u8>, String), String> {
    if let Some(size) = media.file_size {
        if size as usize > MAX_ATTACHMENT_BYTES {
            return Err(format!(
                "{} is {size} bytes (> {MAX_ATTACHMENT_BYTES} cap)",
                media.file_name
            ));
        }
    }
    let url = format!(
        "https://api.telegram.org/bot{token}/getFile?file_id={}",
        media.file_id
    );
    let body: Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("getFile failed: {}", e.without_url()))?
        .json()
        .await
        .map_err(|e| format!("getFile parse failed: {}", e.without_url()))?;
    let file_path = body
        .pointer("/result/file_path")
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("getFile unexpected response: {body}"))?;
    let resp = client
        .get(format!(
            "https://api.telegram.org/file/bot{token}/{file_path}"
        ))
        .send()
        .await
        .map_err(|e| format!("download failed: {}", e.without_url()))?;
    if !resp.status().is_success() {
        return Err(format!("download status {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read bytes: {}", e.without_url()))?;
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "{} is {} bytes (> {MAX_ATTACHMENT_BYTES} cap after fetch)",
            media.file_name,
            bytes.len()
        ));
    }
    Ok((bytes.to_vec(), file_path.to_string()))
}

/// Download `media` and turn it into either an inline image block or a file on
/// disk. Failures are logged and swallowed — a broken attachment should never
/// cost the user their whole turn.
async fn resolve_media(
    client: &reqwest::Client,
    token: &str,
    media: &Media,
) -> (Vec<InlineImage>, Option<String>) {
    let (bytes, tg_path) = match download_media(client, token, media).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("telegram: media skipped: {e}");
            return (Vec::new(), None);
        }
    };

    // Oversized images fall through to the file path rather than being
    // dropped — mimi can still Read them off disk.
    if let Some(mime) =
        claude_supported_image_mime(&media.mime_type).filter(|_| bytes.len() <= MAX_IMAGE_BYTES)
    {
        eprintln!(
            "telegram: inlining image {} ({} bytes, {mime})",
            media.file_name,
            bytes.len()
        );
        return (
            vec![InlineImage {
                media_type: mime.to_string(),
                data_b64: BASE64_STANDARD.encode(&bytes),
            }],
            None,
        );
    }

    let ext = ext_from_name(&media.file_name)
        .or_else(|| ext_from_name(&tg_path))
        .unwrap_or_else(|| ext_from_mime(&media.mime_type));
    let path = format!("/tmp/mimi-attach-{}.{ext}", uuid::Uuid::new_v4());
    match tokio::fs::write(&path, &bytes).await {
        Ok(()) => {
            eprintln!(
                "telegram: saved attachment {} ({} bytes) -> {path}",
                media.file_name,
                bytes.len()
            );
            (Vec::new(), Some(path))
        }
        Err(e) => {
            eprintln!("telegram: failed writing attachment to {path}: {e}");
            (Vec::new(), None)
        }
    }
}

async fn clear_webhook_for_polling(client: &reqwest::Client, token: &str) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{token}/deleteWebhook");
    let resp = client
        .post(&url)
        .json(&json!({ "drop_pending_updates": false }))
        .send()
        .await
        .map_err(|e| format!("deleteWebhook request failed: {}", e.without_url()))?;
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("deleteWebhook parse failed: {}", e.without_url()))?;
    if body.get("ok").and_then(|x| x.as_bool()) == Some(true) {
        eprintln!("telegram: webhook cleared for long polling");
        Ok(())
    } else {
        Err(format!("deleteWebhook unexpected response: {body}"))
    }
}

async fn telegram_reader(
    client: reqwest::Client,
    token: String,
    allowlist: Option<HashSet<i64>>,
    tx: mpsc::Sender<UserTurn>,
) -> Result<(), String> {
    let offset = Arc::new(Mutex::new(0i64));
    let mut consecutive_conflicts = 0u32;
    loop {
        let off = *offset.lock().await;
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates?timeout={}&offset={}&allowed_updates=[\"message\"]",
            token, POLL_TIMEOUT_SECS, off
        );
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("telegram: getUpdates request failed: {}", e.without_url());
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
            Some(r) => {
                consecutive_conflicts = 0;
                r
            }
            None => {
                let wait = if body.get("error_code").and_then(|x| x.as_i64()) == Some(409) {
                    consecutive_conflicts = consecutive_conflicts.saturating_add(1);
                    eprintln!(
                        "telegram: getUpdates conflict ({consecutive_conflicts}/3): another poller owns this bot token"
                    );
                    if consecutive_conflicts >= 3 {
                        return Err(
                            "telegram getUpdates conflict after 3 consecutive attempts; another poller is running"
                                .into(),
                        );
                    }
                    35
                } else {
                    consecutive_conflicts = 0;
                    eprintln!("telegram: unexpected response: {body}");
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
            // Photos and files carry their text in `caption`, and a bare photo
            // has neither field set. Keying only off `text` meant every media
            // message was dropped on the floor — from the user's side mimi
            // simply never answered.
            let text = msg
                .text
                .clone()
                .or_else(|| msg.caption.clone())
                .unwrap_or_default();
            let media = pick_media(&msg);
            if text.is_empty() && media.is_none() {
                continue;
            }
            let from_id = msg.from.as_ref().map(|u| u.id).unwrap_or(0);
            if let Some(allow) = &allowlist
                && !allow.contains(&from_id)
            {
                eprintln!("telegram: blocked user {from_id}");
                continue;
            }
            let user_name = msg
                .from
                .as_ref()
                .and_then(|u| u.username.clone().or_else(|| u.first_name.clone()))
                .unwrap_or_default();
            let chat_type = msg.chat.chat_type.as_deref().unwrap_or("private");
            let chat_id_str = msg.chat.id.to_string();
            // Downloaded after the allowlist check so blocked users can't make
            // us pull files.
            let (images, attachment_file_path) = match &media {
                Some(m) => resolve_media(&client, &token, m).await,
                None => (Vec::new(), None),
            };
            let image_marker = if images.is_empty() {
                String::new()
            } else {
                "\n[image attachment included below]".to_string()
            };
            let attachment_marker = match attachment_file_path.as_deref() {
                Some(p) => format!("\n[attachment available at {p} — Read it for content]"),
                None => String::new(),
            };
            let attachment_attr = attachment_file_path
                .as_deref()
                .map(|p| format!(" attachment_file_path=\"{p}\""))
                .unwrap_or_default();
            let preamble =
                crate::context_buffer::preamble_for("telegram", &chat_id_str).unwrap_or_default();
            let time_ctx = crate::channels::time_context_preamble();
            let wrapped = format!(
                "{}{}{}<channel source=\"telegram\" chat_id=\"{}\" chat_type=\"{}\" user_id=\"{}\" user_name=\"{}\" message_id=\"{}\"{}>\n{}{}{}\n</channel>",
                time_ctx,
                OUTBOUND_PROTOCOL,
                preamble,
                msg.chat.id,
                chat_type,
                from_id,
                user_name,
                msg.message_id,
                attachment_attr,
                text,
                image_marker,
                attachment_marker
            );
            let tg_msg_id_str = msg.message_id.to_string();
            crate::context_buffer::append_user(
                "telegram",
                &chat_id_str,
                &user_name,
                &format!("{text}{image_marker}{attachment_marker}"),
                Some(&tg_msg_id_str),
            );
            eprintln!(
                "telegram: dispatch chat={} msg={} user={}",
                msg.chat.id, msg.message_id, from_id
            );
            let turn = UserTurn {
                text: wrapped,
                images,
                chat_id: msg.chat.id,
            };
            if tx.send(turn).await.is_err() {
                return Err("claude pipe closed".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(json: &str) -> TgMessage {
        serde_json::from_str(json).expect("message should deserialize")
    }

    #[test]
    fn photo_message_picks_largest_size_under_cap() {
        let m = msg(
            r#"{"message_id":7,"chat":{"id":42,"type":"private"},
                "caption":"look at this",
                "photo":[{"file_id":"small","file_size":1024},
                         {"file_id":"large","file_size":8192}]}"#,
        );
        assert_eq!(m.caption.as_deref(), Some("look at this"));
        let media = pick_media(&m).expect("photo should yield media");
        assert_eq!(media.file_id, "large");
        assert_eq!(media.mime_type, "image/jpeg");
        assert!(claude_supported_image_mime(&media.mime_type).is_some());
    }

    #[test]
    fn document_message_yields_attachment_with_extension() {
        let m = msg(
            r#"{"message_id":8,"chat":{"id":42},
                "document":{"file_id":"doc1","file_name":"notes.PDF",
                            "mime_type":"application/pdf","file_size":2048}}"#,
        );
        let media = pick_media(&m).expect("document should yield media");
        assert_eq!(media.file_id, "doc1");
        assert!(claude_supported_image_mime(&media.mime_type).is_none());
        assert_eq!(ext_from_name(&media.file_name).as_deref(), Some("pdf"));
    }

    #[test]
    fn voice_note_falls_back_to_mime_extension() {
        let m = msg(
            r#"{"message_id":9,"chat":{"id":42},
                "voice":{"file_id":"v1","mime_type":"audio/ogg","file_size":512}}"#,
        );
        let media = pick_media(&m).expect("voice should yield media");
        assert_eq!(
            ext_from_name(&media.file_name)
                .unwrap_or_else(|| ext_from_mime(&media.mime_type)),
            "ogg"
        );
    }

    #[test]
    fn plain_text_message_has_no_media() {
        let m = msg(r#"{"message_id":10,"chat":{"id":42},"text":"hi"}"#);
        assert!(pick_media(&m).is_none());
        assert_eq!(m.text.as_deref(), Some("hi"));
    }
}
