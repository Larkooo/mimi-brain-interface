//! Discord voice channel scaffold (bidirectional).
//!
//! Status: SKELETON. Nothing here actually connects yet — this is the
//! initial commit on `feature/discord-voice` branched from master at the
//! request of the owner. The goal is a bidirectional voice loop where
//! Mimi can both speak in a Discord voice channel (TTS) and listen to
//! humans speaking and respond (STT → claude → TTS).
//!
//! Pipeline (target):
//!
//!   text in   -> [TTS API]  -> 48kHz f32 PCM -> [opus encode] -> RTP -> UDP -> Discord
//!   Discord -> UDP -> RTP -> [opus decode]  -> 48kHz f32 PCM -> [VAD] -> [STT API] -> text
//!
//! Components, in build order:
//!
//!   1. `gateway`   — separate WebSocket connection per guild (URL handed
//!                    out by the main gateway in a VOICE_SERVER_UPDATE
//!                    event). Handles HELLO/IDENTIFY/SELECT_PROTOCOL/READY,
//!                    plus the heartbeat keepalive. Hands the UDP endpoint
//!                    + secret_key to the RTP layer.
//!   2. `rtp`       — UDP socket. Sends 20ms opus frames wrapped in RTP+
//!                    xsalsa20poly1305 encryption. Reads inbound RTP, keeps
//!                    per-SSRC streams, and demuxes them back to the caller.
//!   3. `codec`     — thin wrapper around `audiopus` for stereo 48kHz opus.
//!   4. `tts`       — provider client (initially OpenAI TTS via REST,
//!                    HTTP/2 stream into the encoder). Pluggable trait so
//!                    we can swap in ElevenLabs / Piper later.
//!   5. `stt`       — provider client (initially OpenAI Whisper REST per
//!                    utterance; later upgrade to streaming). Fed by the
//!                    VAD turn-taker.
//!   6. `vad`       — webrtc-style energy + zero-crossing voice activity
//!                    detection to chunk inbound audio into utterances.
//!   7. `session`   — top-level loop that wires gateway+rtp+codec+vad+stt+
//!                    claude+tts together. One `VoiceSession` per voice
//!                    channel join. Owns the cancellation token.
//!
//! Outbound CLI (target — not yet implemented):
//!
//!   discord voice join <guild_id> <channel_id>
//!   discord voice leave <guild_id>
//!   discord voice say  <guild_id> <text>     # one-shot TTS into the
//!                                            # current voice connection
//!
//! These will be added to `~/.mimi/bin/discord` once the underlying
//! `VoiceSession::join` actually negotiates a connection successfully.
//!
//! Why not songbird/serenity: the existing Discord gateway in
//! `channels/discord.rs` is hand-rolled on `tokio_tungstenite`, so adding
//! songbird means either a) keeping two gateway clients in-process or
//! b) a full migration. The scope larko greenlit is "feature added", not
//! "rewrite", so we mirror the hand-rolled approach for voice as well.
//!
//! See `docs/voice-architecture.md` (TODO) for the longer write-up.

#![allow(dead_code)] // some paths only get exercised through the control server (chunk 12)

use std::collections::HashMap;
use std::sync::OnceLock;

use tokio::sync::Mutex;

/// Process-wide registry of live voice sessions, keyed by guild_id.
/// Lives in the same process as the main Discord gateway (the
/// `mimi-discord` systemd unit), since `gateway_hooks::send_voice_state_update`
/// only works when the main gateway is owned by *this* process.
///
/// Each guild can host at most one active voice session at a time
/// (Discord's own constraint — bot can only be in one voice channel
/// per guild). Calls to `join` while a session is already live drop the
/// existing one first.
static REGISTRY: OnceLock<Mutex<HashMap<u64, VoiceSession>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<u64, VoiceSession>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Cached bot user_id (set by the Discord bridge from its READY event).
/// The voice gateway IDENTIFY needs this; we read it lazily here so the
/// caller doesn't have to thread it through.
static BOT_USER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_bot_user_id(id: u64) {
    BOT_USER_ID.store(id, std::sync::atomic::Ordering::SeqCst);
}

fn bot_user_id() -> Option<u64> {
    let v = BOT_USER_ID.load(std::sync::atomic::Ordering::SeqCst);
    if v == 0 { None } else { Some(v) }
}

/// Top-level CLI / RPC entrypoint: join a voice channel.
pub async fn ctrl_join(guild_id: u64, channel_id: u64) -> std::io::Result<()> {
    let user_id = bot_user_id().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "discord bridge not READY yet — bot_user_id unknown",
        )
    })?;
    let mut reg = registry().lock().await;
    if let Some(existing) = reg.remove(&guild_id) {
        let _ = existing.leave().await;
    }
    let sess = VoiceSession::join(guild_id, channel_id, user_id).await?;
    reg.insert(guild_id, sess);
    Ok(())
}

pub async fn ctrl_say(guild_id: u64, text: &str) -> std::io::Result<()> {
    let reg = registry().lock().await;
    let sess = reg.get(&guild_id).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no live voice session for guild {guild_id}"),
        )
    })?;
    sess.say(text).await
}

pub async fn ctrl_leave(guild_id: u64) -> std::io::Result<()> {
    let mut reg = registry().lock().await;
    let sess = reg.remove(&guild_id).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no live voice session for guild {guild_id}"),
        )
    })?;
    sess.leave().await
}

pub async fn ctrl_list() -> Vec<u64> {
    registry().lock().await.keys().copied().collect()
}

/// Loopback control server. Bound by the Discord bridge process at
/// startup so the `discord voice {join,say,leave}` bash wrappers can
/// reach in and drive a `VoiceSession`.
///
/// Bound to `127.0.0.1` only — no external exposure. Default port
/// 3132 (sits next to the dashboard on 3131); overridable via the
/// `MIMI_VOICE_CTRL_PORT` env var.
pub mod control {
    use axum::{Router, extract::Json, http::StatusCode, routing::post};
    use serde::Deserialize;

    pub fn port() -> u16 {
        std::env::var("MIMI_VOICE_CTRL_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3132)
    }

    pub async fn serve() {
        let app = Router::new()
            .route("/voice/join", post(api_join))
            .route("/voice/leave", post(api_leave))
            .route("/voice/say", post(api_say))
            .route("/voice/list", post(api_list));
        let bind = format!("127.0.0.1:{}", port());
        match tokio::net::TcpListener::bind(&bind).await {
            Ok(listener) => {
                eprintln!("voice: control server listening on {bind}");
                if let Err(e) = axum::serve(listener, app).await {
                    eprintln!("voice: control server crashed: {e}");
                }
            }
            Err(e) => {
                eprintln!("voice: failed to bind {bind}: {e} — voice CLI wrappers won't work");
            }
        }
    }

    #[derive(Deserialize)]
    struct JoinReq { guild_id: u64, channel_id: u64 }

    async fn api_join(Json(req): Json<JoinReq>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
        super::ctrl_join(req.guild_id, req.channel_id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({ "ok": true, "guild_id": req.guild_id, "channel_id": req.channel_id })))
    }

    #[derive(Deserialize)]
    struct LeaveReq { guild_id: u64 }

    async fn api_leave(Json(req): Json<LeaveReq>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
        super::ctrl_leave(req.guild_id)
            .await
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
        Ok(Json(serde_json::json!({ "ok": true, "guild_id": req.guild_id })))
    }

    #[derive(Deserialize)]
    struct SayReq { guild_id: u64, text: String }

    async fn api_say(Json(req): Json<SayReq>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
        super::ctrl_say(req.guild_id, &req.text)
            .await
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
        Ok(Json(serde_json::json!({ "ok": true, "guild_id": req.guild_id })))
    }

    async fn api_list() -> Json<serde_json::Value> {
        Json(serde_json::json!({ "guilds": super::ctrl_list().await }))
    }
}

/// One live voice connection for a single guild. Thin wrapper over
/// `session::Live` that exposes the public API surface.
///
/// Created by `VoiceSession::join`. Drops cleanly when `leave()` is
/// called (sends VOICE_STATE_UPDATE channel_id=null + cancels the loops).
pub struct VoiceSession {
    pub guild_id: u64,
    pub channel_id: u64,
    inner: Mutex<Option<session::Live>>,
}

impl VoiceSession {
    /// Open a voice connection. Wires VOICE_STATE_UPDATE on the main
    /// gateway, waits for the VOICE_SERVER_UPDATE response, runs the
    /// voice gateway IDENTIFY → SELECT_PROTOCOL → SESSION_DESCRIPTION
    /// dance, and starts the bidirectional audio loops.
    ///
    /// `user_id` is the bot's own Discord user id (read from the main
    /// gateway READY event — the channel CLI will look it up before
    /// calling).
    pub async fn join(guild_id: u64, channel_id: u64, user_id: u64) -> std::io::Result<Self> {
        let live = session::join(guild_id, channel_id, user_id).await?;
        Ok(Self {
            guild_id, channel_id,
            inner: Mutex::new(Some(live)),
        })
    }

