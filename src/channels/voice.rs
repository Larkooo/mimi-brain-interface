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

#![allow(dead_code)] // skeleton — APIs not wired up yet.

use std::sync::Arc;

use tokio::sync::Mutex;

/// One live voice connection for a single guild.
///
/// Created by `VoiceSession::join`. Drops cleanly via the cancellation
/// token in `Drop` so the gateway WS / UDP socket / encoder threads all
/// shut down together.
pub struct VoiceSession {
    pub guild_id: u64,
    pub channel_id: u64,
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    state: ConnState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnState {
    Idle,
    Connecting,
    Ready,
    Closed,
}

impl VoiceSession {
    /// Open a voice connection. Negotiates the gateway handshake and the
    /// UDP endpoint. Currently a stub — real impl is pending.
    pub async fn join(_guild_id: u64, _channel_id: u64) -> std::io::Result<Self> {
        // TODO(voice): trigger VOICE_STATE_UPDATE on the main gateway,
        // wait for VOICE_SERVER_UPDATE, then open the voice WS, do
        // IDENTIFY/SELECT_PROTOCOL, IP-discovery the UDP socket, and
        // store the secret_key in `Inner`.
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "voice gateway not yet implemented",
        ))
    }

    /// One-shot TTS — synthesize `text`, opus-encode, push into the live
    /// connection. Stub.
    pub async fn say(&self, _text: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "tts pipeline not yet implemented",
        ))
    }

    /// Disconnect cleanly. Stub.
    pub async fn leave(self) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "voice gateway not yet implemented",
        ))
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
}

mod rtp {
    //! UDP RTP send/receive. 20ms opus frames, sequence + timestamp
    //! bookkeeping, xsalsa20poly1305 encryption with the secret_key from
    //! the gateway. Inbound: per-SSRC demux into separate audio streams.
}

mod codec {
    //! `audiopus` wrapper. 48kHz stereo, 20ms frames (960 samples/ch).
}

mod tts {
    //! TTS provider abstraction. Initial impl: OpenAI TTS REST. Returns
    //! a stream of 48kHz f32 samples for the encoder to chunk.
}

mod stt {
    //! STT provider abstraction. Initial impl: OpenAI Whisper REST
    //! (per-utterance), called by the VAD when a speech segment ends.
}

mod vad {
    //! Voice activity detection — chunks inbound 48kHz audio into speech
    //! segments so the STT only sees real utterances, not silence.
}

mod session {
    //! Top-level orchestration: gateway + rtp + codec + vad + stt + the
    //! claude turn loop + tts → back into rtp. One per voice channel.
}
