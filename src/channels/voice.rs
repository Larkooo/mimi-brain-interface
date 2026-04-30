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
    //! TTS provider abstraction. Initial impl: OpenAI TTS REST. Returns
    //! a stream of 48kHz f32 samples for the encoder to chunk.
    //!
    //! API key resolution order:
    //!   1. `OPENAI_API_KEY` env var (set on the systemd unit or shell)
    //!   2. `~/.mimi/accounts/openai.json` with `{"api_key": "sk-..."}`
    //!
    //! OWNER ACTION REQUIRED if neither is present — `tts::synthesize`
    //! will return an error at runtime. Drop a key into either location
    //! and the voice session will pick it up on the next call.

    use serde::{Deserialize, Serialize};

    const OPENAI_TTS_URL: &str = "https://api.openai.com/v1/audio/speech";
    /// OpenAI TTS supports `pcm` response format: signed-16-bit, 24kHz,
    /// mono, little-endian. We resample to 48kHz here so the encoder
    /// gets the canonical Discord rate.
    const OPENAI_TTS_RATE_HZ: u32 = 24_000;
    const TARGET_RATE_HZ: u32 = 48_000;

    #[derive(Serialize)]
    struct TtsRequest<'a> {
        model: &'a str,
        input: &'a str,
        voice: &'a str,
        response_format: &'a str,
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
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("openai.json parse: {e}"),
            )
        })?;
        Ok(parsed.api_key)
    }

    /// Synthesize `text` via OpenAI TTS and return 48kHz mono f32 PCM.
    /// The opus encoder expects stereo, so the caller duplicates the
    /// channel before encoding (cheap; saves a TTS round-trip on a
    /// stereo voice that nobody can hear the difference on).
    pub async fn synthesize(text: &str) -> std::io::Result<Vec<f32>> {
        let key = load_api_key()?;
        let req = TtsRequest {
            model: "tts-1",       // low-latency tier; tts-1-hd if quality > speed
            input: text,
            voice: "alloy",       // sane neutral default; switchable later
            response_format: "pcm",
        };
        let client = reqwest::Client::new();
        let resp = client
            .post(OPENAI_TTS_URL)
            .bearer_auth(&key)
            .json(&req)
            .send()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("tts http: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("openai tts http {status}: {body}"),
            ));
        }
        let bytes = resp.bytes().await.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("tts read: {e}"))
        })?;
        // OpenAI returns signed-16 LE PCM mono @ 24kHz.
        let mut pcm24: Vec<f32> = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            let s = i16::from_le_bytes([chunk[0], chunk[1]]);
            pcm24.push(s as f32 / 32768.0);
        }
        Ok(linear_resample(&pcm24, OPENAI_TTS_RATE_HZ, TARGET_RATE_HZ))
    }

    /// Trivial linear interpolation resampler. Good enough for speech;
    /// if we ever care about quality, swap for `dasp_signal` or libsamplerate.
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
}

mod session {
    //! Top-level orchestration: gateway + rtp + codec + vad + stt + the
    //! claude turn loop + tts → back into rtp. One per voice channel.
}