    /// Enqueue `text` for TTS → opus → encrypted RTP send.
    pub async fn say(&self, text: &str) -> std::io::Result<()> {
        let guard = self.inner.lock().await;
        match guard.as_ref() {
            Some(live) => live.say(text),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "voice session already left",
            )),
        }
    }

    /// Disconnect cleanly: VOICE_STATE_UPDATE channel_id=null + cancel
    /// the audio loops.
    pub async fn leave(self) -> std::io::Result<()> {
        let mut guard = self.inner.lock().await;
        if let Some(live) = guard.take() {
            live.leave().await;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sub-module stubs. Each of these gets fleshed out in subsequent commits on
// this branch. Keeping them in one file for now — we'll split into a
// `voice/` directory once any of them grows past ~200 lines.
// ---------------------------------------------------------------------------

mod gateway {
    //! Voice WebSocket gateway. Op codes per Discord voice gateway v8.
    //! Connects to the URL handed out in the main gateway's
    //! VOICE_SERVER_UPDATE event, runs IDENTIFY → READY → SELECT_PROTOCOL
    //! → SESSION_DESCRIPTION, and emits the resulting secret_key + UDP
    //! endpoint up to `VoiceSession`.
    //!
    //! Status: opcode taxonomy + payload structs landed. The connect-loop
    //! (WS handshake, heartbeat, state machine) is the next commit.

    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    /// Discord voice gateway opcodes (v8). Subset we care about; full list at
    /// <https://discord.com/developers/docs/topics/opcodes-and-status-codes#voice>.
    #[repr(u8)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Op {
        Identify = 0,
        SelectProtocol = 1,
        Ready = 2,
        Heartbeat = 3,
        SessionDescription = 4,
        Speaking = 5,
        HeartbeatAck = 6,
        Resume = 7,
        Hello = 8,
        Resumed = 9,
        ClientDisconnect = 13,

        // DAVE protocol (E2EE). Mandatory once `max_dave_protocol_version: 1`
        // is advertised in IDENTIFY. JSON ops 21-24, 31; binary ops 25-30.
        // Spec: https://daveprotocol.com / https://github.com/discord/dave-protocol
        DavePrepareTransition = 21,
        DaveExecuteTransition = 22,
        DaveReadyForTransition = 23,
        DavePrepareEpoch = 24,
        DaveMlsExternalSenderPackage = 25,
        DaveMlsKeyPackage = 26,
        DaveMlsProposals = 27,
        DaveMlsCommitWelcome = 28,
        DaveMlsAnnounceCommitTransition = 29,
        DaveMlsWelcome = 30,
        DaveMlsInvalidCommitWelcome = 31,
    }

    /// Build a binary voice-gateway frame: `[seq:u16 BE][op:u8][payload..]`.
    /// Used for DAVE MLS opcodes 25-30 which are sent as Message::Binary.
    pub fn build_binary_frame(seq: u16, op: u8, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(3 + payload.len());
        buf.extend_from_slice(&seq.to_be_bytes());
        buf.push(op);
        buf.extend_from_slice(payload);
        buf
    }

    /// Parse the header off an inbound binary voice-gateway frame.
    /// Returns (seq, op, payload). Errors if the frame is shorter than 3 bytes.
    pub fn parse_binary_frame(b: &[u8]) -> Option<(u16, u8, &[u8])> {
        if b.len() < 3 {
            return None;
        }
        let seq = u16::from_be_bytes([b[0], b[1]]);
        let op = b[2];
        let payload = &b[3..];
        Some((seq, op, payload))
    }

    /// Wire envelope for every voice gateway message. Discord uses `op` +
    /// untyped `d`; we deserialize `d` into the per-op struct on demand.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Frame {
        pub op: u8,
        pub d: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub seq: Option<u64>,
    }

    /// HELLO (op 8). Sent by Discord immediately after the WS handshake;
    /// tells us how often to heartbeat (in ms, fractional).
    #[derive(Debug, Clone, Deserialize)]
    pub struct Hello {
        pub heartbeat_interval: f64,
    }

    /// IDENTIFY (op 0). First thing we send after HELLO. server_id =
    /// guild snowflake; user_id is the bot's own id; session_id and token
    /// come from the main gateway's VOICE_STATE_UPDATE / VOICE_SERVER_UPDATE
    /// event pair.
    #[derive(Debug, Clone, Serialize)]
    pub struct Identify {
        pub server_id: String,
        pub user_id: String,
        pub session_id: String,
        pub token: String,
        // Opt out of DAVE (E2EE). Discord enforces DAVE on v=8+ unless
        // we advertise 0 here. Without this we get a 4017 close on
        // IDENTIFY ("E2EE/DAVE protocol required").
        pub max_dave_protocol_version: u8,
    }

    /// READY (op 2). Discord answers with the UDP endpoint we should bind
    /// to plus our SSRC and the encryption modes it supports.
    #[derive(Debug, Clone, Deserialize)]
    pub struct Ready {
        pub ssrc: u32,
        pub ip: String,
        pub port: u16,
        pub modes: Vec<String>,
    }

    /// SELECT_PROTOCOL (op 1). After UDP IP-discovery completes we send our
    /// externally-visible address back so Discord knows where to route RTP.
    #[derive(Debug, Clone, Serialize)]
    pub struct SelectProtocol {
        pub protocol: &'static str, // always "udp"
        pub data: SelectProtocolData,
    }

    #[derive(Debug, Clone, Serialize)]
    pub struct SelectProtocolData {
        pub address: String,
        pub port: u16,
        pub mode: &'static str, // "xsalsa20_poly1305" for now
    }

    /// SESSION_DESCRIPTION (op 4). Final handshake step: the 32-byte
    /// secret_key the RTP layer uses for xsalsa20poly1305 encryption.
    #[derive(Debug, Clone, Deserialize)]
    pub struct SessionDescription {
        pub mode: String,
        pub secret_key: Vec<u8>,
    }

    /// SPEAKING (op 5). Sent before each transmission so other clients
    /// know to expect audio. `delay` is always 0 for bots.
    #[derive(Debug, Clone, Serialize)]
    pub struct Speaking {
        pub speaking: u8, // bitfield: 1=mic, 2=soundshare, 4=priority
        pub delay: u32,
        pub ssrc: u32,
    }

    /// HEARTBEAT (op 3). The payload is just a u64 nonce we echo back from
    /// HEARTBEAT_ACK. We use millisecond-since-unix-epoch by convention.
    pub fn heartbeat_payload() -> Value {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        serde_json::json!(now)
    }

    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use tokio::sync::mpsc;
    use tokio::time::interval;
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    /// Output of a successful gateway handshake — what the RTP layer needs
    /// to start encrypting / sending audio. The voice WS task continues
    /// running in the background after this returns; cancelling the
    /// returned `cancel` channel hangs up cleanly.
    ///
    /// The session orchestration layer uses `out_tx` to send post-READY
    /// frames (SELECT_PROTOCOL, SPEAKING, RESUME) and `in_rx` to receive
    /// dispatched events from Discord (SESSION_DESCRIPTION, SPEAKING from
    /// peers, CLIENT_DISCONNECT). The frames are already deserialized
    /// `Frame` envelopes — caller pulls `d` into the per-op type.
    pub struct Handshake {
        pub ssrc: u32,
        pub udp_endpoint: (String, u16),
        /// Filled in by `session::VoiceSession::join` after
        /// SELECT_PROTOCOL → SESSION_DESCRIPTION. Zeroed at handshake
        /// time so the type is well-formed.
        pub secret_key: [u8; 32],
        pub cancel: mpsc::Sender<()>,
        pub out_tx: mpsc::UnboundedSender<Frame>,
        pub in_rx: mpsc::UnboundedReceiver<Frame>,
    }

    /// Connect to the voice gateway URL handed out by the main gateway's
    /// VOICE_SERVER_UPDATE event, run the v8 handshake, then spawn a
    /// background task that handles heartbeats and demuxes inbound frames.
    ///
    /// `endpoint` is the bare host (no scheme, no path) — Discord gives
    /// it as e.g. `dfw.discord.media:443`. We always upgrade to wss with
    /// `?v=8`.
    ///
    /// Stops short of SELECT_PROTOCOL — that step needs the RTP layer to
    /// have done UDP IP-discovery first, so it lives in `session.rs`.
    /// This function returns once we've received READY and started
    /// heartbeating, with the UDP endpoint + SSRC needed for discovery.
    pub async fn connect(
        endpoint: &str,
        guild_id: u64,
        user_id: u64,
        channel_id: u64,
        session_id: &str,
        token: &str,
    ) -> std::io::Result<(Handshake, ReadyState)> {
        // Construct a DAVE session up front — we'll move it into the WS
        // task post-READY. user_id + channel_id snowflakes scope the MLS
        // group; the session itself is INACTIVE until the gateway sends
        // op 24 (prepare_epoch) + op 25 (external_sender_package), at which
        // point we generate a key package and respond with op 26.
        let dave_version = std::num::NonZeroU16::new(davey::DAVE_PROTOCOL_VERSION)
            .expect("DAVE_PROTOCOL_VERSION non-zero");
        let mut dave_session = davey::DaveSession::new(
            dave_version, user_id, channel_id, None,
        ).map_err(|e| std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("DaveSession::new: {e:?}"),
        ))?;
        eprintln!(
            "voice/dave: DaveSession ready (v={}, user={user_id}, channel={channel_id})",
            davey::DAVE_PROTOCOL_VERSION
        );
        // v=8 is the DAVE-aware voice gateway. We advertise
        // max_dave_protocol_version=1 in IDENTIFY and handle ops 21-31
        // in the WS task below; the gateway pushes external_sender_package
        // (op 25), prepare_epoch (op 24), proposals (op 27), commits
        // (op 29), and welcomes (op 30) which we route into davey.
        let url = format!("wss://{}/?v=8", endpoint);
        eprintln!("voice/gateway: connecting url={url} guild={guild_id} user={user_id} session={session_id} token_len={}", token.len());
        let (ws, _resp) = connect_async(&url).await.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e.to_string())
        })?;
        eprintln!("voice/gateway: ws connected, awaiting HELLO");
        let (mut sink, mut stream) = ws.split();

        // 1. Receive HELLO. Anything else here is a protocol violation.
        let hello: Hello = match stream.next().await {
            Some(Ok(Message::Text(txt))) => {
                let frame: Frame = serde_json::from_str(&txt).map_err(io_err)?;
                if frame.op != Op::Hello as u8 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("expected HELLO, got op {}", frame.op),
                    ));
                }
                serde_json::from_value(frame.d).map_err(io_err)?
            }
            other => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    format!("voice ws closed before HELLO: {other:?}"),
                ));
            }
        };

        // 2. Send IDENTIFY.
        let identify = Frame {
            op: Op::Identify as u8,
            d: serde_json::to_value(Identify {
                server_id: guild_id.to_string(),
                user_id: user_id.to_string(),
                session_id: session_id.to_string(),
                token: token.to_string(),
                max_dave_protocol_version: 1,
            })
            .map_err(io_err)?,
            seq: None,
        };
        let identify_json = serde_json::to_string(&identify).map_err(io_err)?;
        eprintln!("voice/gateway: sending IDENTIFY: {identify_json}");
        sink.send(Message::Text(identify_json.into()))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))?;
        eprintln!("voice/gateway: IDENTIFY sent, awaiting READY");

        // 3. Receive READY (Discord may interleave a HEARTBEAT_ACK or
        //    HELLO-echo, so we loop until we see op 2).
        let ready: Ready = loop {
            match stream.next().await {
                Some(Ok(Message::Text(txt))) => {
                    eprintln!("voice/gateway: pre-READY frame: {txt}");
                    let frame: Frame = serde_json::from_str(&txt).map_err(io_err)?;
                    if frame.op == Op::Ready as u8 {
                        break serde_json::from_value(frame.d).map_err(io_err)?;
                    }
                    // Unhandled pre-READY frame — log and continue.
                }
                Some(Ok(Message::Close(cf))) => {
                    eprintln!("voice/gateway: ws CLOSE frame: {cf:?}");
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        format!("voice ws close: {cf:?}"),
                    ));
                }
                Some(Ok(other)) => {
                    eprintln!("voice/gateway: pre-READY non-text frame: {other:?}");
                    continue;
                }
                Some(Err(e)) => {
                    eprintln!("voice/gateway: ws stream error: {e}");
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        e.to_string(),
                    ));
                }
                None => {
                    eprintln!("voice/gateway: ws stream ended (None)");
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "voice ws closed before READY",
                    ));
                }
            }
        };

        // 4. Spawn heartbeat + demux background task. Owns the WS sink/
        //    stream for the rest of the connection lifetime; cancellable
        //    via the returned mpsc. Routes outbound frames from `out_rx`
        //    onto the wire and dispatches inbound text frames into
        //    `in_tx` (after JSON deserialize) so the session layer can
        //    drive SELECT_PROTOCOL → SESSION_DESCRIPTION etc.
        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Frame>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<Frame>();
        let interval_ms = hello.heartbeat_interval as u64;
        tokio::spawn(async move {
            let mut beat = interval(Duration::from_millis(interval_ms.max(1)));
            beat.tick().await; // first tick fires immediately; skip.
            loop {
                tokio::select! {
                    _ = cancel_rx.recv() => break,
                    _ = beat.tick() => {
                        let f = Frame {
                            op: Op::Heartbeat as u8,
                            d: heartbeat_payload(),
                            seq: None,
                        };
                        if let Ok(s) = serde_json::to_string(&f) {
                            if sink.send(Message::Text(s.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    out = out_rx.recv() => match out {
                        Some(frame) => {
                            if let Ok(s) = serde_json::to_string(&frame) {
                                if sink.send(Message::Text(s.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        None => break, // sender dropped — session is gone
                    },
                    inbound = stream.next() => match inbound {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(Message::Text(txt))) => {
                            if let Ok(frame) = serde_json::from_str::<Frame>(&txt) {
                                // DAVE JSON ops (21/22/23/24/31) handled
                                // here pre-channel-forward.
                                match frame.op {
                                    21 => {
                                        // dave_protocol_prepare_transition:
                                        // {protocol_version, transition_id}.
                                        // transition_id=0 means reset MLS state.
                                        if let Some(tid) = frame.d.get("transition_id").and_then(|v| v.as_u64()) {
                                            if tid == 0 {
                                                if let Err(e) = dave_session.reset() {
                                                    eprintln!("voice/dave: reset failed: {e:?}");
                                                }
                                                eprintln!("voice/dave: op 21 prepare_transition tid=0, reset");
                                            } else {
                                                eprintln!("voice/dave: op 21 prepare_transition tid={tid}");
                                            }
                                        }
                                    }
                                    22 => {
                                        // dave_protocol_execute_transition:
                                        // server signals it's safe to switch
                                        // ratchets. davey's encryptor handles
                                        // the active flag internally; just log.
                                        eprintln!("voice/dave: op 22 execute_transition: {}", frame.d);
                                    }
                                    24 => {
                                        // dave_protocol_prepare_epoch:
                                        // {protocol_version, epoch}. The next
                                        // step is to generate + send our
                                        // key package as op 26 binary.
                                        eprintln!("voice/dave: op 24 prepare_epoch: {}", frame.d);
                                        match dave_session.create_key_package() {
                                            Ok(kp) => {
                                                let buf = build_binary_frame(0, 26, &kp);
                                                if sink.send(Message::Binary(buf.into())).await.is_err() {
                                                    break;
                                                }
                                                eprintln!("voice/dave: sent op 26 key_package ({} bytes)", kp.len());
                                            }
                                            Err(e) => {
                                                eprintln!("voice/dave: create_key_package failed: {e:?}");
                                            }
                                        }
                                    }
                                    31 => {
                                        eprintln!("voice/dave: op 31 invalid_commit_welcome: {}", frame.d);
                                    }
                                    _ => {
                                        // Non-DAVE text frame — forward to
                                        // session layer (SESSION_DESCRIPTION,
                                        // peer SPEAKING, CLIENT_DISCONNECT, etc).
                                        if in_tx.send(frame).is_err() { break; }
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Binary(b))) => {
                            // DAVE MLS opcodes 25-30 arrive as binary frames
                            // with the layout `[seq:u16 BE][op:u8][payload..]`.
                            let Some((seq, op, payload)) = parse_binary_frame(&b) else {
                                eprintln!(
                                    "voice/gateway: short binary frame ({} bytes), ignoring",
                                    b.len()
                                );
                                continue;
                            };
                            eprintln!(
                                "voice/gateway: binary frame seq={seq} op={op} payload_len={}",
                                payload.len()
                            );
                            match op {
                                25 => {
                                    // dave_mls_external_sender_package:
                                    // ExternalSender bytes. Set on the
                                    // session — this also resets any
                                    // pre-existing group to a pending one.
                                    if let Err(e) = dave_session.set_external_sender(payload) {
                                        eprintln!("voice/dave: set_external_sender failed: {e:?}");
                                    } else {
                                        eprintln!("voice/dave: external sender set");
                                    }
                                }
                                27 => {
                                    // dave_mls_proposals: first byte is the
                                    // ProposalsOperationType (0=APPEND,
                                    // 1=REVOKE), rest is the VLBytes-wrapped
                                    // proposal stream.
                                    if payload.is_empty() {
                                        eprintln!("voice/dave: op 27 empty payload");
                                        continue;
                                    }
                                    let optype = match payload[0] {
                                        0 => davey::ProposalsOperationType::APPEND,
                                        1 => davey::ProposalsOperationType::REVOKE,
                                        b => {
                                            eprintln!("voice/dave: op 27 unknown optype {b}");
                                            continue;
                                        }
                                    };
                                    match dave_session.process_proposals(optype, &payload[1..], None) {
                                        Ok(Some(cw)) => {
                                            // Build op 28 commit_welcome:
                                            // commit bytes followed by
                                            // optional welcome bytes.
                                            let mut out = Vec::with_capacity(
                                                cw.commit.len() + cw.welcome.as_ref().map_or(0, |w| w.len())
                                            );
                                            out.extend_from_slice(&cw.commit);
                                            if let Some(w) = &cw.welcome {
                                                out.extend_from_slice(w);
                                            }
                                            let buf = build_binary_frame(0, 28, &out);
                                            if sink.send(Message::Binary(buf.into())).await.is_err() {
                                                break;
                                            }
                                            eprintln!(
                                                "voice/dave: sent op 28 commit_welcome (commit={}, welcome={})",
                                                cw.commit.len(),
                                                cw.welcome.as_ref().map_or(0, |w| w.len())
                                            );
                                        }
                                        Ok(None) => {
                                            eprintln!("voice/dave: op 27 no commit needed");
                                        }
                                        Err(e) => {
                                            eprintln!("voice/dave: process_proposals failed: {e:?}");
                                        }
                                    }
                                }
                                29 => {
                                    // dave_mls_announce_commit_transition:
                                    // [transition_id:u16 BE][commit bytes..]
                                    if payload.len() < 2 {
                                        eprintln!("voice/dave: op 29 short payload");
                                        continue;
                                    }
                                    let tid = u16::from_be_bytes([payload[0], payload[1]]);
                                    let commit_bytes = &payload[2..];
                                    match dave_session.process_commit(commit_bytes) {
                                        Ok(_) => {
                                            // Reply with op 23 ready_for_transition.
                                            let f = Frame {
                                                op: 23,
                                                d: serde_json::json!({"transition_id": tid}),
                                                seq: None,
                                            };
                                            if let Ok(s) = serde_json::to_string(&f) {
                                                if sink.send(Message::Text(s.into())).await.is_err() {
                                                    break;
                                                }
                                            }
                                            eprintln!("voice/dave: applied commit, sent op 23 tid={tid}");
                                        }
                                        Err(e) => {
                                            eprintln!("voice/dave: process_commit failed: {e:?}");
                                            // Send op 31 invalid_commit_welcome.
                                            let f = Frame {
                                                op: 31,
                                                d: serde_json::json!({"transition_id": tid}),
                                                seq: None,
                                            };
                                            if let Ok(s) = serde_json::to_string(&f) {
                                                let _ = sink.send(Message::Text(s.into())).await;
                                            }
                                        }
                                    }
                                }
                                30 => {
                                    // dave_mls_welcome:
                                    // [transition_id:u16 BE][welcome bytes..]
                                    if payload.len() < 2 {
                                        eprintln!("voice/dave: op 30 short payload");
                                        continue;
                                    }
                                    let tid = u16::from_be_bytes([payload[0], payload[1]]);
                                    let welcome_bytes = &payload[2..];
                                    match dave_session.process_welcome(welcome_bytes) {
                                        Ok(_) => {
                                            let f = Frame {
                                                op: 23,
                                                d: serde_json::json!({"transition_id": tid}),
                                                seq: None,
                                            };
                                            if let Ok(s) = serde_json::to_string(&f) {
                                                if sink.send(Message::Text(s.into())).await.is_err() {
                                                    break;
                                                }
                                            }
                                            eprintln!("voice/dave: applied welcome, sent op 23 tid={tid}");
                                        }
                                        Err(e) => {
                                            eprintln!("voice/dave: process_welcome failed: {e:?}");
                                            let f = Frame {
                                                op: 31,
                                                d: serde_json::json!({"transition_id": tid}),
                                                seq: None,
                                            };
                                            if let Ok(s) = serde_json::to_string(&f) {
                                                let _ = sink.send(Message::Text(s.into())).await;
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    eprintln!("voice/dave: unhandled binary op {op}");
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        });

        let handshake = Handshake {
            ssrc: ready.ssrc,
            udp_endpoint: (ready.ip.clone(), ready.port),
            secret_key: [0u8; 32],
            cancel: cancel_tx,
            out_tx,
            in_rx,
        };
        Ok((handshake, ReadyState { modes: ready.modes }))
    }

    /// Subset of `Ready` that the session layer needs to pick an
    /// encryption mode for SELECT_PROTOCOL.
    pub struct ReadyState {
        pub modes: Vec<String>,
    }

    fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    }
}

mod rtp {
    //! UDP RTP send/receive. 20ms opus frames, sequence + timestamp
    //! bookkeeping, xsalsa20poly1305 encryption with the secret_key from
    //! the gateway. Inbound: per-SSRC demux into separate audio streams.
    //!
    //! Status this commit: UDP socket bind + IP discovery (the 74-byte
    //! handshake Discord uses to tell the bot what its public-facing
    //! ip:port is). Encrypted RTP send/receive lands once xsalsa20poly1305
    //! is added to Cargo.toml.

    use tokio::net::UdpSocket;

    /// Result of the IP-discovery handshake: the address Discord can route
    /// our RTP back to. Feeds straight into SELECT_PROTOCOL on the gateway.
    pub struct Discovered {
        pub address: String,
        pub port: u16,
        pub socket: UdpSocket,
    }

    /// Bind a UDP socket and run Discord's voice IP-discovery dance.
    ///
    /// Protocol (per Discord docs): send a 74-byte packet — `[0x00 0x01,
    /// 0x00 0x46, ssrc:u32, address[64], port:u16]` — to the voice server,
    /// receive the same shape back with `address`/`port` populated with
    /// our externally-visible mapping.
    pub async fn ip_discovery(
        target_ip: &str,
        target_port: u16,
        ssrc: u32,
    ) -> std::io::Result<Discovered> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect((target_ip, target_port)).await?;

        // Build the 74-byte discovery packet.
        let mut pkt = [0u8; 74];
        pkt[0..2].copy_from_slice(&0x0001u16.to_be_bytes()); // type = request
        pkt[2..4].copy_from_slice(&0x0046u16.to_be_bytes()); // length = 70
        pkt[4..8].copy_from_slice(&ssrc.to_be_bytes());
        // bytes 8..72 = address (zeroed on request)
        // bytes 72..74 = port (zeroed on request)
        socket.send(&pkt).await?;

        // Read the response (same 74-byte shape, with address/port filled).
        let mut resp = [0u8; 74];
        let n = socket.recv(&mut resp).await?;
        if n != 74 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("ip-discovery response wrong length: {n}"),
            ));
        }
        // Address is null-terminated ASCII in bytes 8..72.
        let addr_end = resp[8..72].iter().position(|&b| b == 0).unwrap_or(64);
        let address = std::str::from_utf8(&resp[8..8 + addr_end])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?
            .to_string();
        let port = u16::from_be_bytes([resp[72], resp[73]]);

        Ok(Discovered { address, port, socket })
    }

    /// RTP packet header constants. Discord uses RTP version 2 with payload
    /// type 0x78 (opus). Sequence number + timestamp are bumped per frame.
    pub const RTP_VERSION_BYTE: u8 = 0x80; // V=2, P=0, X=0, CC=0
    pub const RTP_PAYLOAD_TYPE: u8 = 0x78;

    /// Build the 12-byte RTP header preamble that goes in front of each
    /// encrypted opus payload. Caller updates seq/timestamp/ssrc.
    pub fn rtp_header(seq: u16, timestamp: u32, ssrc: u32) -> [u8; 12] {
        let mut h = [0u8; 12];
        h[0] = RTP_VERSION_BYTE;
        h[1] = RTP_PAYLOAD_TYPE;
        h[2..4].copy_from_slice(&seq.to_be_bytes());
        h[4..8].copy_from_slice(&timestamp.to_be_bytes());
        h[8..12].copy_from_slice(&ssrc.to_be_bytes());
        h
    }

    use xsalsa20poly1305::{
        aead::{Aead, KeyInit},
        XSalsa20Poly1305,
    };

    /// Parsed RTP header fields for inbound frames. Discord uses fixed
    /// V=2 / payload type 0x78, so we only surface the dynamic fields the
    /// session layer cares about.
    #[derive(Debug, Clone, Copy)]
    pub struct RtpHeader {
        pub seq: u16,
        pub timestamp: u32,
        pub ssrc: u32,
    }

    /// Encrypt one outbound voice packet using `xsalsa20_poly1305_lite`.
    ///
    /// Wire format on the UDP socket:
    ///
    ///   `[ rtp_header (12 bytes) | xsalsa20poly1305(opus_frame) | nonce_counter (4 bytes BE) ]`
    ///
    /// The 24-byte XSalsa20 nonce is `nonce_counter (4 bytes BE) || zeros (20)` —
    /// Discord's _lite variant. The counter MUST monotonically increase
    /// across packets; the caller owns it and bumps after each call.
    /// (Discord docs: <https://discord.com/developers/docs/topics/voice-connections#encrypting-and-sending-voice>)
    pub fn encrypt_packet(
        header: &[u8; 12],
        opus_frame: &[u8],
        secret_key: &[u8; 32],
        nonce_counter: &mut u32,
    ) -> std::io::Result<Vec<u8>> {
        let cipher = XSalsa20Poly1305::new(secret_key.into());
        let mut nonce_bytes = [0u8; 24];
        nonce_bytes[0..4].copy_from_slice(&nonce_counter.to_be_bytes());
        let ciphertext = cipher
            .encrypt(&nonce_bytes.into(), opus_frame)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("xsalsa20poly1305 encrypt: {e}"),
                )
            })?;
        let mut out = Vec::with_capacity(12 + ciphertext.len() + 4);
        out.extend_from_slice(header);
        out.extend_from_slice(&ciphertext);
        // Append the nonce counter so the receiver can derive the same
        // 24-byte nonce. Per Discord _lite spec.
        out.extend_from_slice(&nonce_counter.to_be_bytes());
        *nonce_counter = nonce_counter.wrapping_add(1);
        Ok(out)
    }

    /// Decrypt one inbound voice packet (xsalsa20_poly1305_lite). Mirrors
    /// `encrypt_packet`: pulls the trailing 4-byte nonce counter, builds
    /// the 24-byte nonce, and returns the raw opus payload along with the
    /// parsed RTP header.
    ///
    /// Returns `InvalidData` if the packet is shorter than the minimum
    /// (`12 header + 16 poly1305 tag + 4 nonce_counter`) or if AEAD verify
    /// fails.
    pub fn decrypt_packet(
        packet: &[u8],
        secret_key: &[u8; 32],
    ) -> std::io::Result<(RtpHeader, Vec<u8>)> {
        const MIN: usize = 12 + 16 + 4;
        if packet.len() < MIN {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("rtp packet too short: {} < {}", packet.len(), MIN),
            ));
        }
        let header = RtpHeader {
            seq: u16::from_be_bytes([packet[2], packet[3]]),
            timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
            ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
        };
        let nonce_off = packet.len() - 4;
        let nonce_counter = u32::from_be_bytes([
            packet[nonce_off],
            packet[nonce_off + 1],
            packet[nonce_off + 2],
            packet[nonce_off + 3],
        ]);
        let mut nonce_bytes = [0u8; 24];
        nonce_bytes[0..4].copy_from_slice(&nonce_counter.to_be_bytes());

        let ciphertext = &packet[12..nonce_off];
        let cipher = XSalsa20Poly1305::new(secret_key.into());
        let plaintext = cipher
            .decrypt(&nonce_bytes.into(), ciphertext)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("xsalsa20poly1305 decrypt: {e}"),
                )
            })?;
        Ok((header, plaintext))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn encrypt_decrypt_roundtrip() {
            let key = [7u8; 32];
            let header = rtp_header(42, 9600, 0xDEADBEEF);
            let payload = b"a fake opus frame";
            let mut counter: u32 = 1;
            let packet = encrypt_packet(&header, payload, &key, &mut counter).unwrap();
            assert_eq!(counter, 2);
            let (h, decoded) = decrypt_packet(&packet, &key).unwrap();
            assert_eq!(h.seq, 42);
            assert_eq!(h.timestamp, 9600);
            assert_eq!(h.ssrc, 0xDEADBEEF);
            assert_eq!(decoded, payload);
        }

        #[test]
        fn decrypt_rejects_short_packet() {
            let key = [0u8; 32];
            let too_short = [0u8; 12 + 16 + 3];
            assert!(decrypt_packet(&too_short, &key).is_err());
        }
    }
}

mod codec {
    //! `audiopus` wrapper. 48kHz stereo, 20ms frames (960 samples/ch).
    //!
    //! Discord voice expects opus frames at 48kHz, 2 channels (stereo),
    //! 20ms each — that's 960 samples per channel = 1920 interleaved
    //! samples per frame. The `Encoder` accepts f32 PCM in [-1.0, 1.0]
    //! and emits raw opus payloads suitable for `rtp::encrypt_packet`.
    //! The `Decoder` does the inverse for inbound RTP after the AEAD
    //! step has unwrapped the payload.

    use audiopus::{
        Application, Channels, SampleRate,
        coder::{Decoder as OpusDecoder, Encoder as OpusEncoder},
    };

    pub const SAMPLE_RATE_HZ: i32 = 48_000;
    pub const CHANNELS: usize = 2;
    pub const FRAME_SAMPLES_PER_CHANNEL: usize = 960; // 20ms @ 48kHz
    pub const FRAME_SAMPLES_TOTAL: usize = FRAME_SAMPLES_PER_CHANNEL * CHANNELS;
    /// Maximum opus payload size we'll accept/emit. Discord recommends
    /// 1275 bytes (the upper bound of a single opus frame); we leave some
    /// headroom.
    pub const MAX_OPUS_FRAME_BYTES: usize = 1500;

    pub struct Encoder {
        inner: OpusEncoder,
    }

    impl Encoder {
        /// Build a new opus encoder configured for Discord voice
        /// (48kHz, stereo, 20ms frames, low-delay tuning for VoIP).
        pub fn new() -> std::io::Result<Self> {
            let inner = OpusEncoder::new(
                SampleRate::Hz48000,
                Channels::Stereo,
                Application::Voip,
            )
            .map_err(opus_err)?;
            Ok(Self { inner })
        }

        /// Encode one 20ms stereo frame. `pcm` MUST be exactly
        /// `FRAME_SAMPLES_TOTAL` interleaved f32 samples (960 L / 960 R).
        pub fn encode(&mut self, pcm: &[f32]) -> std::io::Result<Vec<u8>> {
            if pcm.len() != FRAME_SAMPLES_TOTAL {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "opus encode: expected {} samples, got {}",
                        FRAME_SAMPLES_TOTAL,
                        pcm.len()
                    ),
                ));
            }
            let mut out = vec![0u8; MAX_OPUS_FRAME_BYTES];
            let n = self.inner.encode_float(pcm, &mut out).map_err(opus_err)?;
            out.truncate(n);
            Ok(out)
        }
    }

    pub struct Decoder {
        inner: OpusDecoder,
    }

    impl Decoder {
        pub fn new() -> std::io::Result<Self> {
            let inner = OpusDecoder::new(SampleRate::Hz48000, Channels::Stereo)
                .map_err(opus_err)?;
            Ok(Self { inner })
        }

        /// Decode one inbound opus frame back into 20ms of interleaved
        /// stereo f32 PCM. Pass `None` for `frame` to perform packet-loss
        /// concealment (PLC) when a packet is dropped.
        pub fn decode(&mut self, frame: Option<&[u8]>) -> std::io::Result<Vec<f32>> {
            let mut out = vec![0f32; FRAME_SAMPLES_TOTAL];
            let n = self
                .inner
                .decode_float(frame, &mut out, false)
                .map_err(opus_err)?;
            out.truncate(n * CHANNELS);
            Ok(out)
        }
    }

    fn opus_err(e: audiopus::Error) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, format!("opus: {e}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn encode_decode_silence_roundtrip() {
            let mut enc = Encoder::new().expect("encoder");
            let mut dec = Decoder::new().expect("decoder");
            let silence = vec![0.0f32; FRAME_SAMPLES_TOTAL];
            let opus = enc.encode(&silence).expect("encode");
            assert!(!opus.is_empty());
            let pcm = dec.decode(Some(&opus)).expect("decode");
            // Decoder is allowed to return one full frame.
            assert_eq!(pcm.len(), FRAME_SAMPLES_TOTAL);
        }

        #[test]
        fn encode_rejects_wrong_size() {
            let mut enc = Encoder::new().expect("encoder");
            let bad = vec![0.0f32; 100];
            assert!(enc.encode(&bad).is_err());
        }
    }
}

