// Discord presence bridge — keeps a user account showing as "online" via a
// lone gateway connection + heartbeat, no message traffic. User tokens are
// technically a TOS-adjacent surface; this is the lowest-noise variant
// (identify + heartbeat only, no REST/API calls). The bridge is intended
// to be run under a dedicated systemd user service:
//
//     ExecStart=/usr/local/bin/mimi secret run discord_user_token \
//                 DISCORD_USER_TOKEN \
//                 /usr/local/bin/mimi channel start presence
//
// Token is injected via env var (pulled out of the encrypted vault by the
// `secret run` wrapper); the process itself never reads or writes the vault.
//
// On reconnect we re-read DISCORD_USER_TOKEN from the environment. systemd
// respawns the service on exit, so rotating the vault value + restarting
// the unit is enough to pick up a new token — no code change.

use std::path::PathBuf;
use std::time::Duration;

use chrono::{Datelike, NaiveTime, TimeZone, Weekday};
use chrono_tz::Tz;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::paths;

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const TOKEN_ENV: &str = "DISCORD_USER_TOKEN";

// User-token gateway IDENTIFY uses `capabilities` (bitmask of opt-in gateway
// features) rather than bot `intents`. 16381 is what the official web client
// currently sends; the exact value doesn't really matter for a read-free
// presence connection but matching a real client is less flaggable.
const CAPABILITIES: u64 = 16381;

// How often to re-check the schedule while the gateway is connected. If the
// active window just ended, we break out and disconnect. Also the poll
// cadence while offline (waiting for the next window to open).
const SCHEDULE_TICK: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
struct ScheduleFile {
    #[serde(default = "default_tz")]
    timezone: String,
    /// Lowercase weekday names, 3-letter or full. Defaults to Mon-Fri.
    #[serde(default = "default_days")]
    days: Vec<String>,
    /// List of {"start":"HH:MM","end":"HH:MM"} windows in the given tz.
    windows: Vec<WindowFile>,
}

#[derive(Debug, Deserialize)]
struct WindowFile {
    start: String,
    end: String,
}

fn default_tz() -> String { "America/Chicago".into() }
fn default_days() -> Vec<String> {
    vec!["mon","tue","wed","thu","fri"].into_iter().map(String::from).collect()
}

struct Schedule {
    tz: Tz,
    days: Vec<Weekday>,
    windows: Vec<(NaiveTime, NaiveTime)>,
}

impl Schedule {
    fn load() -> Option<Self> {
        let path = paths::channels_dir().join("presence").join("schedule.json");
        let text = std::fs::read_to_string(&path).ok()?;
        let file: ScheduleFile = serde_json::from_str(&text)
            .map_err(|e| eprintln!("presence: invalid schedule.json: {e}"))
            .ok()?;
        let tz: Tz = file.timezone.parse().ok()?;
        let days = file.days.iter().filter_map(|d| parse_weekday(d)).collect();
        let windows = file
            .windows
            .iter()
            .filter_map(|w| {
                Some((
                    NaiveTime::parse_from_str(&w.start, "%H:%M").ok()?,
                    NaiveTime::parse_from_str(&w.end, "%H:%M").ok()?,
                ))
            })
            .collect();
        Some(Schedule { tz, days, windows })
    }

