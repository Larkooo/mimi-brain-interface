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