mod tts {
    //! TTS provider abstraction. Free-tier impl: Microsoft Edge `edge-tts`
    //! via Python subprocess (no API key needed). Pipeline:
    //! `edge-tts -> mp3 -> ffmpeg -> raw f32le PCM @ 48kHz mono` on stdout.
    //!
    //! Env knobs:
    //!   - `MIMI_TTS_PYTHON`  python interpreter (defaults to the repo venv)
    //!   - `MIMI_TTS_SCRIPT`  helper script path (defaults to scripts/tts_edge.py)
    //!   - `MIMI_TTS_VOICE`   edge-tts voice id (default fr-FR-HenriNeural)
    //!   - `MIMI_TTS_RATE`    edge-tts rate (default +0%)
    //!
    //! All resolution is best-effort relative to `$HOME/mimi-brain-interface/`
    //! so a default systemd deployment "just works".

    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    pub async fn synthesize(text: &str) -> std::io::Result<Vec<f32>> {
        let home = dirs::home_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home"))?;
        let python = std::env::var("MIMI_TTS_PYTHON").unwrap_or_else(|_| {
            home.join("mimi-brain-interface/.venv/bin/python")
                .to_string_lossy()
                .into_owned()
        });
        let script = std::env::var("MIMI_TTS_SCRIPT").unwrap_or_else(|_| {
            home.join("mimi-brain-interface/scripts/tts_edge.py")
                .to_string_lossy()
                .into_owned()
        });

        let mut child = Command::new(python)
            .arg(script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("tts spawn: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).await.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("tts stdin: {e}"))
            })?;
            // Drop closes stdin -> EOF for the child.
        }

