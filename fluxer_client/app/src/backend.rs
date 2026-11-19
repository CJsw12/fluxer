//! Network side of a logged-in session: REST client, gateway connection and
//! the shared `ClientState`. Everything here runs on the tokio runtime; the
//! UI only receives `UiEvent`s and reads the state under the mutex.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use fluxer_client_core::gateway::{self, Compression, GatewayConfig, GatewayEvent, GatewayHandle, IdentifyProperties};
use fluxer_client_core::rest::{
    AckRequest, ClientIdentity, DiscoveryOptions, Endpoints, MessageQuery, RestClient, RestConfig, SecretToken,
    SendMessageRequest, TransportPolicy, UserProfileFull, discover_with,
};
#[cfg(test)]
use fluxer_client_core::rest::{GuildSettingsPatch, UserSettingsPatch};
use fluxer_client_core::state::{ClientState, LoadMode, Message, PAGE_SIZE, Snowflake};
use serde_json::{Value, json};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::notify::{Notification, NotifyFocus, notification_for};

/// Something the UI should react to. The UI re-reads the state on every
/// event, so events only say *that* something changed plus status/errors.
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    StateChanged,
    Gateway(GatewayStatus),
    /// A REST page for the channel was applied (or failed).
    HistoryLoaded { channel_id: Snowflake, error: Option<String> },
    /// A plain message send failed (shown as "Message not sent: ...").
    SendFailed { error: String },
    /// A message action (reply, reaction, edit, delete) failed; `error`
    /// already names the action.
    ActionFailed { error: String },
    /// `open_dm` created (or found) the DM channel; it is in the state.
    DmOpened { channel_id: Snowflake },
    /// An incoming message should play a sound and maybe show a toast.
    Notify(Notification),
    /// A toast was clicked: focus the window and open the channel.
    NotificationClicked { guild_id: Option<Snowflake>, channel_id: Snowflake },
    /// Upload progress of the pending message `nonce`, 0..=100.
    UploadProgress { nonce: Snowflake, percent: u8 },
    /// The upload of `nonce` finished (sent or failed).
    UploadEnded { nonce: Snowflake },
    /// A user profile fetch finished.
    ProfileLoaded {
        user_id: Snowflake,
        guild_id: Option<Snowflake>,
        result: Result<UserProfileFull, String>,
    },
    /// A user note save finished (error is `Some` on failure after rollback).
    NoteSaved { user_id: Snowflake, error: Option<String> },
    /// Friend request by username/tag finished.
    FriendRequestSent { input: String, error: Option<String> },
    /// `GET /invites/{code}` preview finished.
    InvitePreviewLoaded { code: String, result: Result<InvitePreview, String> },
    /// `POST /invites/{code}` accept finished; `Ok(AcceptedInvite)`.
    InviteAccepted { code: String, result: Result<AcceptedInvite, String> },
    /// `DELETE /users/@me/guilds/{id}` finished.
    GuildLeft { guild_id: Snowflake, error: Option<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum GatewayStatus {
    Connecting,
    Ready,
    Reconnecting { attempt: u32 },
    Closed { reason: String },
}

/// Instance endpoints plus a REST client for them (token not set yet).
#[derive(Clone)]
pub struct Instance {
    pub endpoints: Endpoints,
    pub rest: Arc<RestClient>,
}

/// Discovery + REST client construction (`discover_with` resolves the typed
/// address itself).
pub async fn connect_instance(input: &str, allow_insecure_loopback: bool) -> Result<Instance> {
    let policy = TransportPolicy { allow_insecure_loopback };
    let options = DiscoveryOptions { policy, ..DiscoveryOptions::default() };
    let endpoints = discover_with(input, &options).await?;
    instance_from_endpoints(endpoints, allow_insecure_loopback)
}

/// REST client for already known endpoints (discovery or the session cache).
pub fn instance_from_endpoints(endpoints: Endpoints, allow_insecure_loopback: bool) -> Result<Instance> {
    let mut config = RestConfig::new(&endpoints, ClientIdentity::default());
    config.policy = TransportPolicy { allow_insecure_loopback };
    let rest = RestClient::new(config)?;
    Ok(Instance { endpoints, rest: Arc::new(rest) })
}

pub fn lock(state: &Mutex<ClientState>) -> MutexGuard<'_, ClientState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
pub(crate) fn lock_sent<T>(sent: &Mutex<T>) -> MutexGuard<'_, T> {
    sent.lock().unwrap_or_else(PoisonError::into_inner)
}

