//! Fluxer gateway client: one WebSocket connection per account, driven by a
//! tokio task (HELLO -> IDENTIFY/RESUME -> heartbeat -> DISPATCH, reconnect).
//!
//! Protocol reference: `docs/rust_client_core/GATEWAY_PROTOCOL.md`.

mod codec;
mod connection;
mod subscriptions;
#[cfg(test)]
mod tests;

pub use subscriptions::{
    MEMBER_LIST_RANGE_MAX_END, MEMBER_LIST_RANGE_MAX_RANGES, MEMBER_LIST_RANGE_MAX_SPAN,
    MEMBER_LIST_RANGE_OVERSCAN_PAGES, MEMBER_LIST_RANGE_PAGE_SIZE, PresenceUpdate, VoiceStateUpdate,
    member_list_page_ranges, member_list_subscription, normalize_member_list_ranges,
};

use std::fmt;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};

/// Identify flag bits (`packages/constants/src/GatewayConstants.ts:30-34`).
pub const FLAG_DEBOUNCE_MESSAGE_REACTIONS: u64 = 1 << 1;
pub const FLAG_CHANNEL_THREADS: u64 = 1 << 2;
pub const FLAG_GUILD_VISITS: u64 = 1 << 3;

/// Server-side maximum for one client message (`constants.erl:max_payload_size`).
pub const MAX_CLIENT_PAYLOAD_BYTES: usize = 16 * 1024;

const COMMAND_QUEUE_CAPACITY: usize = 64;
const EVENT_QUEUE_CAPACITY: usize = 256;

/// Gateway op codes used by this client.
pub mod op {
    pub const DISPATCH: u8 = 0;
    pub const HEARTBEAT: u8 = 1;
    pub const IDENTIFY: u8 = 2;
    pub const PRESENCE_UPDATE: u8 = 3;
    pub const VOICE_STATE_UPDATE: u8 = 4;
    pub const RESUME: u8 = 6;
    pub const RECONNECT: u8 = 7;
    pub const INVALID_SESSION: u8 = 9;
    pub const HELLO: u8 = 10;
    pub const HEARTBEAT_ACK: u8 = 11;
    pub const GATEWAY_ERROR: u8 = 12;
    pub const LAZY_REQUEST: u8 = 14;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    ZstdStream,
}

/// IDENTIFY `properties`. The server only requires `os`, `browser`, `device`
/// to be strings; anything else (`os_version`, `locale`, ...) goes in `extra`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IdentifyProperties {
    pub os: String,
    pub browser: String,
    pub device: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Clone)]
pub struct GatewayConfig {
    /// `endpoints.gateway` from the instance discovery document (ws/wss).
    pub url: url::Url,
    pub token: String,
    pub properties: IdentifyProperties,
    pub compress: Compression,
    pub flags: u64,
    pub initial_guild_id: Option<String>,
    /// Initial presence for IDENTIFY (`{status, afk, mobile, custom_status?}`).
    pub presence: Option<Value>,
}