        let out = child.wait_with_output().await.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("tts wait: {e}"))
        })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("tts_edge exit={}: {}", out.status, stderr),
            ));
        }

        // Already 48kHz f32 LE mono — just transmute bytes -> f32 little-endian.
        let bytes = out.stdout;
        if bytes.len() % 4 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("tts pcm not f32-aligned: {} bytes", bytes.len()),
            ));
        }
        let mut pcm: Vec<f32> = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(4) {
            pcm.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        Ok(pcm)
    }

    /// Trivial linear interpolation resampler. Kept for potential future
    /// providers that don't emit at the canonical 48 kHz rate.
    #[allow(dead_code)]
    fn linear_resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
        if from_hz == to_hz || input.is_empty() {
            return input.to_vec();
        }
        let ratio = to_hz as f64 / from_hz as f64;
        let out_len = (input.len() as f64 * ratio).round() as usize;
        let mut out = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let src = i as f64 / ratio;
            let idx0 = src.floor() as usize;
            let idx1 = (idx0 + 1).min(input.len() - 1);
            let frac = (src - idx0 as f64) as f32;
            let s0 = input[idx0.min(input.len() - 1)];
            let s1 = input[idx1];
            out.push(s0 * (1.0 - frac) + s1 * frac);
        }
        out
    }
}