mod actions;

pub use actions::TypingSender;
mod members;
mod navigation;
mod profile;
mod settings;
mod social;
mod upload;

pub use social::{AcceptedInvite, InvitePreview, extract_invite_code};
#[cfg(test)]
pub use social::SocialCall;

/// A logged-in session. Cloning shares the same connection and state.
#[derive(Clone)]
pub struct Backend {
    runtime: Handle,
    rest: Arc<RestClient>,
    state: Arc<Mutex<ClientState>>,
    events: mpsc::UnboundedSender<UiEvent>,
    gateway: Option<GatewayHandle>,
    last_nonce: Arc<AtomicU64>,
    /// Instance media endpoint (avatars), from discovery.
    media: Option<Arc<str>>,
    /// Hostnames belonging to the active Fluxer instance (trusted external links).
    instance_hosts: Arc<[String]>,
    /// Window focus and viewed channel for the notification decision.
    focus: Arc<NotifyFocus>,
    /// Monotonic sequence counters so overlapping note/settings saves never
    /// roll back a newer change on an older failure.
    note_seq: Arc<AtomicU64>,
    settings_seq: Arc<AtomicU64>,
    guild_settings_seq: Arc<Mutex<BTreeMap<Option<Snowflake>, u64>>>,
    /// Every `POST .../messages` body, in order (tests check what the
    /// composer sent; the test runtime never runs the request).
    #[cfg(test)]
    pub(crate) sent: Arc<Mutex<Vec<(Snowflake, SendMessageRequest)>>>,
    /// Recipients of every `open_dm` call (tests).
    #[cfg(test)]
    pub(crate) opened_dms: Arc<Mutex<Vec<Snowflake>>>,
    /// Every multipart send: channel, JSON part and files (tests).
    #[cfg(test)]
    pub(crate) sent_uploads: Arc<Mutex<Vec<upload::SentUpload>>>,
    /// Every message page requested: channel, mode, anchor id (tests).
    #[cfg(test)]
    pub(crate) page_loads: Arc<Mutex<Vec<(Snowflake, LoadMode, Option<Snowflake>)>>>,
    /// Saved user notes `(user_id, note)` (tests).
    #[cfg(test)]
    pub(crate) saved_notes: Arc<Mutex<Vec<(Snowflake, Option<String>)>>>,
    /// Sent `PATCH /users/@me/settings` payloads (tests).
    #[cfg(test)]
    pub(crate) sent_settings: Arc<Mutex<Vec<UserSettingsPatch>>>,
    /// Sent `PATCH /users/@me/guilds/{g|@me}/settings` payloads (tests).
    #[cfg(test)]
    pub(crate) sent_guild_settings: Arc<Mutex<Vec<(Option<Snowflake>, GuildSettingsPatch)>>>,
    /// Recorded social & community actions (tests).
    #[cfg(test)]
    pub(crate) sent_social: Arc<Mutex<Vec<SocialCall>>>,
}

