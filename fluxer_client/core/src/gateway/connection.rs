//! Connection lifecycle task: connect, HELLO, IDENTIFY/RESUME, heartbeat,
//! dispatch forwarding, close-code policy and reconnect backoff.
//!
//! Timings follow `fluxer_app/src/features/gateway/transport/GatewaySocket.ts`
//! (see `docs/rust_client_core/GATEWAY_PROTOCOL.md` 3.2, 3.5-3.7).

use std::time::Duration;

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, sleep, sleep_until, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use url::Url;

use super::codec::{ZstdDeflater, ZstdInflater};
use super::{Command, Compression, GatewayConfig, GatewayEvent, encode_frame, op};

pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const HELLO_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const HEARTBEAT_ACK_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const RESUME_TIMEOUT: Duration = Duration::from_secs(15);
const RESUMABLE_WINDOW: Duration = Duration::from_secs(180);
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 41_250;
/// Upper bound for a server-sent interval so `Instant + delay` cannot overflow.
const MAX_HEARTBEAT_INTERVAL_MS: u64 = 600_000;
const RECONNECT_MIN_MS: u64 = 1_000;
const RECONNECT_MAX_MS: u64 = 10_000;
const RECONNECT_FLOOR_MS: u64 = 500;
const RECONNECT_SPREAD_MS: u64 = 2_000;
const INVALID_SESSION_IDENTIFY_SPREAD_MS: u64 = 1_000;
const INVALID_SESSION_RECONNECT_MIN_MS: u64 = 2_500;
const INVALID_SESSION_RECONNECT_MAX_MS: u64 = 3_500;

pub(crate) const CLOSE_NORMAL: u16 = 1000;
pub(crate) const CLOSE_RECONNECT: u16 = 4000;
const CLOSE_DECODE_ERROR: u16 = 4002;
const CLOSE_AUTHENTICATION_FAILED: u16 = 4004;
const CLOSE_INVALID_SEQ: u16 = 4007;
const CLOSE_SESSION_TIMEOUT: u16 = 4009;

#[derive(Debug, Deserialize)]
struct Payload {
    op: u8,
    #[serde(default)]
    d: Value,
    #[serde(default)]
    s: Option<u64>,
    #[serde(default)]
    t: Option<String>,
}

enum Exit {
    /// `close()` or every handle/receiver dropped.
    Shutdown,
    Fatal {
        code: u16,
        reason: String,
    },
    Reconnect {
        code: Option<u16>,
        /// RECONNECT, heartbeat failure, INVALID_SESSION, compression fallback.
        immediate: bool,
        min_delay: Duration,
    },
}

impl Exit {
    fn reconnect(code: Option<u16>, immediate: bool) -> Self {
        Exit::Reconnect {
            code,
            immediate,
            min_delay: Duration::ZERO,
        }
    }
}

enum Step {
    Message(Option<Result<Message, tokio_tungstenite::tungstenite::Error>>),
    HelloTimeout,
    HeartbeatTick,
    AckTimeout,
    ResumeTimeout,
    Identify,
    DelayedReconnect,
    Command(Option<Command>),
    Shutdown,
}

enum Incoming {
    Payload(Payload),
    Nothing,
    Closed { code: Option<u16>, reason: String },
    DecompressFailed(anyhow::Error),
    Invalid(anyhow::Error),
}

/// Per-connection timers and flags.
#[derive(Default)]
struct ConnState {
    hello_deadline: Option<Instant>,
    interval_ms: u64,
    next_heartbeat: Option<Instant>,
    ack_deadline: Option<Instant>,
    resume_deadline: Option<Instant>,
    identify_at: Option<Instant>,
    reconnect_at: Option<Instant>,
    /// READY or RESUMED received; queued commands may be sent.
    ready: bool,
}

struct Conn<S> {
    ws: WebSocketStream<S>,
    inflater: Option<ZstdInflater>,
    deflater: Option<ZstdDeflater>,
}

