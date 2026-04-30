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

    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use tokio::sync::mpsc;
    use tokio::time::interval;
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    /// Output of a successful gateway handshake — what the RTP layer needs
    /// to start encrypting / sending audio. The voice WS task continues
    /// running in the background after this returns; cancelling the
    /// returned `cancel` channel hangs up cleanly.
    pub struct Handshake {
        pub ssrc: u32,
        pub udp_endpoint: (String, u16),
        pub secret_key: [u8; 32],
        pub cancel: mpsc::Sender<()>,
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
        session_id: &str,
        token: &str,
    ) -> std::io::Result<(Handshake, ReadyState)> {
        let url = format!("wss://{}/?v=8", endpoint);
        let (ws, _resp) = connect_async(&url).await.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e.to_string())
        })?;
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
            })
            .map_err(io_err)?,
            seq: None,
        };
        sink.send(Message::Text(
            serde_json::to_string(&identify).map_err(io_err)?.into(),
        ))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))?;

        // 3. Receive READY (Discord may interleave a HEARTBEAT_ACK or
        //    HELLO-echo, so we loop until we see op 2).
        let ready: Ready = loop {
            match stream.next().await {
                Some(Ok(Message::Text(txt))) => {
                    let frame: Frame = serde_json::from_str(&txt).map_err(io_err)?;
                    if frame.op == Op::Ready as u8 {
                        break serde_json::from_value(frame.d).map_err(io_err)?;
                    }
                    // Unhandled pre-READY frame — log and continue.
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        e.to_string(),
                    ));
                }
                None => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "voice ws closed before READY",
                    ));
                }
            }
        };

        // 4. Spawn heartbeat + demux background task. Owns the WS sink/
        //    stream for the rest of the connection lifetime; cancellable
        //    via the returned mpsc.
        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
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
                    inbound = stream.next() => match inbound {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(Message::Text(_))) => {
                            // SESSION_DESCRIPTION / SPEAKING / etc. land here.
                            // The session.rs orchestration layer will swap in
                            // a real demuxer (mpsc out) in a follow-up commit;
                            // for now we just keep the connection warm.
                        }
                        _ => {}
                    }
                }
            }
        });

        let handshake = Handshake {
            ssrc: ready.ssrc,
            udp_endpoint: (ready.ip.clone(), ready.port),
            // Filled in by SELECT_PROTOCOL step in session.rs; placeholder
            // so the type is well-formed before that step runs.
            secret_key: [0u8; 32],
            cancel: cancel_tx,
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