impl Backend {
    /// Starts the gateway and the task that applies its dispatches to the
    /// state. The caller has already set `token` on `instance.rest`.
    pub fn start(
        runtime: Handle,
        instance: Instance,
        token: SecretToken,
    ) -> Result<(Self, mpsc::UnboundedReceiver<UiEvent>)> {
        let url = url::Url::parse(&instance.endpoints.gateway).context("gateway URL from discovery is invalid")?;
        let config = GatewayConfig {
            url,
            token: token.expose().to_owned(),
            properties: IdentifyProperties {
                os: std::env::consts::OS.to_owned(),
                browser: "Fluxer Rust".to_owned(),
                device: "desktop".to_owned(),
                extra: serde_json::Map::new(),
            },
            compress: Compression::ZstdStream,
            flags: 0,
            initial_guild_id: None,
            presence: None,
        };
        let (gateway, gateway_events) = {
            let _guard = runtime.enter();
            gateway::spawn(config)?
        };
        let (mut backend, rx) = Self::new(runtime, instance.rest, Some(gateway));
        backend.media = instance.endpoints.media.as_deref().map(Arc::from);
        let hosts = crate::trusted_domains::hosts_from_urls(
            [
                Some(instance.endpoints.api.as_str()),
                Some(instance.endpoints.gateway.as_str()),
                instance.endpoints.media.as_deref(),
                instance.endpoints.static_cdn.as_deref(),
                instance.endpoints.upload_relay.as_deref(),
                instance.endpoints.invite.as_deref(),
                instance.endpoints.webapp.as_deref(),
            ]
            .into_iter()
            .flatten(),
        );
        backend.instance_hosts = Arc::from(hosts);
        backend.runtime.spawn(pump_gateway(
            gateway_events,
            backend.state.clone(),
            backend.events.clone(),
            backend.focus.clone(),
        ));
        let _ = backend.events.send(UiEvent::Gateway(GatewayStatus::Connecting));
        Ok((backend, rx))
    }

    /// A backend without a gateway connection (tests feed the state directly).
    pub fn new(
        runtime: Handle,
        rest: Arc<RestClient>,
        gateway: Option<GatewayHandle>,
    ) -> (Self, mpsc::UnboundedReceiver<UiEvent>) {
        let (events, rx) = mpsc::unbounded_channel();
        let backend = Self {
            runtime,
            rest,
            state: Arc::new(Mutex::new(ClientState::new())),
            events,
            gateway,
            last_nonce: Arc::new(AtomicU64::new(0)),
            media: None,
            instance_hosts: Arc::from(Vec::new()),
            focus: Arc::default(),
            note_seq: Arc::new(AtomicU64::new(0)),
            settings_seq: Arc::new(AtomicU64::new(0)),
            guild_settings_seq: Arc::default(),
            #[cfg(test)]
            sent: Arc::default(),
            #[cfg(test)]
            opened_dms: Arc::default(),
            #[cfg(test)]
            sent_uploads: Arc::default(),
            #[cfg(test)]
            page_loads: Arc::default(),
            #[cfg(test)]
            saved_notes: Arc::default(),
            #[cfg(test)]
            sent_settings: Arc::default(),
            #[cfg(test)]
            sent_guild_settings: Arc::default(),
            #[cfg(test)]
            sent_social: Arc::default(),
        };
        (backend, rx)
    }