impl<S> Conn<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    async fn send_text(&mut self, text: &str) -> anyhow::Result<()> {
        let message = match &mut self.deflater {
            Some(deflater) => Message::binary(deflater.compress(text.as_bytes())?),
            None => Message::text(text.to_owned()),
        };
        timeout(SEND_TIMEOUT, self.ws.send(message))
            .await
            .context("gateway send timed out")?
            .context("gateway send failed")
    }

    /// Best effort; the peer may already be gone.
    async fn close(&mut self, code: u16) {
        let frame = CloseFrame {
            code: CloseCode::from(code),
            reason: Utf8Bytes::from_static(""),
        };
        let _ = timeout(CLOSE_TIMEOUT, self.ws.close(Some(frame))).await;
    }

    fn decode(&mut self, message: Message) -> Incoming {
        match message {
            Message::Text(text) => parse_payload(text.as_bytes()),
            Message::Binary(data) => match &mut self.inflater {
                Some(inflater) => match inflater.decompress(&data) {
                    Ok(Some(bytes)) => parse_payload(&bytes),
                    Ok(None) => Incoming::Nothing,
                    Err(err) => Incoming::DecompressFailed(err),
                },
                None => parse_payload(&data),
            },
            Message::Close(frame) => Incoming::Closed {
                code: frame.as_ref().map(|f| u16::from(f.code)),
                reason: frame.map(|f| f.reason.to_string()).unwrap_or_default(),
            },
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Incoming::Nothing,
        }
    }
}

fn parse_payload(bytes: &[u8]) -> Incoming {
    match serde_json::from_slice::<Payload>(bytes) {
        Ok(payload) => Incoming::Payload(payload),
        Err(err) => Incoming::Invalid(err.into()),
    }
}

pub(crate) struct Runner<C> {
    config: GatewayConfig,
    connect: C,
    events: mpsc::Sender<GatewayEvent>,
    commands: mpsc::Receiver<Command>,
    shutdown: watch::Receiver<bool>,
    compress: Compression,
    session_id: Option<String>,
    seq: Option<u64>,
    last_activity: Option<Instant>,
    presence: Option<Value>,
    backoff: Backoff,
    attempt: u32,
}

/// Reconnect backoff (`GatewaySocket.nextReconnectDelay`): the attempt
/// count grows only when the previous backoff was at least
/// `RECONNECT_MIN_MS` ago; a quicker failure waits the minimum plus spread
/// without counting. Measured from the last counted backoff (not from the
/// connect attempt), so a server refusing at once still backs off to the
/// maximum instead of retrying every 1-3 s forever.
#[derive(Debug, Default)]
pub(crate) struct Backoff {
    attempts: u32,
    last_at: Option<Instant>,
}

impl Backoff {
    /// Inclusive bounds in ms of the next (not immediate) reconnect delay.
    pub(crate) fn next_bounds(&mut self, now: Instant) -> (u64, u64) {
        let recent = self
            .last_at
            .is_some_and(|at| now.saturating_duration_since(at) < Duration::from_millis(RECONNECT_MIN_MS));
        if recent {
            return (RECONNECT_MIN_MS, RECONNECT_MIN_MS + RECONNECT_SPREAD_MS);
        }
        self.last_at = Some(now);
        self.attempts = self.attempts.saturating_add(1);
        let exponent = (self.attempts - 1).min(20);
        let ceiling = RECONNECT_MIN_MS
            .saturating_mul(1u64 << exponent)
            .min(RECONNECT_MAX_MS);
        (RECONNECT_FLOOR_MS, ceiling)
    }

    /// READY / RESUMED.
    pub(crate) fn reset(&mut self) {
        self.attempts = 0;
    }
}