impl fmt::Debug for GatewayConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewayConfig")
            .field("url", &self.url.as_str())
            .field("token", &"<redacted>")
            .field("properties", &self.properties)
            .field("compress", &self.compress)
            .field("flags", &self.flags)
            .field("initial_guild_id", &self.initial_guild_id)
            .field("presence", &self.presence)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GatewayEvent {
    /// Every op 0 frame, including READY and RESUMED, in arrival order.
    Dispatch(crate::Dispatch),
    /// Emitted right after `Dispatch(READY)`.
    Ready { session_id: String },
    /// Emitted right after `Dispatch(RESUMED)`.
    Resumed,
    /// The connection dropped; a new one starts after backoff.
    /// `attempt` counts reconnects since the last READY/RESUMED (1-based).
    Reconnecting { code: Option<u16>, attempt: u32 },
    InvalidSession { resumable: bool },
    /// Fatal close (4004, 4010-4012, 4002 without compression). The task ends.
    Closed { code: u16, reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("gateway url must use ws or wss, got {0:?}")]
    InvalidUrl(String),
    #[error("gateway::spawn must be called inside a tokio runtime")]
    NoRuntime,
    #[error("gateway payload is {size} bytes (max {MAX_CLIENT_PAYLOAD_BYTES})")]
    PayloadTooLarge { size: usize },
    #[error("gateway command queue is full")]
    QueueFull,
    #[error("gateway task has stopped")]
    Closed,
    #[error("failed to encode gateway payload: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("invalid gateway command argument: {0}")]
    InvalidArgument(&'static str),
}

pub(crate) enum Command {
    /// Pre-serialized `{"op","d"}` frame.
    Send(String),
    /// Presence `d`; remembered for IDENTIFY and re-sent after RESUMED.
    Presence(Value),
}

/// Cloneable control handle. Commands are queued (bounded) and sent once the
/// session is READY/RESUMED. Dropping every clone closes the gateway.
#[derive(Debug, Clone)]
pub struct GatewayHandle {
    commands: mpsc::Sender<Command>,
    shutdown: Arc<watch::Sender<bool>>,
}

impl GatewayHandle {
    /// Queues a raw `{"op": op, "d": d}` frame.
    pub fn send_raw(&self, op: u8, d: Value) -> Result<(), GatewayError> {
        let payload = encode_frame(op, &d)?;
        self.push(Command::Send(payload))
    }

    /// LAZY_REQUEST (op 14). `subscriptions` is keyed by guild id, e.g.
    /// `{"<guild_id>": {"active": true, "sync": true}}`.
    pub fn lazy_request(&self, subscriptions: Value) -> Result<(), GatewayError> {
        self.send_raw(op::LAZY_REQUEST, json!({ "subscriptions": subscriptions }))
    }

    /// PRESENCE_UPDATE (op 3). `presence` must contain `status`.
    pub fn update_presence(&self, presence: Value) -> Result<(), GatewayError> {
        encode_frame(op::PRESENCE_UPDATE, &presence)?;
        self.push(Command::Presence(presence))
    }

    /// Closes the connection with code 1000 (server drops the session) and
    /// ends the task. The event receiver then yields `None`.
    pub fn close(&self) {
        self.shutdown.send_replace(true);
    }

    fn push(&self, command: Command) -> Result<(), GatewayError> {
        self.commands.try_send(command).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => GatewayError::QueueFull,
            mpsc::error::TrySendError::Closed(_) => GatewayError::Closed,
        })
    }
}

pub(crate) fn encode_frame(op: u8, d: &Value) -> Result<String, GatewayError> {
    let payload = serde_json::to_string(&json!({ "op": op, "d": d }))?;
    if payload.len() > MAX_CLIENT_PAYLOAD_BYTES {
        return Err(GatewayError::PayloadTooLarge {
            size: payload.len(),
        });
    }
    Ok(payload)
}

/// Starts the gateway task on the current tokio runtime.
pub fn spawn(
    config: GatewayConfig,
) -> Result<(GatewayHandle, mpsc::Receiver<GatewayEvent>), GatewayError> {
    spawn_with(config, |url: url::Url| async move {
        let (ws, _response) = tokio_tungstenite::connect_async(url.as_str()).await?;
        Ok(ws)
    })
}

/// `spawn` with an injectable connector (tests use in-memory streams).
pub(crate) fn spawn_with<C, F, S>(
    config: GatewayConfig,
    connect: C,
) -> Result<(GatewayHandle, mpsc::Receiver<GatewayEvent>), GatewayError>
where
    C: FnMut(url::Url) -> F + Send + 'static,
    F: Future<Output = anyhow::Result<tokio_tungstenite::WebSocketStream<S>>> + Send + 'static,
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    if !matches!(config.url.scheme(), "ws" | "wss") {
        return Err(GatewayError::InvalidUrl(config.url.scheme().to_owned()));
    }
    let runtime = tokio::runtime::Handle::try_current().map_err(|_| GatewayError::NoRuntime)?;
    let (command_tx, command_rx) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
    let (event_tx, event_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let runner = connection::Runner::new(config, connect, event_tx, command_rx, shutdown_rx);
    runtime.spawn(runner.run());
    let handle = GatewayHandle {
        commands: command_tx,
        shutdown: Arc::new(shutdown_tx),
    };
    Ok((handle, event_rx))
}