    pub fn state(&self) -> MutexGuard<'_, ClientState> {
        lock(&self.state)
    }

    pub fn media(&self) -> Option<&str> {
        self.media.as_deref()
    }

    pub fn instance_hosts(&self) -> &[String] {
        &self.instance_hosts
    }

    #[cfg(test)]
    pub(crate) fn set_instance_hosts_for_test(&mut self, hosts: Vec<String>) {
        self.instance_hosts = Arc::from(hosts);
    }

    /// Shared with the gateway task; the UI keeps it current.
    pub fn notify_focus(&self) -> &NotifyFocus {
        &self.focus
    }

    /// For OS callbacks that report back to the UI (toast clicks).
    #[cfg(not(test))]
    pub fn event_sender(&self) -> mpsc::UnboundedSender<UiEvent> {
        self.events.clone()
    }

    /// Marks `message_id` read: local state now (clears the unread dot),
    /// then `POST /read-states/ack`. A failed POST is only logged; the next
    /// READY/MESSAGE_ACK carries the server's state.
    pub fn ack(&self, channel_id: Snowflake, message_id: Snowflake) {
        let changes = self.state().local_ack(channel_id, message_id);
        if !changes.is_empty() {
            let _ = self.events.send(UiEvent::StateChanged);
        }
        let rest = self.rest.clone();
        self.runtime.spawn(async move {
            let ack = AckRequest {
                channel_id: channel_id.to_string(),
                message_id: message_id.to_string(),
                mention_count: None,
                manual: false,
            };
            if let Err(err) = rest.ack_message(&ack).await {
                tracing::warn!(%err, %channel_id, "read ack failed");
            }
        });
    }

    /// Asks the gateway to stream member/presence/message events for a guild.
    pub fn subscribe_guild(&self, guild_id: Snowflake) {
        let Some(gateway) = &self.gateway else { return };
        let subscriptions = json!({ guild_id.to_string(): { "active": true, "sync": true } });
        if let Err(err) = gateway.lazy_request(subscriptions) {
            tracing::warn!(%err, "LAZY_REQUEST not queued");
        }
    }

    /// `GET /channels/{id}/messages`: latest page, or the page before `before`.
    pub fn load_messages(&self, channel_id: Snowflake, before: Option<Snowflake>) {
        match before {
            Some(id) => self.load_page(channel_id, LoadMode::Before, Some(id)),
            None => self.load_page(channel_id, LoadMode::Replace, None),
        }
    }

    /// The page after `after` (back towards the newest messages once newer
    /// ones were trimmed from the window).
    pub fn load_newer(&self, channel_id: Snowflake, after: Snowflake) {
        self.load_page(channel_id, LoadMode::After, Some(after));
    }

    fn load_page(&self, channel_id: Snowflake, mode: LoadMode, anchor: Option<Snowflake>) {
        #[cfg(test)]
        lock_sent(&self.page_loads).push((channel_id, mode, anchor));
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let anchor = anchor.map(|id| id.to_string());
            let query = MessageQuery {
                before: anchor.clone().filter(|_| mode == LoadMode::Before),
                after: anchor.filter(|_| mode == LoadMode::After),
                limit: u8::try_from(PAGE_SIZE).ok(),
                ..MessageQuery::default()
            };
            let error = match rest.get_messages(&channel_id.to_string(), &query).await {
                Ok(body) => {
                    let page = parse_messages(&body);
                    lock(&state).messages.load(channel_id, page, mode);
                    None
                }
                Err(err) => Some(err.to_string()),
            };
            let _ = events.send(UiEvent::HistoryLoaded { channel_id, error });
        });
    }

    /// Optimistic send: inserts a pending row now, then POSTs with the nonce.
    /// Returns false if there is no current user yet (before READY).
    pub fn send_message(&self, channel_id: Snowflake, content: String) -> bool {
        let nonce = self.next_nonce();
        {
            let mut state = self.state();
            let Some(author) = state.current_user.clone() else { return false };
            state.messages.insert_pending(channel_id, nonce, content.clone(), author);
        }
        let request = SendMessageRequest { content: Some(content), nonce: nonce.to_string(), ..Default::default() };
        self.post_message(channel_id, nonce, request, |error| UiEvent::SendFailed { error });
        true
    }

    /// `POST /channels/{c}/messages` for the pending row `nonce`: the
    /// response replaces the row, a failure marks it failed and sends
    /// `failed(error)`.
    pub(super) fn post_message(
        &self,
        channel_id: Snowflake,
        nonce: Snowflake,
        request: SendMessageRequest,
        failed: fn(String) -> UiEvent,
    ) {
        #[cfg(test)]
        lock_sent(&self.sent).push((channel_id, request.clone()));
        let _ = self.events.send(UiEvent::StateChanged);
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = rest.send_message_request(&channel_id.to_string(), &request).await;
            let failure = match result {
                Ok(body) => {
                    match serde_json::from_value::<Message>(body) {
                        Ok(message) => {
                            lock(&state).messages.apply_incoming(message);
                        }
                        // Sent; MESSAGE_CREATE will replace the pending row.
                        Err(err) => tracing::warn!(%err, "send response did not parse"),
                    }
                    None
                }
                Err(err) => {
                    lock(&state).messages.mark_failed(nonce);
                    Some(err.to_string())
                }
            };
            if let Some(error) = failure {
                let _ = events.send(failed(error));
            }
            let _ = events.send(UiEvent::StateChanged);
        });
    }

    /// Snowflake from the current time, strictly increasing within this session.
    pub(super) fn next_nonce(&self) -> Snowflake {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or_default();
        let candidate = Snowflake::from_unix_ms(now_ms).0;
        let mut previous = self.last_nonce.load(Ordering::Relaxed);
        loop {
            let next = candidate.max(previous.saturating_add(1));
            match self
                .last_nonce
                .compare_exchange_weak(previous, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return Snowflake(next),
                Err(actual) => previous = actual,
            }
        }
    }

    /// Closes the gateway and sends `POST /auth/logout` in the background
    /// (core clears the client's token either way). A failed call is only
    /// logged: the caller forgets the session regardless.
    pub fn logout(&self) {
        self.close();
        let rest = self.rest.clone();
        self.runtime.spawn(async move {
            if let Err(err) = rest.logout().await {
                tracing::warn!(%err, "logout request failed");
            }
        });
    }

    pub fn close(&self) {
        if let Some(gateway) = &self.gateway {
            gateway.close();
        }
    }
}

