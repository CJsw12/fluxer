//! Gateway WebSocket session: HELLO, IDENTIFY → READY, RESUME → RESUMED,
//! heartbeat ACK, op 14 accepted silently, and broadcast dispatches.
//! `compress=zstd-stream`: one zstd context per direction for the whole
//! connection, one flushed chunk per message (client side does the same).

use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use zstd::stream::raw::{CParameter, DParameter, Decoder, Encoder, InBuffer, Operation, OutBuffer};

use crate::Shared;

pub const HEARTBEAT_INTERVAL_MS: u64 = 41_250;
const SCRATCH_BYTES: usize = 16 * 1024;
const MAX_INFLATED_BYTES: usize = 1024 * 1024;

const CLOSE_DECODE_ERROR: u16 = 4002;
const CLOSE_NOT_AUTHENTICATED: u16 = 4003;
const CLOSE_AUTHENTICATION_FAILED: u16 = 4004;
const CLOSE_ALREADY_AUTHENTICATED: u16 = 4005;

struct Zstd {
    encoder: Encoder<'static>,
    decoder: Decoder<'static>,
    scratch: Vec<u8>,
}

impl Zstd {
    fn new() -> Result<Self> {
        let mut encoder = Encoder::new(3).context("zstd encoder")?;
        encoder.set_parameter(CParameter::WindowLog(18)).context("zstd window log")?;
        let mut decoder = Decoder::new().context("zstd decoder")?;
        decoder.set_parameter(DParameter::WindowLogMax(21)).context("zstd window log max")?;
        Ok(Self { encoder, decoder, scratch: vec![0; SCRATCH_BYTES] })
    }

    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        let mut src = InBuffer::around(input);
        let mut out = Vec::new();
        while src.pos() < input.len() {
            let mut dst = OutBuffer::around(&mut self.scratch[..]);
            self.encoder.run(&mut src, &mut dst).context("zstd compress")?;
            let written = dst.pos();
            out.extend_from_slice(&self.scratch[..written]);
        }
        loop {
            let mut dst = OutBuffer::around(&mut self.scratch[..]);
            let remaining = self.encoder.flush(&mut dst).context("zstd flush")?;
            let written = dst.pos();
            out.extend_from_slice(&self.scratch[..written]);
            if remaining == 0 {
                return Ok(out);
            }
        }
    }

    fn decompress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        let mut src = InBuffer::around(input);
        let mut out = Vec::new();
        loop {
            let mut dst = OutBuffer::around(&mut self.scratch[..]);
            self.decoder.run(&mut src, &mut dst).context("zstd decompress")?;
            let written = dst.pos();
            out.extend_from_slice(&self.scratch[..written]);
            anyhow::ensure!(out.len() <= MAX_INFLATED_BYTES, "client message too large");
            if src.pos() == input.len() && written < self.scratch.len() {
                return Ok(out);
            }
        }
    }
}

struct Session {
    zstd: Option<Zstd>,
    seq: u64,
    session_id: Option<String>,
}

type Sink = futures_util::stream::SplitSink<WebSocketStream<TcpStream>, Message>;

impl Session {
    async fn send(&mut self, sink: &mut Sink, payload: &Value) -> Result<()> {
        let text = serde_json::to_string(payload).context("encode gateway payload")?;
        let message = match &mut self.zstd {
            Some(zstd) => Message::binary(zstd.compress(text.as_bytes())?),
            None => Message::text(text),
        };
        sink.send(message).await.context("gateway send")
    }

    async fn dispatch(&mut self, sink: &mut Sink, t: &str, d: &Value) -> Result<()> {
        self.seq += 1;
        let payload = json!({ "op": 0, "t": t, "s": self.seq, "d": d });
        self.send(sink, &payload).await
    }
}

async fn close(sink: &mut Sink, code: u16, reason: &'static str) {
    let frame = CloseFrame { code: CloseCode::from(code), reason: reason.into() };
    let _ = sink.send(Message::Close(Some(frame))).await;
}