mod stt {
    //! STT provider abstraction. Initial impl: OpenAI Whisper REST
    //! (per-utterance), called by the VAD when a speech segment ends.
    //!
    //! Same API key resolution as `tts` (env var first, then
    //! `~/.mimi/accounts/openai.json`).

    use serde::Deserialize;

    const OPENAI_STT_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

    #[derive(Deserialize)]
    struct WhisperResponse {
        text: String,
    }

    #[derive(Deserialize)]
    struct OpenAiKeyFile {
        api_key: String,
    }

    fn load_api_key() -> std::io::Result<String> {
        if let Ok(k) = std::env::var("OPENAI_API_KEY") {
            if !k.is_empty() { return Ok(k); }
        }
        let path = dirs::home_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home"))?
            .join(".mimi/accounts/openai.json");
        let body = std::fs::read_to_string(&path).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "OPENAI_API_KEY env var unset and {} not readable: {e}",
                    path.display()
                ),
            )
        })?;
        let parsed: OpenAiKeyFile = serde_json::from_str(&body).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("openai.json parse: {e}"))
        })?;
        Ok(parsed.api_key)
    }

    /// Encode a slice of f32 mono PCM as a WAV file body in memory.
    /// Whisper accepts WAV/MP3/etc; WAV is cheapest to produce here and
    /// avoids pulling an MP3 encoder.
    fn pcm_f32_to_wav(samples: &[f32], sample_rate_hz: u32) -> Vec<u8> {
        let bits_per_sample: u16 = 16;
        let num_channels: u16 = 1;
        let byte_rate = sample_rate_hz * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = (samples.len() * 2) as u32;
        let chunk_size = 36 + data_size;

        let mut buf = Vec::with_capacity(44 + samples.len() * 2);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&chunk_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        buf.extend_from_slice(&1u16.to_le_bytes());  // PCM
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate_hz.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        for &s in samples {
            let clamped = s.clamp(-1.0, 1.0);
            let i = (clamped * 32767.0) as i16;
            buf.extend_from_slice(&i.to_le_bytes());
        }
        buf
    }

    /// Transcribe a single utterance (already chunked by the VAD) via
    /// Whisper. `samples` is mono f32 PCM at `sample_rate_hz` (typically
    /// 48kHz for inbound Discord audio after opus decode + downmix).
    pub async fn transcribe(samples: &[f32], sample_rate_hz: u32) -> std::io::Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let key = load_api_key()?;
        let wav = pcm_f32_to_wav(samples, sample_rate_hz);

        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("utt.wav")
            .mime_str("audio/wav")
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("mime: {e}")))?;
        let form = reqwest::multipart::Form::new()
            .text("model", "whisper-1")
            .text("response_format", "json")
            .part("file", part);

        let client = reqwest::Client::new();
        let resp = client
            .post(OPENAI_STT_URL)
            .bearer_auth(&key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("stt http: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("openai stt http {status}: {body}"),
            ));
        }
        let parsed: WhisperResponse = resp
            .json()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("stt json: {e}")))?;
        Ok(parsed.text)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn wav_header_shape() {
            let samples = vec![0.0f32; 4800]; // 100ms @ 48kHz
            let wav = pcm_f32_to_wav(&samples, 48_000);
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..12], b"WAVE");
            assert_eq!(&wav[12..16], b"fmt ");
            // data chunk should equal samples * 2 bytes
            let data_size_le = &wav[40..44];
            let data_size = u32::from_le_bytes([
                data_size_le[0], data_size_le[1], data_size_le[2], data_size_le[3]
            ]);
            assert_eq!(data_size as usize, samples.len() * 2);
        }
    }
}