/// Skips entries that do not parse instead of failing the whole page.
fn parse_messages(body: &Value) -> Vec<Message> {
    body.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match serde_json::from_value::<Message>(item.clone()) {
                    Ok(message) => Some(message),
                    Err(err) => {
                        tracing::debug!(%err, "skipping malformed message");
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

/// Applies a dispatch; a `MESSAGE_CREATE` also yields the notification it
/// raises (decided against the state that now includes it).
pub(crate) fn apply_dispatch(
    state: &Mutex<ClientState>,
    dispatch: &fluxer_client_core::Dispatch,
    focus: &NotifyFocus,
    now_ms: i64,
) -> (bool, Option<Notification>) {
    let mut state = lock(state);
    let changed = !state.apply(dispatch).is_empty();
    let notification = match dispatch.t.as_str() {
        "MESSAGE_CREATE" => notification_for(&state, &dispatch.d, focus, now_ms),
        _ => None,
    };
    (changed, notification)
}

async fn pump_gateway(
    mut gateway_events: mpsc::Receiver<GatewayEvent>,
    state: Arc<Mutex<ClientState>>,
    events: mpsc::UnboundedSender<UiEvent>,
    focus: Arc<NotifyFocus>,
) {
    let mut closed = false;
    while let Some(event) = gateway_events.recv().await {
        let ui = match event {
            GatewayEvent::Dispatch(dispatch) => {
                let (changed, notification) = apply_dispatch(&state, &dispatch, &focus, now_ms());
                if let Some(notification) = notification
                    && events.send(UiEvent::Notify(notification)).is_err()
                {
                    return;
                }
                if !changed {
                    continue;
                }
                UiEvent::StateChanged
            }
            GatewayEvent::Ready { .. } | GatewayEvent::Resumed => UiEvent::Gateway(GatewayStatus::Ready),
            GatewayEvent::Reconnecting { attempt, .. } => UiEvent::Gateway(GatewayStatus::Reconnecting { attempt }),
            GatewayEvent::InvalidSession { .. } => UiEvent::Gateway(GatewayStatus::Connecting),
            GatewayEvent::Closed { code, reason } => {
                closed = true;
                UiEvent::Gateway(GatewayStatus::Closed { reason: format!("{code} {reason}") })
            }
        };
        if events.send(ui).is_err() {
            return;
        }
    }
    if !closed {
        let _ = events.send(UiEvent::Gateway(GatewayStatus::Closed { reason: "connection ended".to_owned() }));
    }
}