pub async fn run(ws: WebSocketStream<TcpStream>, zstd: bool, shared: Arc<Shared>) -> Result<()> {
    let (mut sink, mut stream) = ws.split();
    let mut events = shared.events.subscribe();
    let mut session = Session { zstd: if zstd { Some(Zstd::new()?) } else { None }, seq: 0, session_id: None };
    session.send(&mut sink, &json!({ "op": 10, "d": { "heartbeat_interval": HEARTBEAT_INTERVAL_MS } })).await?;
    loop {
        tokio::select! {
            incoming = stream.next() => {
                let message = match incoming {
                    None | Some(Err(_)) => return Ok(()),
                    Some(Ok(message)) => message,
                };
                let bytes = match message {
                    Message::Text(text) => text.as_bytes().to_vec(),
                    Message::Binary(data) => match &mut session.zstd {
                        Some(zstd) => match zstd.decompress(&data) {
                            Ok(bytes) => bytes,
                            Err(_) => {
                                close(&mut sink, CLOSE_DECODE_ERROR, "Decode error").await;
                                return Ok(());
                            }
                        },
                        None => data.to_vec(),
                    },
                    Message::Close(_) => return Ok(()),
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                };
                let Ok(payload) = serde_json::from_slice::<Value>(&bytes) else {
                    close(&mut sink, CLOSE_DECODE_ERROR, "Decode error").await;
                    return Ok(());
                };
                if !handle_payload(&mut session, &mut sink, &shared, &payload).await? {
                    return Ok(());
                }
            }
            event = events.recv(), if session.session_id.is_some() => match event {
                Ok(event) => session.dispatch(&mut sink, &event.t, &event.d).await?,
                Err(RecvError::Lagged(skipped)) => eprintln!("gateway session lagged, dropped {skipped} events"),
                Err(RecvError::Closed) => return Ok(()),
            },
        }
    }
}

/// Returns `false` when the connection was closed.
async fn handle_payload(session: &mut Session, sink: &mut Sink, shared: &Shared, payload: &Value) -> Result<bool> {
    let op = payload["op"].as_u64().unwrap_or(u64::MAX);
    let d = &payload["d"];
    match op {
        // HEARTBEAT
        1 => session.send(sink, &json!({ "op": 11, "d": null })).await?,
        // IDENTIFY
        2 => {
            if session.session_id.is_some() {
                close(sink, CLOSE_ALREADY_AUTHENTICATED, "Already authenticated").await;
                return Ok(false);
            }
            let token = d["token"].as_str().unwrap_or_default();
            let ready = {
                let mut state = shared.lock();
                if !state.tokens.contains(token) {
                    None
                } else {
                    let session_id = state.secret(32);
                    state.sessions.insert(session_id.clone());
                    Some((session_id.clone(), state.ready_payload(&session_id)))
                }
            };
            let Some((session_id, ready)) = ready else {
                close(sink, CLOSE_AUTHENTICATION_FAILED, "Authentication failed").await;
                return Ok(false);
            };
            session.session_id = Some(session_id);
            session.dispatch(sink, "READY", &ready).await?;
        }
        // RESUME
        6 => {
            let token = d["token"].as_str().unwrap_or_default();
            let session_id = d["session_id"].as_str().unwrap_or_default().to_owned();
            let ok = {
                let state = shared.lock();
                state.tokens.contains(token) && state.sessions.contains(&session_id)
            };
            if !ok {
                session.send(sink, &json!({ "op": 9, "d": false })).await?;
                return Ok(true);
            }
            // No replay buffer: continue numbering from the client's seq.
            session.seq = d["seq"].as_u64().unwrap_or(0);
            session.session_id = Some(session_id);
            session.dispatch(sink, "RESUMED", &json!({})).await?;
        }
        // PRESENCE_UPDATE, VOICE_STATE_UPDATE, LAZY_REQUEST and anything else
        // after authentication: accepted and ignored.
        _ if session.session_id.is_some() => {
            if op == 14 {
                eprintln!("gateway: lazy request {}", d["subscriptions"]);
            }
        }
        _ => {
            close(sink, CLOSE_NOT_AUTHENTICATED, "Not authenticated").await;
            return Ok(false);
        }
    }
    Ok(true)
}