    fn should_be_online(&self) -> bool {
        let now = self.tz.from_utc_datetime(&chrono::Utc::now().naive_utc());
        if !self.days.contains(&now.weekday()) {
            return false;
        }
        let t = now.time();
        self.windows.iter().any(|(s, e)| {
            // Handles normal windows; a window that wraps past midnight (end
            // <= start) is treated as "t >= start OR t < end" — unusual but
            // cheap to support.
            if e > s {
                t >= *s && t < *e
            } else {
                t >= *s || t < *e
            }
        })
    }
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    match s.to_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

pub async fn start() -> Result<(), String> {
    let token = std::env::var(TOKEN_ENV).map_err(|_| {
        format!(
            "${TOKEN_ENV} not set — run via `mimi secret run discord_user_token {TOKEN_ENV} mimi channel start presence`"
        )
    })?;
    if token.trim().is_empty() {
        return Err(format!("${TOKEN_ENV} is empty"));
    }

    write_pidfile()?;

    let schedule = Schedule::load();
    match &schedule {
        Some(s) => eprintln!(
            "presence: starting (token len={}, schedule: tz={} days={:?} windows={:?})",
            token.len(),
            s.tz.name(),
            s.days,
            s.windows,
        ),
        None => eprintln!(
            "presence: starting (token len={}, no schedule.json — always online)",
            token.len()
        ),
    }

    // Outer supervisor loop. When the schedule says "be online" we run the
    // gateway; the gateway itself also polls the schedule and bails when the
    // window closes, so this loop handles both cold-start waiting and
    // reconnects. 4004 (auth failure) still retries — a persistent 4004
    // means the vault token is stale and will stay bad until updated.
    loop {
        let should_online = schedule.as_ref().map(|s| s.should_be_online()).unwrap_or(true);
        if !should_online {
            // Outside any configured window; poll again soon.
            tokio::time::sleep(SCHEDULE_TICK).await;
            continue;
        }
        match run_gateway(&token, schedule.as_ref()).await {
            Ok(()) => eprintln!("presence: gateway closed — re-evaluating schedule"),
            Err(e) => {
                eprintln!("presence: gateway error: {e} — reconnecting in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

pub fn stop() -> Result<(), String> {
    let pidfile = pidfile_path();
    if !pidfile.exists() {
        return Err("presence: not running (no pidfile)".into());
    }
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .map_err(|e| format!("read pidfile: {e}"))?
        .trim()
        .parse()
        .map_err(|e| format!("parse pid: {e}"))?;
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    let _ = std::fs::remove_file(&pidfile);
    Ok(())
}

fn pidfile_path() -> PathBuf {
    paths::channels_dir().join("presence").join("pid")
}

fn write_pidfile() -> Result<(), String> {
    let path = pidfile_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    std::fs::write(&path, std::process::id().to_string())
        .map_err(|e| format!("write pid: {e}"))
}

async fn run_gateway(token: &str, schedule: Option<&Schedule>) -> Result<(), String> {
    let (ws, _) = connect_async(GATEWAY_URL)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let (mut write, mut read) = ws.split();

    // HELLO — carries the heartbeat cadence.
    let hello = read
        .next()
        .await
        .ok_or("gateway closed before HELLO")?
        .map_err(|e| format!("ws read: {e}"))?;
    let hello_text = match hello {
        Message::Text(t) => t.to_string(),
        _ => return Err("unexpected first gateway frame".into()),
    };
    let hello_json: Value =
        serde_json::from_str(&hello_text).map_err(|e| format!("parse hello: {e}"))?;
    let heartbeat_ms = hello_json
        .pointer("/d/heartbeat_interval")
        .and_then(|x| x.as_u64())
        .ok_or("no heartbeat_interval in HELLO")?;

    // IDENTIFY — user-token shape (browser-like properties + presence set
    // to online + capabilities bitmask). We do NOT request any intents; this
    // connection is purely for presence and doesn't need event delivery.
    let identify = json!({
        "op": 2,
        "d": {
            "token": token,
            "capabilities": CAPABILITIES,
            "properties": {
                "os": "Mac OS X",
                "browser": "Chrome",
                "device": "",
                "system_locale": "en-US",
                "browser_user_agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
                "browser_version": "128.0.0.0",
                "os_version": "10.15.7",
                "referrer": "",
                "referring_domain": "",
                "referrer_current": "",
                "referring_domain_current": "",
                "release_channel": "stable",
                "client_build_number": 342000,
                "client_event_source": null
            },
            "presence": {
                "status": "online",
                "since": 0,
                "activities": [],
                "afk": false
            },
            "compress": false
        }
    });
    write
        .send(Message::Text(identify.to_string().into()))
        .await
        .map_err(|e| format!("send identify: {e}"))?;

    let write = std::sync::Arc::new(tokio::sync::Mutex::new(write));

    // Heartbeat task.
    let hb_write = std::sync::Arc::clone(&write);
    let last_seq = std::sync::Arc::new(tokio::sync::RwLock::new(None::<u64>));
    let hb_seq = std::sync::Arc::clone(&last_seq);
    let hb_task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(heartbeat_ms));
        ticker.tick().await; // burn the immediate tick
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

    // Main loop — we only care about READY (success) and INVALID_SESSION /
    // close-with-4004 (auth failure). Everything else is ignored. A separate
    // ticker polls the schedule and bails (clean exit) when the configured
    // window closes, so the outer supervisor can go back to sleep.
    let mut sched_tick = tokio::time::interval(SCHEDULE_TICK);
    sched_tick.tick().await; // burn immediate
    loop {
        tokio::select! {
            _ = sched_tick.tick() => {
                if let Some(s) = schedule {
                    if !s.should_be_online() {
                        eprintln!("presence: schedule window ended — disconnecting");
                        hb_task.abort();
                        return Ok(());
                    }
                }
            }
            msg = read.next() => {
                let msg = match msg {
                    Some(m) => m.map_err(|e| format!("ws read: {e}"))?,
                    None => {
                        hb_task.abort();
                        return Err("gateway stream ended".into());
                    }
                };
                let text = match msg {
                    Message::Text(t) => t.to_string(),
                    Message::Close(frame) => {
                        hb_task.abort();
                        let code = frame.as_ref().map(|f| u16::from(f.code));
                        return Err(format!("gateway closed, code={:?}", code));
                    }
                    _ => continue,
                };
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(s) = v.get("s").and_then(|x| x.as_u64()) {
                    *last_seq.write().await = Some(s);
                }
                let op = v.get("op").and_then(|x| x.as_u64()).unwrap_or(0);
                if op == 9 {
                    hb_task.abort();
                    return Err("INVALID_SESSION (op=9) — token likely expired".into());
                }
                if op != 0 { continue; }
                let event = v.get("t").and_then(|x| x.as_str()).unwrap_or("");
                if event == "READY" {
                    let username = v.pointer("/d/user/username").and_then(|x| x.as_str()).unwrap_or("?");
                    let user_id = v.pointer("/d/user/id").and_then(|x| x.as_str()).unwrap_or("?");
                    eprintln!("presence: READY — logged in as {username} ({user_id}), status=online");
                }
            }
        }
    }
}