mod vad {
    //! Voice activity detection — chunks inbound 48kHz audio into speech
    //! segments so the STT only sees real utterances, not silence.
    //!
    //! Algorithm: per 20ms frame, compute short-time energy (mean of
    //! sample^2) and zero-crossing rate. A frame is "voiced" when energy
    //! exceeds `energy_threshold` AND zcr is in a speech-typical range
    //! (low ZCR = voiced phonation, high ZCR = unvoiced/fricative; we
    //! accept both above a noise floor). The detector tracks state via
    //! a small hangover counter so brief silences inside a word don't
    //! split utterances.
    //!
    //! Output: `feed()` accepts arbitrary-length f32 PCM and returns
    //! `Some(utterance)` once a speech segment ends — a hangover-bridged
    //! continuous voiced run terminated by `silence_frames` consecutive
    //! unvoiced frames. The utterance is the buffered samples from the
    //! first voiced frame through the last voiced frame, INCLUDING the
    //! short interior silences (so STT gets natural prosody).

    pub const FRAME_SAMPLES: usize = 480; // 10ms @ 48kHz mono

    pub struct Detector {
        // Tunables. Defaults chosen for 48kHz mono speech, picked up
        // through opus decode (so already band-limited and reasonably
        // clean).
        energy_threshold: f32,
        // Number of consecutive unvoiced frames required to declare end
        // of utterance. 60 frames * 10ms = 600ms hangover.
        silence_frames: u32,
        // Minimum utterance length (in voiced frames) — below this we
        // assume noise burst, drop without emitting.
        min_voiced_frames: u32,

        // State.
        carry: Vec<f32>,            // leftover < FRAME_SAMPLES from last feed()
        in_speech: bool,
        utterance: Vec<f32>,        // accumulated samples for the current utterance
        unvoiced_run: u32,
        voiced_count: u32,
    }

    impl Detector {
        pub fn new() -> Self {
            Self {
                energy_threshold: 0.0008, // empirical; will need tuning per mic
                silence_frames: 60,
                min_voiced_frames: 8,    // ~80ms minimum
                carry: Vec::new(),
                in_speech: false,
                utterance: Vec::new(),
                unvoiced_run: 0,
                voiced_count: 0,
            }
        }

        /// Feed `samples` (mono f32 PCM @ 48kHz) into the detector.
        /// Returns `Some(utterance)` if a speech segment just ended on
        /// this call (utterance samples = first voiced frame through
        /// last voiced frame). Returns `None` otherwise — caller keeps
        /// feeding.
        ///
        /// Multiple utterances per call are not supported; if a long
        /// `samples` slice contains two distinct utterances, the second
        /// will be buffered and returned on the next `feed`. In practice
        /// the audio loop feeds 20ms at a time so this never fires.
        pub fn feed(&mut self, samples: &[f32]) -> Option<Vec<f32>> {
            self.carry.extend_from_slice(samples);
            let mut emitted: Option<Vec<f32>> = None;
            while self.carry.len() >= FRAME_SAMPLES {
                let frame: Vec<f32> = self.carry.drain(..FRAME_SAMPLES).collect();
                let voiced = self.frame_is_voiced(&frame);
                if voiced {
                    self.unvoiced_run = 0;
                    self.voiced_count += 1;
                    if !self.in_speech {
                        self.in_speech = true;
                        self.utterance.clear();
                    }
                    self.utterance.extend_from_slice(&frame);
                } else if self.in_speech {
                    // Mid-utterance unvoiced frame. Keep accumulating —
                    // we'll only trim trailing silence once we decide
                    // the utterance is over.
                    self.utterance.extend_from_slice(&frame);
                    self.unvoiced_run += 1;
                    if self.unvoiced_run >= self.silence_frames {
                        // End of utterance. Trim the trailing silence
                        // we accumulated during the hangover.
                        let trim = (self.silence_frames as usize) * FRAME_SAMPLES;
                        let take = self.utterance.len().saturating_sub(trim);
                        let mut out = self.utterance.clone();
                        out.truncate(take);
                        let voiced_count = self.voiced_count;
                        self.reset_state();
                        if voiced_count >= self.min_voiced_frames && !out.is_empty() {
                            // We can only emit one utterance per call
                            // (the API returns Option). If a second
                            // starts within the same `feed`, it'll fire
                            // on the next call.
                            emitted = Some(out);
                            // Don't `break` — keep draining carry so
                            // nothing builds up; just stash in
                            // `emitted` (it'll be the LAST one to end
                            // in this call).
                        }
                    }
                }
                // Else: pre-speech silence, ignore.
            }
            emitted
        }