impl<C, F, S> Runner<C>
where
    C: FnMut(Url) -> F + Send + 'static,
    F: Future<Output = anyhow::Result<WebSocketStream<S>>> + Send + 'static,
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    pub(crate) fn new(
        config: GatewayConfig,
        connect: C,
        events: mpsc::Sender<GatewayEvent>,
        commands: mpsc::Receiver<Command>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self {
            compress: config.compress,
            presence: config.presence.clone(),
            config,
            connect,
            events,
            commands,
            shutdown,
            session_id: None,
            seq: None,
            last_activity: None,
            backoff: Backoff::default(),
            attempt: 0,
        }
    }

    pub(crate) async fn run(mut self) {
        loop {
            if *self.shutdown.borrow() {
                return;
            }
            let url = gateway_url(&self.config.url, self.compress);
            let connect = (self.connect)(url);
            let connected = tokio::select! {
                result = timeout(CONNECT_TIMEOUT, connect) => result,
                () = wait_shutdown(&mut self.shutdown) => return,
            };
            let exit = match connected {
                Ok(Ok(ws)) => self.run_connection(ws).await,
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, "gateway connect failed");
                    Exit::reconnect(None, false)
                }
                Err(_) => {
                    tracing::warn!("gateway connect timed out");
                    Exit::reconnect(None, false)
                }
            };
            match exit {
                Exit::Shutdown => return,
                Exit::Fatal { code, reason } => {
                    tracing::warn!(code, %reason, "gateway closed fatally");
                    let _ = self.events.send(GatewayEvent::Closed { code, reason }).await;
                    return;
                }
                Exit::Reconnect {
                    code,
                    immediate,
                    min_delay,
                } => {
                    self.attempt = self.attempt.saturating_add(1);
                    let event = GatewayEvent::Reconnecting {
                        code,
                        attempt: self.attempt,
                    };
                    if !self.emit(event).await {
                        return;
                    }
                    let delay = self.backoff_delay(immediate).max(min_delay);
                    tracing::info!(?code, attempt = self.attempt, ?delay, "gateway reconnecting");
                    tokio::select! {
                        () = sleep(delay) => {}
                        () = wait_shutdown(&mut self.shutdown) => return,
                    }
                }
            }
        }
    }

    async fn run_connection(&mut self, ws: WebSocketStream<S>) -> Exit {
        let zstd = self.compress == Compression::ZstdStream;
        let codecs = if zstd {
            ZstdInflater::new().and_then(|inf| Ok((Some(inf), Some(ZstdDeflater::new()?))))
        } else {
            Ok((None, None))
        };
        let (inflater, deflater) = match codecs {
            Ok(codecs) => codecs,
            Err(err) => {
                tracing::warn!(error = %err, "zstd init failed; falling back to uncompressed");
                self.compress = Compression::None;
                return Exit::reconnect(None, true);
            }
        };
        let mut conn = Conn {
            ws,
            inflater,
            deflater,
        };
        let mut st = ConnState {
            hello_deadline: Some(Instant::now() + HELLO_TIMEOUT),
            interval_ms: DEFAULT_HEARTBEAT_INTERVAL_MS,
            ..ConnState::default()
        };
        loop {
            let ready = st.ready;
            let step = tokio::select! {
                message = conn.ws.next() => Step::Message(message),
                () = sleep_opt(st.hello_deadline) => Step::HelloTimeout,
                () = sleep_opt(st.next_heartbeat) => Step::HeartbeatTick,
                () = sleep_opt(st.ack_deadline) => Step::AckTimeout,
                () = sleep_opt(st.resume_deadline) => Step::ResumeTimeout,
                () = sleep_opt(st.identify_at) => Step::Identify,
                () = sleep_opt(st.reconnect_at) => Step::DelayedReconnect,
                command = self.commands.recv(), if ready => Step::Command(command),
                () = wait_shutdown(&mut self.shutdown) => Step::Shutdown,
            };
            let exit = match self.handle_step(&mut conn, &mut st, step).await {
                Ok(None) => continue,
                Ok(Some(exit)) => exit,
                Err(err) => {
                    tracing::warn!(error = %err, "gateway connection error");
                    Exit::reconnect(None, false)
                }
            };
            let close_code = match exit {
                Exit::Shutdown => CLOSE_NORMAL,
                _ => CLOSE_RECONNECT,
            };
            conn.close(close_code).await;
            return exit;
        }
    }

    async fn handle_step(
        &mut self,
        conn: &mut Conn<S>,
        st: &mut ConnState,
        step: Step,
    ) -> anyhow::Result<Option<Exit>> {
        let now = Instant::now();
        match step {
            Step::Message(None) => Ok(Some(Exit::reconnect(None, false))),
            Step::Message(Some(Err(err))) => {
                tracing::warn!(error = %err, "gateway socket error");
                Ok(Some(Exit::reconnect(None, false)))
            }
            Step::Message(Some(Ok(message))) => match conn.decode(message) {
                Incoming::Nothing => Ok(None),
                Incoming::Payload(payload) => self.handle_payload(conn, st, payload).await,
                Incoming::Closed { code, reason } => Ok(Some(self.close_policy(code, reason))),
                Incoming::DecompressFailed(err) => {
                    tracing::warn!(error = %err, "zstd decompress failed; falling back to uncompressed");
                    self.compress = Compression::None;
                    Ok(Some(Exit::reconnect(None, true)))
                }
                Incoming::Invalid(err) => {
                    tracing::warn!(error = %err, "invalid gateway payload");
                    Ok(Some(Exit::reconnect(None, false)))
                }
            },
            Step::HelloTimeout => {
                tracing::warn!("gateway HELLO timed out");
                Ok(Some(Exit::reconnect(None, false)))
            }
            Step::HeartbeatTick => {
                if st.ack_deadline.is_some() {
                    let defer = (st.interval_ms.saturating_mul(3) / 4).clamp(500, 15_000);
                    st.next_heartbeat = Some(now + Duration::from_millis(defer));
                } else {
                    self.send_heartbeat(conn, st).await?;
                    st.next_heartbeat = Some(now + heartbeat_delay(st.interval_ms));
                }
                Ok(None)
            }
            Step::AckTimeout => {
                tracing::warn!("gateway heartbeat ACK timed out");
                Ok(Some(Exit::reconnect(None, true)))
            }
            Step::ResumeTimeout => {
                tracing::warn!("gateway RESUME timed out; discarding session");
                self.clear_session();
                Ok(Some(Exit::reconnect(None, true)))
            }
            Step::Identify => {
                st.identify_at = None;
                self.send_identify(conn).await?;
                Ok(None)
            }
            Step::DelayedReconnect => Ok(Some(Exit::reconnect(None, true))),
            Step::Command(None) | Step::Shutdown => Ok(Some(Exit::Shutdown)),
            Step::Command(Some(Command::Send(payload))) => {
                conn.send_text(&payload).await?;
                Ok(None)
            }
            Step::Command(Some(Command::Presence(presence))) => {
                let payload = encode_frame(op::PRESENCE_UPDATE, &presence)?;
                self.presence = Some(presence);
                conn.send_text(&payload).await?;
                Ok(None)
            }
        }
    }

    async fn handle_payload(
        &mut self,
        conn: &mut Conn<S>,
        st: &mut ConnState,
        payload: Payload,
    ) -> anyhow::Result<Option<Exit>> {
        let now = Instant::now();
        match payload.op {
            op::HELLO => {
                if st.hello_deadline.take().is_none() {
                    return Ok(None);
                }
                st.interval_ms = payload
                    .d
                    .get("heartbeat_interval")
                    .and_then(Value::as_u64)
                    .filter(|ms| *ms > 0)
                    .unwrap_or(DEFAULT_HEARTBEAT_INTERVAL_MS)
                    .min(MAX_HEARTBEAT_INTERVAL_MS);
                st.next_heartbeat = Some(now + heartbeat_delay(st.interval_ms));
                // Resumability is judged on activity from the previous connection.
                if self.can_resume() {
                    self.send_resume(conn).await?;
                    st.resume_deadline = Some(now + RESUME_TIMEOUT);
                } else {
                    self.send_identify(conn).await?;
                }
                self.last_activity = Some(now);
                Ok(None)
            }
            op::HEARTBEAT => {
                self.last_activity = Some(now);
                self.send_heartbeat(conn, st).await?;
                Ok(None)
            }
            op::HEARTBEAT_ACK => {
                self.last_activity = Some(now);
                st.ack_deadline = None;
                Ok(None)
            }
            op::DISPATCH => {
                self.last_activity = Some(now);
                self.handle_dispatch(conn, st, payload).await
            }
            op::RECONNECT => {
                tracing::info!("gateway requested reconnect");
                Ok(Some(Exit::reconnect(None, true)))
            }
            op::INVALID_SESSION => {
                let resumable = payload.d.as_bool().unwrap_or(false);
                if !self.emit(GatewayEvent::InvalidSession { resumable }).await {
                    return Ok(Some(Exit::Shutdown));
                }
                st.ready = false;
                if st.resume_deadline.take().is_some() && !resumable {
                    // RESUME rejected: identify on the same connection.
                    self.clear_session();
                    let spread = rand::rng().random_range(0..=INVALID_SESSION_IDENTIFY_SPREAD_MS);
                    st.identify_at = Some(now + Duration::from_millis(spread));
                } else {
                    if !resumable {
                        self.clear_session();
                    }
                    let wait = rand::rng().random_range(
                        INVALID_SESSION_RECONNECT_MIN_MS..=INVALID_SESSION_RECONNECT_MAX_MS,
                    );
                    st.reconnect_at = Some(now + Duration::from_millis(wait));
                }
                Ok(None)
            }
            op::GATEWAY_ERROR => {
                tracing::warn!(d = %payload.d, "gateway error");
                Ok(None)
            }
            other => {
                tracing::debug!(op = other, "ignoring gateway op");
                Ok(None)
            }
        }
    }

    async fn handle_dispatch(
        &mut self,
        conn: &mut Conn<S>,
        st: &mut ConnState,
        payload: Payload,
    ) -> anyhow::Result<Option<Exit>> {
        let Some(t) = payload.t else {
            return Ok(None);
        };
        if let Some(s) = payload.s
            && self.seq.is_none_or(|current| s > current)
        {
            self.seq = Some(s);
        }
        let s = payload.s.or(self.seq).unwrap_or(0);
        let ready_session = (t == "READY").then(|| {
            payload
                .d
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
        let resumed = t == "RESUMED";
        let dispatch = crate::Dispatch { t, s, d: payload.d };
        if !self.emit(GatewayEvent::Dispatch(dispatch)).await {
            return Ok(Some(Exit::Shutdown));
        }
        if let Some(session_id) = ready_session {
            self.mark_connected(st);
            let Some(session_id) = session_id else {
                tracing::warn!("READY without session_id; resume disabled");
                return Ok(None);
            };
            self.session_id = Some(session_id.clone());
            if !self.emit(GatewayEvent::Ready { session_id }).await {
                return Ok(Some(Exit::Shutdown));
            }
        } else if resumed {
            st.resume_deadline = None;
            self.mark_connected(st);
            if !self.emit(GatewayEvent::Resumed).await {
                return Ok(Some(Exit::Shutdown));
            }
            if let Some(presence) = &self.presence {
                let payload = encode_frame(op::PRESENCE_UPDATE, presence)?;
                conn.send_text(&payload).await?;
            }
        }
        Ok(None)
    }

    fn close_policy(&mut self, code: Option<u16>, reason: String) -> Exit {
        tracing::info!(?code, %reason, "gateway closed by server");
        match code {
            Some(CLOSE_AUTHENTICATION_FAILED | 4010..=4012) => Exit::Fatal {
                code: code.unwrap_or_default(),
                reason,
            },
            Some(CLOSE_DECODE_ERROR) if self.compress == Compression::ZstdStream => {
                self.compress = Compression::None;
                Exit::reconnect(code, true)
            }
            Some(CLOSE_DECODE_ERROR) => Exit::Fatal {
                code: CLOSE_DECODE_ERROR,
                reason,
            },
            Some(CLOSE_INVALID_SEQ | CLOSE_SESSION_TIMEOUT) => {
                self.clear_session();
                Exit::reconnect(code, false)
            }
            _ => Exit::reconnect(code, false),
        }
    }

    fn mark_connected(&mut self, st: &mut ConnState) {
        st.ready = true;
        self.attempt = 0;
        self.backoff.reset();
    }

    fn clear_session(&mut self) {
        self.session_id = None;
        self.seq = None;
    }

    fn can_resume(&self) -> bool {
        self.session_id.is_some()
            && self.seq.is_some()
            && self
                .last_activity
                .is_some_and(|at| at.elapsed() <= RESUMABLE_WINDOW)
    }

    async fn send_identify(&mut self, conn: &mut Conn<S>) -> anyhow::Result<()> {
        let mut d = json!({
            "token": self.config.token,
            "properties": self.config.properties,
            "flags": self.config.flags,
        });
        if let Some(guild_id) = &self.config.initial_guild_id {
            d["initial_guild_id"] = json!(guild_id);
        }
        if let Some(presence) = &self.presence {
            d["presence"] = presence.clone();
        }
        let payload = encode_frame(op::IDENTIFY, &d)?;
        conn.send_text(&payload).await
    }

    async fn send_resume(&mut self, conn: &mut Conn<S>) -> anyhow::Result<()> {
        let d = json!({
            "token": self.config.token,
            "session_id": self.session_id,
            "seq": self.seq,
        });
        let payload = encode_frame(op::RESUME, &d)?;
        conn.send_text(&payload).await
    }

    async fn send_heartbeat(&mut self, conn: &mut Conn<S>, st: &mut ConnState) -> anyhow::Result<()> {
        let payload = encode_frame(op::HEARTBEAT, &json!(self.seq))?;
        conn.send_text(&payload).await?;
        let now = Instant::now();
        self.last_activity = Some(now);
        if st.ack_deadline.is_none() {
            st.ack_deadline = Some(now + HEARTBEAT_ACK_TIMEOUT);
        }
        Ok(())
    }

    /// Returns `false` when the event receiver is gone (treated as shutdown).
    async fn emit(&mut self, event: GatewayEvent) -> bool {
        self.events.send(event).await.is_ok()
    }

    fn backoff_delay(&mut self, immediate: bool) -> Duration {
        let mut rng = rand::rng();
        if immediate {
            return Duration::from_millis(rng.random_range(0..=RECONNECT_SPREAD_MS));
        }
        let (low, high) = self.backoff.next_bounds(Instant::now());
        Duration::from_millis(rng.random_range(low..=high))
    }
}

/// Appends `v=1&encoding=json` (+ `compress=zstd-stream&stream=1`) to the
/// discovery URL, keeping any unrelated query pairs.
pub(crate) fn gateway_url(base: &Url, compress: Compression) -> Url {
    let mut url = base.clone();
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| !matches!(key.as_ref(), "v" | "encoding" | "compress" | "stream"))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        query.extend_pairs(kept);
        query.append_pair("v", "1");
        query.append_pair("encoding", "json");
        if compress == Compression::ZstdStream {
            query.append_pair("compress", "zstd-stream");
            query.append_pair("stream", "1");
        }
    }
    url
}

/// `max(1000, 0.8 * interval) + rand(0..=min(1500, 0.05 * interval))`.
pub(crate) fn heartbeat_delay(interval_ms: u64) -> Duration {
    let base = (interval_ms.saturating_mul(4) / 5).max(1_000);
    let jitter_max = (interval_ms / 20).min(1_500);
    let jitter = rand::rng().random_range(0..=jitter_max);
    Duration::from_millis(base.saturating_add(jitter))
}

async fn sleep_opt(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Resolves when `close()` was called or every handle was dropped.
async fn wait_shutdown(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow_and_update() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}