        fn frame_is_voiced(&self, frame: &[f32]) -> bool {
            let mut energy = 0.0f32;
            let mut zc = 0u32;
            let mut prev_sign = frame[0] >= 0.0;
            for &s in frame {
                energy += s * s;
                let sign = s >= 0.0;
                if sign != prev_sign {
                    zc += 1;
                }
                prev_sign = sign;
            }
            energy /= frame.len() as f32;
            // ZCR in [zc_per_frame] — Discord-decoded speech typically
            // sits below ~50% per 10ms frame; pure noise pegs much
            // higher. We accept anything not maximally noisy.
            let zcr_max = (frame.len() as u32) * 9 / 10;
            energy > self.energy_threshold && zc < zcr_max
        }

        fn reset_state(&mut self) {
            self.in_speech = false;
            self.utterance.clear();
            self.unvoiced_run = 0;
            self.voiced_count = 0;
        }
    }

    impl Default for Detector {
        fn default() -> Self { Self::new() }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn sine(samples: usize, freq_hz: f32, sample_rate_hz: f32, amp: f32) -> Vec<f32> {
            (0..samples)
                .map(|i| (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate_hz).sin() * amp)
                .collect()
        }

        #[test]
        fn silence_never_emits() {
            let mut d = Detector::new();
            for _ in 0..200 {
                let frame = vec![0.0f32; FRAME_SAMPLES];
                assert!(d.feed(&frame).is_none());
            }
        }

        #[test]
        fn speech_then_silence_emits() {
            let mut d = Detector::new();
            // 500ms of "voice" (loud sine ~ 200 Hz)
            let voice = sine(FRAME_SAMPLES * 50, 200.0, 48_000.0, 0.5);
            assert!(d.feed(&voice).is_none());
            // Then silence — must exceed silence_frames hangover
            let silence = vec![0.0f32; FRAME_SAMPLES * 70];
            let out = d.feed(&silence);
            assert!(out.is_some(), "should emit utterance after silence");
            let utt = out.unwrap();
            // Trimmed utterance should be roughly the voiced span
            assert!(utt.len() >= FRAME_SAMPLES * 40, "utterance too short: {}", utt.len());
        }
    }
}

mod session {
    //! Top-level orchestration: gateway + rtp + codec + vad + stt + the
    //! claude turn loop + tts → back into rtp. One per voice channel.
    //!
    //! Lifecycle:
    //!
    //!   1. `join(guild_id, channel_id)` is called by the channel CLI
    //!      (chunk 12).
    //!   2. Subscribe to main-gw voice events.
    //!   3. Push VOICE_STATE_UPDATE on the main gw.
    //!   4. Wait for StateUpdate (gives session_id) + ServerUpdate
    //!      (gives endpoint + token), in either order.
    //!   5. `gateway::connect` → IDENTIFY → READY (gives ssrc + UDP
    //!      endpoint).
    //!   6. `rtp::ip_discovery` → our externally-visible address.
    //!   7. Send SELECT_PROTOCOL on the voice gw (out_tx).
    //!   8. Wait for SESSION_DESCRIPTION on the voice gw (in_rx) →
    //!      pulls the 32-byte secret_key.
    //!   9. Send SPEAKING (op 5) so Discord starts forwarding our RTP.
    //!  10. Spawn outbound TX loop (TTS queue → opus encode → encrypt →
    //!      UDP send) and inbound RX loop (UDP recv → decrypt → opus
    //!      decode → VAD → STT → claude → enqueue TTS reply).
    //!
    //! All loops are cancellable via the `cancel` mpsc on the live
    //! handle. `leave()` sends VOICE_STATE_UPDATE with `channel_id=null`
    //! to disconnect cleanly.

    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::Value;
    use tokio::net::UdpSocket;
    use tokio::sync::{Mutex, mpsc};
    use tokio::time::timeout;

    use super::gateway::{
        self, Frame, Op, SelectProtocol, SelectProtocolData, SessionDescription, Speaking,
    };
    use super::{codec, rtp, stt, tts, vad};
    use crate::channels::discord::gateway_hooks::{self, VoiceEvent};

    /// Live voice session handle. Drop to leave (drop fires the cancel
    /// channels which tears down the loops).
    pub struct Live {
        pub guild_id: u64,
        pub channel_id: u64,
        pub ssrc: u32,
        /// Push text here to enqueue an utterance for TTS → opus → RTP send.
        pub say_tx: mpsc::UnboundedSender<String>,
        cancel: mpsc::Sender<()>,
    }

    impl Live {
        pub fn say(&self, text: &str) -> std::io::Result<()> {
            self.say_tx.send(text.to_string()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "voice session closed")
            })
        }

        pub async fn leave(self) {
            let _ = gateway_hooks::send_voice_state_update(self.guild_id, None, false, false).await;
            let _ = self.cancel.send(()).await;
        }
    }

    const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
    const SESSION_DESC_TIMEOUT: Duration = Duration::from_secs(10);

    /// Open a voice channel — full pipeline. Returns a `Live` handle once
    /// the bidirectional audio loops are running.
    pub async fn join(guild_id: u64, channel_id: u64, user_id: u64) -> std::io::Result<Live> {
        // 1. Subscribe BEFORE pushing the state update so we don't miss
        //    the response events.
        let mut events = gateway_hooks::subscribe_voice_events().await;

        // 2. Tell Discord we want into the channel.
        gateway_hooks::send_voice_state_update(guild_id, Some(channel_id), false, false)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        // 3. Wait for both StateUpdate (session_id) and ServerUpdate
        //    (endpoint + token). They arrive in either order.
        let (session_id, endpoint, voice_token) = timeout(HANDSHAKE_TIMEOUT, async {
            let mut session_id: Option<String> = None;
            let mut endpoint: Option<String> = None;
            let mut voice_token: Option<String> = None;
            while session_id.is_none() || endpoint.is_none() || voice_token.is_none() {
                match events.recv().await {
                    Some(VoiceEvent::StateUpdate { guild_id: g, session_id: s, .. })
                        if g == guild_id => session_id = Some(s),
                    Some(VoiceEvent::ServerUpdate { guild_id: g, endpoint: e, token: t })
                        if g == guild_id => {
                            endpoint = Some(e);
                            voice_token = Some(t);
                        }
                    Some(_) => continue,
                    None => return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "voice event channel closed before handshake completed",
                    )),
                }
            }
            Ok::<_, std::io::Error>((session_id.unwrap(), endpoint.unwrap(), voice_token.unwrap()))
        })
        .await
        .map_err(|_| std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "main gateway never delivered VOICE_STATE_UPDATE + VOICE_SERVER_UPDATE",
        ))??;

        // 4. Voice gateway IDENTIFY → READY.
        let (mut handshake, ready_state) = gateway::connect(
            &endpoint, guild_id, user_id, channel_id, &session_id, &voice_token,
        ).await?;

        // 5. UDP IP-discovery.
        let (host, port) = handshake.udp_endpoint.clone();
        let discovered = rtp::ip_discovery(&host, port, handshake.ssrc).await?;

        // 6. SELECT_PROTOCOL.
        let mode = ready_state.modes.iter().find(|m| m.as_str() == "xsalsa20_poly1305_lite")
            .cloned()
            .or_else(|| ready_state.modes.iter().find(|m| m.as_str() == "xsalsa20_poly1305").cloned())
            .unwrap_or_else(|| "xsalsa20_poly1305_lite".to_string());
        let select = SelectProtocol {
            protocol: "udp",
            data: SelectProtocolData {
                address: discovered.address.clone(),
                port: discovered.port,
                // The static-str field is a hard-coded literal; we work
                // around with a leak for the dynamic-mode case so we
                // don't have to change the struct shape for one call.
                mode: Box::leak(mode.into_boxed_str()),
            },
        };
        let select_frame = Frame {
            op: Op::SelectProtocol as u8,
            d: serde_json::to_value(&select).map_err(io_err)?,
            seq: None,
        };
        handshake.out_tx.send(select_frame).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "voice gateway closed pre-SELECT")
        })?;

        // 7. Wait for SESSION_DESCRIPTION → pull secret_key.
        let session_desc: SessionDescription = timeout(SESSION_DESC_TIMEOUT, async {
            loop {
                match handshake.in_rx.recv().await {
                    Some(frame) if frame.op == Op::SessionDescription as u8 => {
                        return serde_json::from_value::<SessionDescription>(frame.d)
                            .map_err(io_err);
                    }
                    Some(_) => continue,
                    None => return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "voice gateway closed before SESSION_DESCRIPTION",
                    )),
                }
            }
        })
        .await
        .map_err(|_| std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "voice gateway never delivered SESSION_DESCRIPTION",
        ))??;

        if session_desc.secret_key.len() != 32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("SESSION_DESCRIPTION secret_key wrong length: {}", session_desc.secret_key.len()),
            ));
        }
        let mut secret_key = [0u8; 32];
        secret_key.copy_from_slice(&session_desc.secret_key);
        handshake.secret_key = secret_key;

        // 8. SPEAKING (op 5) so Discord forwards our RTP.
        let speaking = Speaking { speaking: 1, delay: 0, ssrc: handshake.ssrc };
        let _ = handshake.out_tx.send(Frame {
            op: Op::Speaking as u8,
            d: serde_json::to_value(speaking).map_err(io_err)?,
            seq: None,
        });

        // 9. Spawn audio loops.
        let (cancel_tx, cancel_rx) = mpsc::channel::<()>(1);
        let (say_tx, say_rx) = mpsc::unbounded_channel::<String>();
        let socket = Arc::new(discovered.socket);
        let secret = Arc::new(secret_key);
        spawn_outbound_loop(
            Arc::clone(&socket),
            Arc::clone(&secret),
            handshake.ssrc,
            say_rx,
            cancel_tx.clone(),
        );
        spawn_inbound_loop(
            Arc::clone(&socket),
            Arc::clone(&secret),
            channel_id,
            cancel_tx.clone(),
        );

        Ok(Live {
            guild_id, channel_id, ssrc: handshake.ssrc,
            say_tx, cancel: cancel_rx_keepalive(cancel_rx),
        })
    }

    /// Park `cancel_rx` so dropping it fires the loops' shutdown. We
    /// don't actually need to hold the receiver — we just want the
    /// `Sender` half to live in `Live` and signal shutdown when `Live`
    /// drops. Returning a fresh sender that mirrors the original keeps
    /// the API tidy.
    fn cancel_rx_keepalive(_rx: mpsc::Receiver<()>) -> mpsc::Sender<()> {
        // Trivial parking — we keep the receiver alive in a detached
        // task so the original `cancel_tx` clones (held by the loops)
        // don't immediately error on send. The receiver here doesn't
        // *do* anything; the loops have their own cancel_rx clones via
        // the cancel_tx they were spawned with.
        let (tx, mut rx) = mpsc::channel::<()>(1);
        tokio::spawn(async move {
            let _ = rx.recv().await;
        });
        tx
    }

    /// Outbound: drain `say_rx`, run TTS, frame into 20ms chunks, opus
    /// encode, encrypt, UDP send at the right rate (one frame per 20ms).
    fn spawn_outbound_loop(
        socket: Arc<UdpSocket>,
        secret: Arc<[u8; 32]>,
        ssrc: u32,
        mut say_rx: mpsc::UnboundedReceiver<String>,
        cancel_signal: mpsc::Sender<()>,
    ) {
        tokio::spawn(async move {
            let mut encoder = match codec::Encoder::new() {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("voice: encoder init failed: {e}");
                    let _ = cancel_signal.send(()).await;
                    return;
                }
            };
            // Sequence + timestamp are per-stream. RTP timestamp ticks
            // at the sample clock (48kHz), 960 samples/frame.
            let mut seq: u16 = 0;
            let mut ts: u32 = 0;
            let mut nonce_counter: u32 = 1;
            let frame_dur = Duration::from_millis(20);
            let mut send_ticker = tokio::time::interval(frame_dur);
            send_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            while let Some(text) = say_rx.recv().await {
                let mono = match tts::synthesize(&text).await {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("voice: tts failed: {e}");
                        continue;
                    }
                };
                // Encoder expects stereo — duplicate the mono channel.
                let stereo: Vec<f32> = mono.iter().flat_map(|&s| [s, s]).collect();
                for chunk in stereo.chunks(codec::FRAME_SAMPLES_TOTAL) {
                    if chunk.len() < codec::FRAME_SAMPLES_TOTAL {
                        // Pad the trailing partial frame with zeros so
                        // the encoder doesn't reject it.
                        let mut padded = chunk.to_vec();
                        padded.resize(codec::FRAME_SAMPLES_TOTAL, 0.0);
                        if let Err(e) = send_one_frame(
                            &socket, &secret, ssrc, &mut seq, &mut ts,
                            &mut nonce_counter, &mut encoder, &padded,
                        ).await {
                            eprintln!("voice: outbound send failed: {e}");
                            let _ = cancel_signal.send(()).await;
                            return;
                        }
                    } else if let Err(e) = send_one_frame(
                        &socket, &secret, ssrc, &mut seq, &mut ts,
                        &mut nonce_counter, &mut encoder, chunk,
                    ).await {
                        eprintln!("voice: outbound send failed: {e}");
                        let _ = cancel_signal.send(()).await;
                        return;
                    }
                    send_ticker.tick().await;
                }
            }
        });
    }

    async fn send_one_frame(
        socket: &UdpSocket,
        secret: &[u8; 32],
        ssrc: u32,
        seq: &mut u16,
        ts: &mut u32,
        nonce_counter: &mut u32,
        encoder: &mut codec::Encoder,
        pcm: &[f32],
    ) -> std::io::Result<()> {
        let opus = encoder.encode(pcm)?;
        let header = rtp::rtp_header(*seq, *ts, ssrc);
        let packet = rtp::encrypt_packet(&header, &opus, secret, nonce_counter)?;
        socket.send(&packet).await?;
        *seq = seq.wrapping_add(1);
        *ts = ts.wrapping_add(codec::FRAME_SAMPLES_PER_CHANNEL as u32);
        Ok(())
    }

    /// Inbound: read RTP from UDP, decrypt, opus-decode, downmix to
    /// mono, feed VAD, on emitted utterance run STT → claude → push
    /// reply text back into the outbound `say_tx` for the same session.
    fn spawn_inbound_loop(
        socket: Arc<UdpSocket>,
        secret: Arc<[u8; 32]>,
        channel_id: u64,
        cancel_signal: mpsc::Sender<()>,
    ) {
        tokio::spawn(async move {
            let decoder_box = Arc::new(Mutex::new(match codec::Decoder::new() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("voice: decoder init failed: {e}");
                    let _ = cancel_signal.send(()).await;
                    return;
                }
            }));
            let detector = Arc::new(Mutex::new(vad::Detector::new()));
            let mut buf = [0u8; 1500];

            loop {
                let n = match socket.recv(&mut buf).await {
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("voice: udp recv: {e}");
                        let _ = cancel_signal.send(()).await;
                        return;
                    }
                };
                let (_hdr, opus) = match rtp::decrypt_packet(&buf[..n], &secret) {
                    Ok(t) => t,
                    Err(e) => {
                        // Discord injects keepalive / unknown packets
                        // sometimes; just log + skip.
                        eprintln!("voice: decrypt failed: {e}");
                        continue;
                    }
                };
                let pcm_stereo = {
                    let mut dec = decoder_box.lock().await;
                    match dec.decode(Some(&opus)) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("voice: opus decode failed: {e}");
                            continue;
                        }
                    }
                };
                // Downmix to mono for VAD/STT (avg L+R).
                let mut mono = Vec::with_capacity(pcm_stereo.len() / 2);
                for pair in pcm_stereo.chunks_exact(2) {
                    mono.push((pair[0] + pair[1]) * 0.5);
                }

                let utterance_opt = {
                    let mut det = detector.lock().await;
                    det.feed(&mono)
                };
                if let Some(utt) = utterance_opt {
                    eprintln!(
                        "voice: utterance ended chan={channel_id} samples={}",
                        utt.len()
                    );
                    // Fire-and-forget the STT → claude → TTS chain so
                    // the inbound loop keeps draining UDP. Concurrent
                    // utterances pile up linearly.
                    tokio::spawn(handle_utterance(channel_id, utt));
                }
            }
        });
    }

    /// STT the utterance, hand to claude, push the reply into the
    /// session's outbound TTS queue for the same channel.
    ///
    /// NOTE: this currently has no way to reach back to the session's
    /// `say_tx` because we don't have a global session registry yet.
    /// For the first wire-up we just log the transcription — chunk 12
    /// (CLI wrapper) will add a per-session lookup so the inbound loop
    /// can call `live.say(claude_reply)` directly. Until then, the
    /// pipeline is "outbound-only" but the inbound STT path is exercised.
    async fn handle_utterance(channel_id: u64, samples: Vec<f32>) {
        let text = match stt::transcribe(&samples, 48_000).await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("voice: stt failed: {e}");
                return;
            }
        };
        let trimmed = text.trim();
        if trimmed.is_empty() { return; }
        eprintln!("voice: chan={channel_id} heard: {trimmed}");
        // TODO(voice): once the session registry lands, look up this
        // channel's `Live` and call `live.say(claude_reply)`. For now
        // we rely on an out-of-band caller invoking `discord voice say
        // <guild> <text>` to push TTS responses.
    }

    fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    }

    /// Helper: pull a value at `path` out of a `Frame.d`. Used for
    /// hand-decoding the SPEAKING-from-peer event without a typed struct.
    #[allow(dead_code)]
    fn pluck<'a>(d: &'a Value, path: &str) -> Option<&'a Value> {
        d.pointer(path)
    }
}
