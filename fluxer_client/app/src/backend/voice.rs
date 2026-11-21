//! Voice session ownership, gateway `VOICE_SERVER_UPDATE` / `VOICE_STATE_UPDATE`
//! routing, and media task lifecycle.
//!
//! Credentials (`VoiceCredentials`) issued by `VOICE_SERVER_UPDATE` are kept
//! strictly private to this module, redacted in `Debug`, and never placed in
//! `ClientState`, `VoiceSnapshot`, `UiEvent`, or logs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use fluxer_client_core::gateway::{GatewayHandle, VoiceStateUpdate};
use fluxer_client_core::rest::SecretToken;
use fluxer_client_core::state::{ClientState, Snowflake, bits};
use fluxer_client_core::voice::{
    DecodedVideoFrame, LatestVideoFrameQueue, ScreenShareCodecTracker, VideoCodec,
};
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use url::Url;

use super::{Backend, UiEvent, lock};

/// Public, UI-safe voice connection phase (contains no credentials).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum VoiceConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed(VoiceFailure),
}

/// Explicit failure reasons for a voice connection attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceFailure {
    UnsupportedE2ee,
    UnsupportedP2pMesh,
    InvalidServerUpdate(&'static str),
    TransportFailed(String),
    GatewayUnavailable,
}

impl fmt::Display for VoiceFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedE2ee => write!(f, "End-to-end encrypted voice channels are not supported yet"),
            Self::UnsupportedP2pMesh => write!(f, "Peer-to-peer mesh voice channels are not supported yet"),
            Self::InvalidServerUpdate(reason) => write!(f, "Invalid voice server update: {reason}"),
            Self::TransportFailed(reason) => write!(f, "Voice transport failed: {reason}"),
            Self::GatewayUnavailable => write!(f, "Gateway connection is not available"),
        }
    }
}

/// Source kind of a subscribed remote track, matching LiveKit `TrackSource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(dead_code)]
pub enum VoiceTrackSource {
    Microphone,
    Camera,
    ScreenShare,
    ScreenShareAudio,
    Unknown,
}

/// Stable ownership key for a remote track subscription.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackSubscriptionKey {
    pub participant_identity: String,
    pub participant_sid: String,
    pub track_sid: String,
    pub source: VoiceTrackSource,
}

/// Public voice presence for one user in a guild or DM voice channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelVoiceMember {
    pub user_id: Snowflake,
    pub guild_id: Option<Snowflake>,
    pub channel_id: Snowflake,
    pub session_id: String,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub self_video: bool,
    pub self_stream: bool,
    pub mute: bool,
    pub deaf: bool,
    pub suppress: bool,
}

/// UI-safe snapshot of the current voice session and channel members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceSnapshot {
    pub state: VoiceConnectionState,
    pub guild_id: Option<Snowflake>,
    pub channel_id: Option<Snowflake>,
    pub connection_id: Option<String>,
    pub attempt_id: u64,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub self_stream: bool,
    pub screen_share_audio: bool,
    pub server_mute: bool,
    pub server_deaf: bool,
    pub speak_denied: bool,
    pub screen_share_codec: ScreenShareCodecTracker,
    pub subscribed_tracks: Vec<TrackSubscriptionKey>,
    pub watched_video_track: Option<TrackSubscriptionKey>,
    pub channel_members: BTreeMap<(Option<Snowflake>, Snowflake), ChannelVoiceMember>,
}

impl Default for VoiceSnapshot {
    fn default() -> Self {
        Self {
            state: VoiceConnectionState::Disconnected,
            guild_id: None,
            channel_id: None,
            connection_id: None,
            attempt_id: 0,
            self_mute: false,
            self_deaf: false,
            self_stream: false,
            screen_share_audio: false,
            server_mute: false,
            server_deaf: false,
            speak_denied: false,
            screen_share_codec: ScreenShareCodecTracker::default(),
            subscribed_tracks: Vec::new(),
            watched_video_track: None,
            channel_members: BTreeMap::new(),
        }
    }
}

impl VoiceSnapshot {
    /// Members currently present in `channel_id`, sorted by user id.
    pub fn members_in_channel(&self, channel_id: Snowflake) -> Vec<ChannelVoiceMember> {
        self.channel_members
            .values()
            .filter(|m| m.channel_id == channel_id)
            .cloned()
            .collect()
    }
}

/// Server-issued LiveKit credentials for a single voice attempt.
/// Never exposed to UI, `ClientState`, or logs.
#[allow(dead_code)]
pub(crate) struct VoiceCredentials {
    pub(crate) attempt_id: u64,
    pub(crate) guild_id: Option<Snowflake>,
    pub(crate) channel_id: Snowflake,
    pub(crate) connection_id: String,
    pub(crate) endpoint: String,
    pub(crate) token: SecretToken,
}

impl fmt::Debug for VoiceCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceCredentials")
            .field("attempt_id", &self.attempt_id)
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .field("connection_id", &self.connection_id)
            .field("endpoint", &"<redacted>")
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Events emitted by an active `VoiceRoom` implementation (e.g. remote track
/// subscription changes or unexpected worker termination).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VoiceRoomEvent {
    TracksChanged,
    Disconnected(String),
}

/// User-configured voice input/output devices and volumes (`0..=200`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceAudioSettings {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub input_volume: u16,
    pub output_volume: u16,
}

impl Default for VoiceAudioSettings {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            input_volume: 100,
            output_volume: 100,
        }
    }
}

impl VoiceAudioSettings {
    fn normalize_device(device: Option<String>) -> Option<String> {
        device.and_then(|d| {
            if d.trim().is_empty() {
                None
            } else {
                Some(d)
            }
        })
    }

    pub(crate) fn normalized(self) -> Self {
        Self {
            input_device: Self::normalize_device(self.input_device),
            output_device: Self::normalize_device(self.output_device),
            input_volume: self.input_volume.min(200),
            output_volume: self.output_volume.min(200),
        }
    }
}

/// Active media room handle owned by the backend session.
pub(crate) trait VoiceRoom: Send + Sync {
    fn set_microphone_muted(&self, muted: bool);
    fn set_playback_muted(&self, muted: bool);
    fn set_screen_share(&self, active: bool, with_audio: bool);
    fn set_audio_settings(&self, _settings: &VoiceAudioSettings) {}
    fn set_watched_video_track(&self, _track_sid: Option<String>) {}
    fn subscribed_tracks(&self) -> Vec<TrackSubscriptionKey> {
        Vec::new()
    }
    fn take_event_rx(&self) -> Option<mpsc::UnboundedReceiver<VoiceRoomEvent>> {
        None
    }
    fn close(&self);
}

/// Factory that turns server-issued credentials into an active `VoiceRoom`.
pub(crate) trait VoiceTransport: Send + Sync {
    fn connect(
        &self,
        credentials: VoiceCredentials,
    ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>>;
}

/// Placeholder transport until the native LiveKit room/audio pipeline is wired.
pub(crate) struct UnconfiguredVoiceTransport;

impl VoiceTransport for UnconfiguredVoiceTransport {
    fn connect(
        &self,
        _credentials: VoiceCredentials,
    ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
        Box::pin(async { Err("native LiveKit voice transport is not wired yet".to_owned()) })
    }
}

/// Room + microphone + screen share + per-track playback tasks owned by the current voice attempt.
struct OwnedVoiceMedia {
    room: Arc<dyn VoiceRoom>,
    mic_task: Option<JoinHandle<()>>,
    screen_share_video_task: Option<JoinHandle<()>>,
    screen_share_audio_task: Option<JoinHandle<()>>,
    event_task: Option<JoinHandle<()>>,
    playback_tasks: BTreeMap<TrackSubscriptionKey, JoinHandle<()>>,
    closed: bool,
}

impl OwnedVoiceMedia {
    fn new(room: Arc<dyn VoiceRoom>) -> Self {
        Self {
            room,
            mic_task: None,
            screen_share_video_task: None,
            screen_share_audio_task: None,
            event_task: None,
            playback_tasks: BTreeMap::new(),
            closed: false,
        }
    }

    fn set_event_task(&mut self, task: JoinHandle<()>) {
        if let Some(prev) = self.event_task.replace(task) {
            prev.abort();
        }
    }

    #[allow(dead_code)]
    fn set_mic_task(&mut self, task: JoinHandle<()>) {
        if let Some(prev) = self.mic_task.replace(task) {
            prev.abort();
        }
    }

    #[allow(dead_code)]
    fn set_screen_share_tasks(
        &mut self,
        video_task: Option<JoinHandle<()>>,
        audio_task: Option<JoinHandle<()>>,
    ) {
        if let Some(prev) = std::mem::replace(&mut self.screen_share_video_task, video_task) {
            prev.abort();
        }
        if let Some(prev) = std::mem::replace(&mut self.screen_share_audio_task, audio_task) {
            prev.abort();
        }
    }

    fn stop_screen_share_audio(&mut self) -> Option<JoinHandle<()>> {
        let task = self.screen_share_audio_task.take()?;
        task.abort();
        Some(task)
    }

    fn stop_screen_share(&mut self) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();
        if let Some(task) = self.screen_share_video_task.take() {
            task.abort();
            handles.push(task);
        }
        if let Some(task) = self.screen_share_audio_task.take() {
            task.abort();
            handles.push(task);
        }
        handles
    }

    #[allow(dead_code)]
    fn insert_playback_task(&mut self, key: TrackSubscriptionKey, task: JoinHandle<()>) {
        if let Some(prev) = self.playback_tasks.insert(key, task) {
            prev.abort();
        }
    }

    #[allow(dead_code)]
    fn remove_playback_task(&mut self, key: &TrackSubscriptionKey) -> bool {
        if let Some(task) = self.playback_tasks.remove(key) {
            task.abort();
            true
        } else {
            false
        }
    }

    fn subscribed_keys(&self) -> Vec<TrackSubscriptionKey> {
        let mut keys: BTreeSet<TrackSubscriptionKey> = self.playback_tasks.keys().cloned().collect();
        keys.extend(self.room.subscribed_tracks());
        keys.into_iter().collect()
    }

    fn shutdown(&mut self) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();
        if let Some(task) = self.mic_task.take() {
            task.abort();
            handles.push(task);
        }
        if let Some(task) = self.event_task.take() {
            task.abort();
            handles.push(task);
        }
        handles.extend(self.stop_screen_share());
        for (_, task) in std::mem::take(&mut self.playback_tasks) {
            task.abort();
            handles.push(task);
        }
        if !self.closed {
            self.closed = true;
            self.room.close();
        }
        handles
    }
}

impl Drop for OwnedVoiceMedia {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

struct ParsedVoiceServerUpdate {
    guild_id: Option<Snowflake>,
    channel_id: Option<Snowflake>,
    endpoint: String,
    token: SecretToken,
    connection_id: String,
    has_e2ee_key: bool,
    has_ice_servers: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVoiceStateUpdate {
    user_id: Snowflake,
    guild_id: Option<Snowflake>,
    channel_id: Option<Snowflake>,
    session_id: String,
    connection_id: Option<String>,
    self_mute: bool,
    self_deaf: bool,
    self_video: bool,
    self_stream: bool,
    mute: bool,
    deaf: bool,
    suppress: bool,
}

/// Backend-owned voice state machine and media handle owner.
pub(crate) struct VoiceManager {
    transport: Arc<dyn VoiceTransport>,
    gateway_session_id: Option<String>,
    state: VoiceConnectionState,
    guild_id: Option<Snowflake>,
    channel_id: Option<Snowflake>,
    connection_id: Option<String>,
    /// Last accepted voice server endpoint (kept private, never exposed in `VoiceSnapshot`).
    endpoint: Option<String>,
    attempt_id: u64,
    self_mute: bool,
    self_deaf: bool,
    self_stream: bool,
    screen_share_audio: bool,
    server_mute: bool,
    server_deaf: bool,
    speak_denied: bool,
    screen_share_codec: ScreenShareCodecTracker,
    audio_settings: VoiceAudioSettings,
    connect_task: Option<JoinHandle<()>>,
    media: Option<OwnedVoiceMedia>,
    watched_video_track: Option<TrackSubscriptionKey>,
    video_frame_queue: LatestVideoFrameQueue,
    channel_members: BTreeMap<(Option<Snowflake>, Snowflake), ChannelVoiceMember>,
}

impl VoiceManager {
    pub(crate) fn new(transport: Arc<dyn VoiceTransport>) -> Self {
        Self {
            transport,
            gateway_session_id: None,
            state: VoiceConnectionState::Disconnected,
            guild_id: None,
            channel_id: None,
            connection_id: None,
            endpoint: None,
            attempt_id: 0,
            self_mute: false,
            self_deaf: false,
            self_stream: false,
            screen_share_audio: false,
            server_mute: false,
            server_deaf: false,
            speak_denied: false,
            screen_share_codec: ScreenShareCodecTracker::default(),
            audio_settings: VoiceAudioSettings::default(),
            connect_task: None,
            media: None,
            watched_video_track: None,
            video_frame_queue: LatestVideoFrameQueue::new(),
            channel_members: BTreeMap::new(),
        }
    }

    pub(crate) fn snapshot(&self) -> VoiceSnapshot {
        VoiceSnapshot {
            state: self.state.clone(),
            guild_id: self.guild_id,
            channel_id: self.channel_id,
            connection_id: self.connection_id.clone(),
            attempt_id: self.attempt_id,
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
            self_stream: self.self_stream,
            screen_share_audio: self.screen_share_audio,
            server_mute: self.server_mute,
            server_deaf: self.server_deaf,
            speak_denied: self.speak_denied,
            screen_share_codec: self.screen_share_codec.clone(),
            subscribed_tracks: self.media.as_ref().map_or_else(Vec::new, OwnedVoiceMedia::subscribed_keys),
            watched_video_track: self.watched_video_track.clone(),
            channel_members: self.channel_members.clone(),
        }
    }

    fn watch_video_track(
        &mut self,
        key: TrackSubscriptionKey,
    ) -> Result<LatestVideoFrameQueue, &'static str> {
        if self.state != VoiceConnectionState::Connected {
            return Err("voice session is not connected");
        }
        if !matches!(key.source, VoiceTrackSource::ScreenShare | VoiceTrackSource::Camera) {
            return Err("only video tracks can be watched");
        }
        let track_sid = key.track_sid.clone();
        self.video_frame_queue.close();
        let queue = LatestVideoFrameQueue::new();
        self.video_frame_queue = queue.clone();
        self.watched_video_track = Some(key);
        if let Some(media) = &self.media {
            media.room.set_watched_video_track(Some(track_sid));
        }
        Ok(queue)
    }

    fn stop_watching_video(&mut self) -> bool {
        self.video_frame_queue.close();
        let cleared = self.watched_video_track.take().is_some();
        if cleared && let Some(media) = &self.media {
            media.room.set_watched_video_track(None);
        }
        cleared
    }

    fn abort_active_media(&mut self) -> Vec<JoinHandle<()>> {
        self.stop_watching_video();
        let mut handles = Vec::new();
        if let Some(task) = self.connect_task.take() {
            task.abort();
            handles.push(task);
        }
        if let Some(mut media) = self.media.take() {
            handles.extend(media.shutdown());
        }
        handles
    }

    fn stop_screen_share_media(&mut self) -> Vec<JoinHandle<()>> {
        self.self_stream = false;
        self.screen_share_audio = false;
        self.screen_share_codec.reset();
        if let Some(media) = &mut self.media {
            let handles = media.stop_screen_share();
            media.room.set_screen_share(false, false);
            handles
        } else {
            Vec::new()
        }
    }

    fn current_self_flags(&self) -> (bool, bool) {
        (self.self_mute, self.self_deaf)
    }

    fn apply_audio_mute_flags(&self) {
        if let Some(media) = &self.media {
            media.room.set_microphone_muted(
                self.self_mute || self.self_deaf || self.server_mute || self.speak_denied,
            );
            media.room.set_playback_muted(self.self_deaf || self.server_deaf);
        }
    }

    /// Starts a new join attempt, or returns `None` if an active attempt for the
    /// exact same `(guild_id, channel_id)` is already in progress or connected
    /// (same-channel retry serialization).
    fn begin_join(&mut self, guild_id: Option<Snowflake>, channel_id: Snowflake) -> Option<(VoiceStateUpdate, Vec<JoinHandle<()>>)> {
        if self.guild_id == guild_id
            && self.channel_id == Some(channel_id)
            && matches!(
                self.state,
                VoiceConnectionState::Connecting
                    | VoiceConnectionState::Connected
                    | VoiceConnectionState::Reconnecting
            )
        {
            return None;
        }
        let handles = self.abort_active_media();
        self.attempt_id = self.attempt_id.saturating_add(1);
        self.guild_id = guild_id;
        self.channel_id = Some(channel_id);
        self.connection_id = None;
        self.endpoint = None;
        self.self_stream = false;
        self.screen_share_audio = false;
        self.server_mute = false;
        self.server_deaf = false;
        self.speak_denied = false;
        self.screen_share_codec.reset();
        self.state = VoiceConnectionState::Connecting;

        let mut payload = VoiceStateUpdate::join(
            guild_id.map(|id| id.to_string()),
            channel_id.to_string(),
            None,
        );
        payload.self_mute = self.self_mute;
        payload.self_deaf = self.self_deaf;
        payload.self_stream = self.self_stream;
        Some((payload, handles))
    }

    fn begin_leave(&mut self) -> (Option<VoiceStateUpdate>, Vec<JoinHandle<()>>) {
        let was_active = self.channel_id.is_some() || self.state != VoiceConnectionState::Disconnected;
        let guild_id = self.guild_id;
        let connection_id = self.connection_id.clone();

        let handles = self.abort_active_media();
        self.attempt_id = self.attempt_id.saturating_add(1);
        self.guild_id = None;
        self.channel_id = None;
        self.connection_id = None;
        self.endpoint = None;
        self.self_stream = false;
        self.screen_share_audio = false;
        self.server_mute = false;
        self.server_deaf = false;
        self.speak_denied = false;
        self.screen_share_codec.reset();
        self.state = VoiceConnectionState::Disconnected;

        let payload = if was_active {
            Some(VoiceStateUpdate::leave(guild_id.map(|id| id.to_string()), connection_id))
        } else {
            None
        };
        (payload, handles)
    }

    fn set_self_mute(&mut self, muted: bool) -> Option<VoiceStateUpdate> {
        self.self_mute = muted;
        self.apply_audio_mute_flags();
        self.current_state_update_payload()
    }

    fn set_self_deaf(&mut self, deafened: bool) -> Option<VoiceStateUpdate> {
        self.self_deaf = deafened;
        self.apply_audio_mute_flags();
        self.current_state_update_payload()
    }

    fn current_state_update_payload(&self) -> Option<VoiceStateUpdate> {
        let channel_id = self.channel_id?;
        let mut payload = VoiceStateUpdate::join(
            self.guild_id.map(|id| id.to_string()),
            channel_id.to_string(),
            self.connection_id.clone(),
        );
        let (self_mute, self_deaf) = self.current_self_flags();
        payload.self_mute = self_mute;
        payload.self_deaf = self_deaf;
        payload.self_stream = self.self_stream;
        Some(payload)
    }

    fn replace_guild_voice_states(&mut self, guild_id: Snowflake, voice_states: &[Value]) {
        self.channel_members.retain(|(g, _), _| *g != Some(guild_id));
        for item in voice_states {
            let Some(mut update) = parse_voice_state_update(item) else {
                continue;
            };
            if update.guild_id.is_none() {
                update.guild_id = Some(guild_id);
            }
            if let Some(channel_id) = update.channel_id {
                self.channel_members.insert(
                    (update.guild_id, update.user_id),
                    ChannelVoiceMember {
                        user_id: update.user_id,
                        guild_id: update.guild_id,
                        channel_id,
                        session_id: update.session_id,
                        self_mute: update.self_mute,
                        self_deaf: update.self_deaf,
                        self_video: update.self_video,
                        self_stream: update.self_stream,
                        mute: update.mute,
                        deaf: update.deaf,
                        suppress: update.suppress,
                    },
                );
            }
        }
    }

    /// Handles a fresh `READY` session. Prunes stale voice members, shuts down
    /// any media from a previous gateway session, and returns a recovery opcode 4
    /// join payload if the user was actively in a voice channel before the drop.
    fn handle_gateway_ready(&mut self, session_id: String) -> (bool, Option<VoiceStateUpdate>, Vec<JoinHandle<()>>) {
        let previous = self.gateway_session_id.replace(session_id);
        let mut changed = !self.channel_members.is_empty();
        self.channel_members.clear();
        if previous.is_some() && self.stop_watching_video() {
            changed = true;
        }
        if previous.is_some()
            && let Some(channel_id) = self.channel_id
            && matches!(
                self.state,
                VoiceConnectionState::Connecting
                    | VoiceConnectionState::Connected
                    | VoiceConnectionState::Reconnecting
            )
        {
            let guild_id = self.guild_id;
            let handles = self.abort_active_media();
            self.attempt_id = self.attempt_id.saturating_add(1);
            self.connection_id = None;
            self.endpoint = None;
            self.self_stream = false;
            self.screen_share_audio = false;
            self.server_mute = false;
            self.server_deaf = false;
            self.speak_denied = false;
            self.screen_share_codec.reset();
            self.state = VoiceConnectionState::Reconnecting;
            let mut payload = VoiceStateUpdate::join(
                guild_id.map(|id| id.to_string()),
                channel_id.to_string(),
                None,
            );
            payload.self_mute = self.self_mute;
            payload.self_deaf = self.self_deaf;
            payload.self_stream = self.self_stream;
            return (true, Some(payload), handles);
        }
        if previous.is_some() && self.media.is_some() {
            let handles = self.abort_active_media();
            self.attempt_id = self.attempt_id.saturating_add(1);
            self.guild_id = None;
            self.channel_id = None;
            self.connection_id = None;
            self.endpoint = None;
            self.self_stream = false;
            self.screen_share_audio = false;
            self.speak_denied = false;
            self.screen_share_codec.reset();
            self.state = VoiceConnectionState::Disconnected;
            changed = true;
            return (changed, None, handles);
        }
        (changed, None, Vec::new())
    }

    fn handle_gateway_reset(&mut self) -> (bool, Vec<JoinHandle<()>>) {
        self.gateway_session_id = None;
        let changed = self.channel_id.is_some()
            || self.media.is_some()
            || self.state != VoiceConnectionState::Disconnected
            || !self.channel_members.is_empty();
        let handles = self.abort_active_media();
        self.attempt_id = self.attempt_id.saturating_add(1);
        self.guild_id = None;
        self.channel_id = None;
        self.connection_id = None;
        self.endpoint = None;
        self.self_stream = false;
        self.screen_share_audio = false;
        self.server_mute = false;
        self.server_deaf = false;
        self.speak_denied = false;
        self.screen_share_codec.reset();
        self.state = VoiceConnectionState::Disconnected;
        self.channel_members.clear();
        (changed, handles)
    }

    fn handle_voice_state_dispatch(
        &mut self,
        payload: &Value,
        current_user_id: Option<Snowflake>,
    ) -> bool {
        let Some(update) = parse_voice_state_update(payload) else {
            return false;
        };
        let key = (update.guild_id, update.user_id);
        match update.channel_id {
            Some(channel_id) => {
                self.channel_members.insert(
                    key,
                    ChannelVoiceMember {
                        user_id: update.user_id,
                        guild_id: update.guild_id,
                        channel_id,
                        session_id: update.session_id.clone(),
                        self_mute: update.self_mute,
                        self_deaf: update.self_deaf,
                        self_video: update.self_video,
                        self_stream: update.self_stream,
                        mute: update.mute,
                        deaf: update.deaf,
                        suppress: update.suppress,
                    },
                );
            }
            None => {
                self.channel_members.remove(&key);
            }
        }

        if current_user_id == Some(update.user_id) {
            let session_matches = self
                .gateway_session_id
                .as_deref()
                .is_none_or(|sid| sid == update.session_id);
            if session_matches {
                self.server_mute = update.mute;
                self.server_deaf = update.deaf;
                self.apply_audio_mute_flags();

                match update.channel_id {
                    None if self.guild_id == update.guild_id && self.channel_id.is_some() => {
                        let conn_matches = match (&self.connection_id, &update.connection_id) {
                            (Some(current), Some(incoming)) => current == incoming,
                            _ => true,
                        };
                        if conn_matches {
                            let _ = self.abort_active_media();
                            self.attempt_id = self.attempt_id.saturating_add(1);
                            self.guild_id = None;
                            self.channel_id = None;
                            self.connection_id = None;
                            self.endpoint = None;
                            self.self_stream = false;
                            self.screen_share_audio = false;
                            self.speak_denied = false;
                            self.screen_share_codec.reset();
                            self.state = VoiceConnectionState::Disconnected;
                        }
                    }
                    Some(new_channel_id)
                        if self.guild_id == update.guild_id
                            && self.channel_id.is_some()
                            && self.channel_id != Some(new_channel_id) =>
                    {
                        self.channel_id = Some(new_channel_id);
                    }
                    _ => {}
                }
            }
        }
        true
    }

    fn accept_voice_server_dispatch(&mut self, payload: &Value) -> Result<Option<VoiceCredentials>, VoiceFailure> {
        let parsed = match parse_voice_server_update(payload) {
            Ok(parsed) => parsed,
            Err(reason) => {
                tracing::warn!(reason, "ignoring malformed VOICE_SERVER_UPDATE");
                return Err(VoiceFailure::InvalidServerUpdate(reason));
            }
        };

        let Some(expected_channel_id) = self.channel_id else {
            return Ok(None);
        };
        if self.guild_id != parsed.guild_id {
            return Ok(None);
        }

        let is_channel_move = parsed.channel_id.is_some_and(|ch| ch != expected_channel_id);
        if is_channel_move && self.state != VoiceConnectionState::Connected {
            return Ok(None);
        }

        if parsed.has_e2ee_key {
            let _ = self.abort_active_media();
            self.attempt_id = self.attempt_id.saturating_add(1);
            self.guild_id = None;
            self.channel_id = None;
            self.connection_id = None;
            self.endpoint = None;
            self.self_stream = false;
            self.screen_share_audio = false;
            self.speak_denied = false;
            self.screen_share_codec.reset();
            self.state = VoiceConnectionState::Failed(VoiceFailure::UnsupportedE2ee);
            return Err(VoiceFailure::UnsupportedE2ee);
        }
        if parsed.has_ice_servers {
            let _ = self.abort_active_media();
            self.attempt_id = self.attempt_id.saturating_add(1);
            self.guild_id = None;
            self.channel_id = None;
            self.connection_id = None;
            self.endpoint = None;
            self.self_stream = false;
            self.screen_share_audio = false;
            self.speak_denied = false;
            self.screen_share_codec.reset();
            self.state = VoiceConnectionState::Failed(VoiceFailure::UnsupportedP2pMesh);
            return Err(VoiceFailure::UnsupportedP2pMesh);
        }

        let resolved_channel_id = parsed.channel_id.unwrap_or(expected_channel_id);
        // Deduplicate repeated VOICE_SERVER_UPDATE frames for the exact same active connection.
        if !is_channel_move
            && matches!(self.state, VoiceConnectionState::Connecting | VoiceConnectionState::Connected)
            && self.connection_id.as_deref() == Some(parsed.connection_id.as_str())
            && self.endpoint.as_deref() == Some(parsed.endpoint.as_str())
        {
            return Ok(None);
        }

        self.channel_id = Some(resolved_channel_id);
        self.connection_id = Some(parsed.connection_id.clone());
        self.endpoint = Some(parsed.endpoint.clone());
        self.state = VoiceConnectionState::Connecting;

        if let Some(task) = self.connect_task.take() {
            task.abort();
        }
        if is_channel_move
            && let Some(mut media) = self.media.take()
        {
            self.stop_watching_video();
            let _ = media.shutdown();
            self.self_stream = false;
            self.screen_share_audio = false;
            self.speak_denied = false;
            self.screen_share_codec.reset();
        }

        Ok(Some(VoiceCredentials {
            attempt_id: self.attempt_id,
            guild_id: parsed.guild_id,
            channel_id: resolved_channel_id,
            connection_id: parsed.connection_id,
            endpoint: parsed.endpoint,
            token: parsed.token,
        }))
    }

    fn finish_connect(
        &mut self,
        attempt_id: u64,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
        connection_id: &str,
        result: Result<Arc<dyn VoiceRoom>, String>,
    ) -> bool {
        let still_current = self.attempt_id == attempt_id
            && self.guild_id == guild_id
            && self.channel_id == Some(channel_id)
            && self.connection_id.as_deref() == Some(connection_id);
        if !still_current {
            if let Ok(room) = result {
                room.close();
            }
            return false;
        }
        self.connect_task = None;
        match result {
            Ok(room) => {
                if let Some(mut previous) = self.media.take() {
                    let _ = previous.shutdown();
                }
                room.set_audio_settings(&self.audio_settings);
                self.media = Some(OwnedVoiceMedia::new(room));
                self.state = VoiceConnectionState::Connected;
                self.apply_audio_mute_flags();
                true
            }
            Err(err) => {
                tracing::warn!(attempt_id, error = %err, "voice room connect failed");
                self.stop_watching_video();
                self.state = VoiceConnectionState::Failed(VoiceFailure::TransportFailed(err));
                true
            }
        }
    }

    fn handle_room_disconnected(
        &mut self,
        attempt_id: u64,
        connection_id: &str,
        reason: String,
    ) -> bool {
        let still_current = self.attempt_id == attempt_id
            && self.connection_id.as_deref() == Some(connection_id)
            && self.state == VoiceConnectionState::Connected;
        if !still_current {
            return false;
        }
        if let Some(mut media) = self.media.take() {
            let _ = media.event_task.take();
            let _ = media.shutdown();
        }
        self.stop_watching_video();
        self.self_stream = false;
        self.screen_share_audio = false;
        self.screen_share_codec.reset();
        self.state = VoiceConnectionState::Failed(VoiceFailure::TransportFailed(reason));
        true
    }

    #[allow(dead_code)]
    pub(crate) fn attach_mic_task(&mut self, attempt_id: u64, task: JoinHandle<()>) -> bool {
        if self.attempt_id != attempt_id {
            task.abort();
            return false;
        }
        let Some(media) = &mut self.media else {
            task.abort();
            return false;
        };
        media.set_mic_task(task);
        true
    }

    #[allow(dead_code)]
    pub(crate) fn attach_screen_share_tasks(
        &mut self,
        attempt_id: u64,
        video_task: Option<JoinHandle<()>>,
        audio_task: Option<JoinHandle<()>>,
    ) -> bool {
        if self.attempt_id != attempt_id {
            if let Some(task) = video_task {
                task.abort();
            }
            if let Some(task) = audio_task {
                task.abort();
            }
            return false;
        }
        let Some(media) = &mut self.media else {
            if let Some(task) = video_task {
                task.abort();
            }
            if let Some(task) = audio_task {
                task.abort();
            }
            return false;
        };
        media.set_screen_share_tasks(video_task, audio_task);
        true
    }

    #[allow(dead_code)]
    pub(crate) fn attach_playback_task(
        &mut self,
        attempt_id: u64,
        key: TrackSubscriptionKey,
        task: JoinHandle<()>,
    ) -> bool {
        if self.attempt_id != attempt_id {
            task.abort();
            return false;
        }
        let Some(media) = &mut self.media else {
            task.abort();
            return false;
        };
        media.insert_playback_task(key, task);
        true
    }

    #[allow(dead_code)]
    pub(crate) fn detach_playback_task(&mut self, key: &TrackSubscriptionKey) -> bool {
        self.media
            .as_mut()
            .is_some_and(|media| media.remove_playback_task(key))
    }
}

fn parse_optional_snowflake(value: Option<&Value>) -> Result<Option<Snowflake>, &'static str> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => Snowflake::parse(raw)
            .map(Some)
            .ok_or("invalid snowflake"),
        Some(_) => Err("snowflake field must be a string or null"),
    }
}

const MAX_VOICE_TOKEN_BYTES: usize = 65_536;
const MAX_CONNECTION_ID_BYTES: usize = 256;

fn parse_voice_server_update(payload: &Value) -> Result<ParsedVoiceServerUpdate, &'static str> {
    let token = payload
        .get("token")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= MAX_VOICE_TOKEN_BYTES)
        .ok_or("missing or oversized token")?;
    let endpoint = payload
        .get("endpoint")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("missing endpoint")?;
    let parsed_url = Url::parse(endpoint).map_err(|_| "invalid endpoint url")?;
    let is_loopback = parsed_url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !(parsed_url.scheme() == "wss" || (parsed_url.scheme() == "ws" && is_loopback)) {
        return Err("voice endpoint requires wss (or loopback ws)");
    }
    if parsed_url.host().is_none()
        || !parsed_url.username().is_empty()
        || parsed_url.password().is_some()
        || parsed_url.query().is_some()
        || parsed_url.fragment().is_some()
    {
        return Err("voice endpoint contains unsupported URL fields");
    }
    let connection_id = payload
        .get("connection_id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= MAX_CONNECTION_ID_BYTES)
        .ok_or("missing or oversized connection_id")?;
    let guild_id = parse_optional_snowflake(payload.get("guild_id"))?;
    let channel_id = parse_optional_snowflake(payload.get("channel_id"))?;
    // Any non-null e2ee_key or ice_servers (including empty array `[]`, which is
    // truthy in the web client) must be rejected before connecting.
    let has_e2ee_key = payload.get("e2ee_key").is_some_and(|v| !v.is_null());
    let has_ice_servers = payload.get("ice_servers").is_some_and(|v| !v.is_null());

    Ok(ParsedVoiceServerUpdate {
        guild_id,
        channel_id,
        endpoint: endpoint.to_owned(),
        token: SecretToken::new(token),
        connection_id: connection_id.to_owned(),
        has_e2ee_key,
        has_ice_servers,
    })
}

fn parse_voice_state_update(payload: &Value) -> Option<ParsedVoiceStateUpdate> {
    let user_id = payload.get("user_id").and_then(Value::as_str).and_then(Snowflake::parse)?;
    let guild_id = parse_optional_snowflake(payload.get("guild_id")).ok()?;
    let channel_id = parse_optional_snowflake(payload.get("channel_id")).ok()?;
    let session_id = payload.get("session_id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let connection_id = payload
        .get("connection_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let flag = |name: &str| payload.get(name).and_then(Value::as_bool).unwrap_or(false);
    Some(ParsedVoiceStateUpdate {
        user_id,
        guild_id,
        channel_id,
        session_id,
        connection_id,
        self_mute: flag("self_mute"),
        self_deaf: flag("self_deaf"),
        self_video: flag("self_video"),
        self_stream: flag("self_stream"),
        mute: flag("mute"),
        deaf: flag("deaf"),
        suppress: flag("suppress"),
    })
}

pub(crate) fn on_gateway_dispatch(
    dispatch: &fluxer_client_core::Dispatch,
    state: &Mutex<ClientState>,
    voice: &Arc<Mutex<VoiceManager>>,
    gateway: Option<&GatewayHandle>,
    runtime: &Handle,
    events: &mpsc::UnboundedSender<UiEvent>,
) {
    on_gateway_dispatch_inner(
        dispatch,
        state,
        voice,
        gateway,
        runtime,
        events,
        #[cfg(test)]
        None,
    );
}

fn on_gateway_dispatch_inner(
    dispatch: &fluxer_client_core::Dispatch,
    state: &Mutex<ClientState>,
    voice: &Arc<Mutex<VoiceManager>>,
    gateway: Option<&GatewayHandle>,
    runtime: &Handle,
    events: &mpsc::UnboundedSender<UiEvent>,
    #[cfg(test)] sent_voice_states: Option<&Arc<Mutex<Vec<VoiceStateUpdate>>>>,
) {
    let record_sent = |payload: &VoiceStateUpdate| {
        #[cfg(test)]
        if let Some(log) = sent_voice_states {
            lock(log).push(payload.clone());
        }
        let _ = payload;
    };
    match dispatch.t.as_str() {
        "READY" => {
            if let Some(guilds) = dispatch.d.get("guilds").and_then(Value::as_array) {
                let mut mgr = lock(voice);
                for guild in guilds {
                    let Some(guild_id) = guild.get("id").and_then(Value::as_str).and_then(Snowflake::parse) else {
                        continue;
                    };
                    if let Some(voice_states) = guild.get("voice_states").and_then(Value::as_array) {
                        mgr.replace_guild_voice_states(guild_id, voice_states);
                    }
                }
            }
        }
        "GUILD_CREATE" | "GUILD_SYNC" => {
            if let Some(guild_id) = dispatch.d.get("id").and_then(Value::as_str).and_then(Snowflake::parse)
                && let Some(voice_states) = dispatch.d.get("voice_states").and_then(Value::as_array)
            {
                lock(voice).replace_guild_voice_states(guild_id, voice_states);
                let _ = events.send(UiEvent::VoiceChanged);
            }
        }
        "GUILD_DELETE" => {
            if let Some(guild_id) = dispatch.d.get("id").and_then(Value::as_str).and_then(Snowflake::parse) {
                let mut mgr = lock(voice);
                mgr.channel_members.retain(|(g, _), _| *g != Some(guild_id));
                if mgr.guild_id == Some(guild_id) && mgr.channel_id.is_some() {
                    let (payload, _handles) = mgr.begin_leave();
                    drop(mgr);
                    if let Some(payload) = payload {
                        let _ = send_voice_gateway_update(gateway, &payload);
                    }
                    let _ = events.send(UiEvent::VoiceChanged);
                }
            }
        }
        "CHANNEL_DELETE" => {
            if let Some(channel_id) = dispatch.d.get("id").and_then(Value::as_str).and_then(Snowflake::parse) {
                let mut mgr = lock(voice);
                mgr.channel_members.retain(|_, m| m.channel_id != channel_id);
                if mgr.channel_id == Some(channel_id) {
                    let (payload, _handles) = mgr.begin_leave();
                    drop(mgr);
                    if let Some(payload) = payload {
                        let _ = send_voice_gateway_update(gateway, &payload);
                    }
                    let _ = events.send(UiEvent::VoiceChanged);
                }
            }
        }
        "CHANNEL_UPDATE" | "GUILD_ROLE_UPDATE" | "GUILD_ROLE_DELETE" | "GUILD_MEMBER_UPDATE" => {
            let active = {
                let mgr = lock(voice);
                match (mgr.guild_id, mgr.channel_id) {
                    (Some(g), Some(c)) => Some((g, c)),
                    _ => None,
                }
            };
            if let Some((guild_id, channel_id)) = active {
                let perms = {
                    let st = lock(state);
                    (st.guilds.contains_key(&guild_id) && st.channels.contains_key(&channel_id))
                        .then(|| st.channel_permissions(channel_id))
                };
                if let Some(perms) = perms {
                    if (perms & (bits::VIEW_CHANNEL | bits::CONNECT))
                        != (bits::VIEW_CHANNEL | bits::CONNECT)
                    {
                        let (payload, _handles) = lock(voice).begin_leave();
                        if let Some(payload) = payload {
                            record_sent(&payload);
                            let _ = send_voice_gateway_update(gateway, &payload);
                        }
                        let _ = events.send(UiEvent::VoiceChanged);
                    } else {
                        let stream_allowed = (perms & bits::STREAM) != 0;
                        let speak_denied = (perms & bits::SPEAK) == 0;
                        let mut changed = false;
                        let mut stream_stop_payload = None;
                        {
                            let mut mgr = lock(voice);
                            if mgr.guild_id == Some(guild_id) && mgr.channel_id == Some(channel_id) {
                                if !stream_allowed && (mgr.self_stream || mgr.screen_share_audio) {
                                    let _handles = mgr.stop_screen_share_media();
                                    stream_stop_payload = mgr.current_state_update_payload();
                                    changed = true;
                                }
                                if mgr.speak_denied != speak_denied {
                                    mgr.speak_denied = speak_denied;
                                    mgr.apply_audio_mute_flags();
                                    changed = true;
                                }
                            }
                        }
                        if let Some(payload) = stream_stop_payload {
                            record_sent(&payload);
                            let _ = send_voice_gateway_update(gateway, &payload);
                        }
                        if changed {
                            let _ = events.send(UiEvent::VoiceChanged);
                        }
                    }
                }
            }
        }
        "VOICE_STATE_UPDATE" => {
            let current_user_id = lock(state).current_user.as_ref().map(|u| u.id);
            let changed = lock(voice).handle_voice_state_dispatch(&dispatch.d, current_user_id);
            if changed {
                let _ = events.send(UiEvent::VoiceChanged);
            }
        }
        "VOICE_SERVER_UPDATE" => {
            let decision = {
                let mut mgr = lock(voice);
                match mgr.accept_voice_server_dispatch(&dispatch.d) {
                    Ok(Some(creds)) => Some((mgr.transport.clone(), creds)),
                    Ok(None) => None,
                    Err(_) => {
                        let _ = events.send(UiEvent::VoiceChanged);
                        None
                    }
                }
            };
            if let Some((transport, creds)) = decision {
                let attempt_id = creds.attempt_id;
                let guild_id = creds.guild_id;
                let channel_id = creds.channel_id;
                let connection_id = creds.connection_id.clone();
                let voice_clone = voice.clone();
                let events_clone = events.clone();
                let conn_id_check = creds.connection_id.clone();
                let task = runtime.spawn(async move {
                    let result = transport.connect(creds).await;
                    let watcher = {
                        let mut mgr = lock(&voice_clone);
                        let changed = mgr.finish_connect(
                            attempt_id,
                            guild_id,
                            channel_id,
                            &connection_id,
                            result,
                        );
                        if !changed {
                            None
                        } else if mgr.state == VoiceConnectionState::Connected
                            && let Some(event_rx) = mgr
                                .media
                                .as_ref()
                                .and_then(|media| media.room.take_event_rx())
                        {
                            let mut rx = event_rx;
                            let voice_for_events = voice_clone.clone();
                            let events_for_watcher = events_clone.clone();
                            let conn_id_for_watcher = connection_id.clone();
                            let handle = tokio::spawn(async move {
                                while let Some(ev) = rx.recv().await {
                                    match ev {
                                        VoiceRoomEvent::TracksChanged => {
                                            if lock(&voice_for_events).attempt_id == attempt_id {
                                                let _ = events_for_watcher.send(UiEvent::VoiceChanged);
                                            } else {
                                                break;
                                            }
                                        }
                                        VoiceRoomEvent::Disconnected(reason) => {
                                            let changed = lock(&voice_for_events)
                                                .handle_room_disconnected(
                                                    attempt_id,
                                                    &conn_id_for_watcher,
                                                    reason,
                                                );
                                            if changed {
                                                let _ = events_for_watcher.send(UiEvent::VoiceChanged);
                                            }
                                            break;
                                        }
                                    }
                                }
                            });
                            if let Some(media) = &mut mgr.media {
                                media.set_event_task(handle);
                            } else {
                                handle.abort();
                            }
                            Some(())
                        } else {
                            Some(())
                        }
                    };
                    if watcher.is_some() {
                        let _ = events_clone.send(UiEvent::VoiceChanged);
                    }
                });
                let mut mgr = lock(voice);
                if mgr.attempt_id == attempt_id && mgr.connection_id.as_deref() == Some(conn_id_check.as_str()) {
                    if let Some(prev) = mgr.connect_task.replace(task) {
                        prev.abort();
                    }
                } else {
                    task.abort();
                }
                let _ = events.send(UiEvent::VoiceChanged);
            }
        }
        _ => {}
    }
}

pub(crate) fn on_gateway_ready(
    session_id: &str,
    voice: &Mutex<VoiceManager>,
    gateway: Option<&GatewayHandle>,
    events: &mpsc::UnboundedSender<UiEvent>,
) {
    let (changed, recovery_payload, _handles) = lock(voice).handle_gateway_ready(session_id.to_owned());
    if let Some(payload) = recovery_payload {
        let _ = send_voice_gateway_update(gateway, &payload);
    }
    if changed {
        let _ = events.send(UiEvent::VoiceChanged);
    }
}

pub(crate) fn on_gateway_reset(
    voice: &Mutex<VoiceManager>,
    events: &mpsc::UnboundedSender<UiEvent>,
) {
    let (changed, _handles) = lock(voice).handle_gateway_reset();
    if changed {
        let _ = events.send(UiEvent::VoiceChanged);
    }
}

fn send_voice_gateway_update(gateway: Option<&GatewayHandle>, payload: &VoiceStateUpdate) -> bool {
    let Some(gateway) = gateway else {
        return false;
    };
    if let Err(err) = gateway.update_voice_state(payload) {
        tracing::warn!(%err, "failed to send gateway VOICE_STATE_UPDATE");
        return false;
    }
    true
}

impl Backend {
    /// Returns a UI-safe snapshot of the current voice session (never includes tokens).
    pub fn voice_snapshot(&self) -> VoiceSnapshot {
        lock(&self.voice).snapshot()
    }

    /// Starts joining a guild or DM voice channel via gateway opcode 4.
    /// Same-channel calls while already connecting/connected/reconnecting are serialized (no-op).
    pub fn join_voice(&self, guild_id: Option<Snowflake>, channel_id: Snowflake) -> bool {
        let Some((payload, _handles)) = lock(&self.voice).begin_join(guild_id, channel_id) else {
            return true;
        };
        #[cfg(test)]
        lock(&self.sent_voice_states).push(payload.clone());
        let sent = if self.gateway.is_some() {
            send_voice_gateway_update(self.gateway.as_ref(), &payload)
        } else {
            #[cfg(test)]
            {
                true
            }
            #[cfg(not(test))]
            {
                false
            }
        };
        if !sent {
            let mut mgr = lock(&self.voice);
            mgr.state = VoiceConnectionState::Failed(VoiceFailure::GatewayUnavailable);
            mgr.channel_id = None;
            mgr.guild_id = None;
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
        sent
    }

    /// Leaves the active voice channel (`channel_id: null`) and shuts down owned media tasks.
    pub fn leave_voice(&self) {
        let (payload, _handles) = lock(&self.voice).begin_leave();
        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Leaves the active voice channel and awaits termination of all owned media/connect tasks.
    #[allow(dead_code)]
    pub async fn leave_voice_and_wait(&self) {
        let (payload, handles) = lock(&self.voice).begin_leave();
        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        for handle in handles {
            let _ = handle.await;
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Updates local self-mute state and forwards opcode 4 if currently in a voice channel.
    pub fn set_voice_mute(&self, muted: bool) {
        let payload = lock(&self.voice).set_self_mute(muted);
        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Updates local self-deaf state and forwards opcode 4 if currently in a voice channel.
    pub fn set_voice_deaf(&self, deafened: bool) {
        let payload = lock(&self.voice).set_self_deaf(deafened);
        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Stores normalized voice device/volume settings and applies them to any active room.
    pub fn set_voice_audio_settings(&self, settings: VoiceAudioSettings) {
        let normalized = settings.normalized();
        let mut mgr = lock(&self.voice);
        mgr.audio_settings = normalized.clone();
        if let Some(media) = &mgr.media {
            media.room.set_audio_settings(&normalized);
        }
    }

    /// Starts or updates screen sharing for the active connected voice session.
    pub fn start_screen_share(&self, include_audio: bool) -> Result<(), &'static str> {
        let (guild_id, channel_id) = {
            let mgr = lock(&self.voice);
            if mgr.state != VoiceConnectionState::Connected {
                return Err("voice session is not connected");
            }
            (mgr.guild_id, mgr.channel_id)
        };

        if let (Some(guild_id), Some(channel_id)) = (guild_id, channel_id) {
            let has_stream = {
                let st = lock(&self.state);
                if st.guilds.contains_key(&guild_id) && st.channels.contains_key(&channel_id) {
                    (st.channel_permissions(channel_id) & bits::STREAM) != 0
                } else {
                    true
                }
            };
            if !has_stream {
                return Err("missing STREAM permission");
            }
        }

        let payload = {
            let mut mgr = lock(&self.voice);
            if mgr.state != VoiceConnectionState::Connected {
                return Err("voice session is not connected");
            }
            mgr.self_stream = true;
            mgr.screen_share_audio = include_audio;
            if let Some(media) = &mut mgr.media {
                if !include_audio {
                    let _ = media.stop_screen_share_audio();
                }
                media.room.set_screen_share(true, include_audio);
            }
            mgr.current_state_update_payload()
        };

        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
        Ok(())
    }

    /// Stops screen sharing (video and audio tasks) while keeping the voice session active.
    pub fn stop_screen_share(&self) {
        let (was_streaming, payload) = {
            let mut mgr = lock(&self.voice);
            let was_streaming = mgr.self_stream || mgr.screen_share_audio;
            let _handles = mgr.stop_screen_share_media();
            let payload = if was_streaming {
                mgr.current_state_update_payload()
            } else {
                None
            };
            (was_streaming, payload)
        };
        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        if was_streaming {
            let _ = self.events.send(UiEvent::VoiceChanged);
        }
    }

    /// Records the user-requested video codec for the current screen-share session.
    #[allow(dead_code)]
    pub fn set_screen_share_requested_codec(&self, codec: VideoCodec) {
        lock(&self.voice).screen_share_codec.set_requested(codec);
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Applies the negotiated screen-share video codec, returning `Ok(true)` if
    /// republishing is required, `Ok(false)` if unchanged, or `Err` when capped.
    #[allow(dead_code)]
    pub fn apply_negotiated_screen_share_codec(
        &self,
        codec: VideoCodec,
    ) -> Result<bool, &'static str> {
        let res = lock(&self.voice).screen_share_codec.apply_negotiated(codec);
        if res == Ok(true) {
            let _ = self.events.send(UiEvent::VoiceChanged);
        }
        res
    }

    /// Records the video codec actually observed/rendered on the screen-share stream.
    #[allow(dead_code)]
    pub fn record_rendered_screen_share_codec(&self, codec: VideoCodec) {
        lock(&self.voice).screen_share_codec.record_rendered(codec);
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Begins watching a remote video track (`ScreenShare` or `Camera`), replacing any
    /// previously watched track and returning a fresh capacity-1 latest-frame queue.
    pub fn watch_video_track(
        &self,
        key: TrackSubscriptionKey,
    ) -> Result<LatestVideoFrameQueue, &'static str> {
        let queue = lock(&self.voice).watch_video_track(key)?;
        let _ = self.events.send(UiEvent::VoiceChanged);
        Ok(queue)
    }

    /// Stops watching the currently watched remote video track and closes its frame queue.
    pub fn stop_watching_video(&self) {
        if lock(&self.voice).stop_watching_video() {
            let _ = self.events.send(UiEvent::VoiceChanged);
        }
    }

    /// Takes the latest decoded video frame if its generation is newer than `last_generation`.
    pub fn latest_video_frame(&self, last_generation: u64) -> Option<(u64, DecodedVideoFrame)> {
        let queue = lock(&self.voice).video_frame_queue.clone();
        queue.take_if_newer(last_generation)
    }

    #[cfg(test)]
    pub(crate) fn push_video_frame_for_test(&self, frame: DecodedVideoFrame) -> bool {
        let queue = lock(&self.voice).video_frame_queue.clone();
        queue.push(frame)
    }

    /// Tears down active voice media on session close/logout.
    pub(super) fn shutdown_voice(&self) {
        self.leave_voice();
    }

    #[cfg(test)]
    pub(crate) fn set_voice_transport_for_test(&self, transport: Arc<dyn VoiceTransport>) {
        lock(&self.voice).transport = transport;
    }

    #[cfg(test)]
    pub(crate) fn feed_voice_dispatch_for_test(&self, dispatch: &fluxer_client_core::Dispatch) {
        on_gateway_dispatch_inner(
            dispatch,
            &self.state,
            &self.voice,
            self.gateway.as_ref(),
            &self.runtime,
            &self.events,
            Some(&self.sent_voice_states),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use fluxer_client_core::Dispatch;
    use fluxer_client_core::rest::{ClientIdentity, Endpoints, RestClient, RestConfig};
    use serde_json::json;
    use tokio::sync::Notify;

    use super::*;

    struct FakeRoom {
        mic_muted: AtomicBool,
        playback_muted: AtomicBool,
        screen_share_active: AtomicBool,
        screen_share_audio: AtomicBool,
        audio_settings: Mutex<Vec<VoiceAudioSettings>>,
        watched_video_tracks: Mutex<Vec<Option<String>>>,
        closed: AtomicBool,
        close_count: AtomicUsize,
    }

    impl FakeRoom {
        fn new() -> Self {
            Self {
                mic_muted: AtomicBool::new(false),
                playback_muted: AtomicBool::new(false),
                screen_share_active: AtomicBool::new(false),
                screen_share_audio: AtomicBool::new(false),
                audio_settings: Mutex::new(Vec::new()),
                watched_video_tracks: Mutex::new(Vec::new()),
                closed: AtomicBool::new(false),
                close_count: AtomicUsize::new(0),
            }
        }
    }

    impl VoiceRoom for FakeRoom {
        fn set_microphone_muted(&self, muted: bool) {
            self.mic_muted.store(muted, Ordering::SeqCst);
        }

        fn set_playback_muted(&self, muted: bool) {
            self.playback_muted.store(muted, Ordering::SeqCst);
        }

        fn set_screen_share(&self, active: bool, with_audio: bool) {
            self.screen_share_active.store(active, Ordering::SeqCst);
            self.screen_share_audio.store(active && with_audio, Ordering::SeqCst);
        }

        fn set_audio_settings(&self, settings: &VoiceAudioSettings) {
            self.audio_settings.lock().unwrap().push(settings.clone());
        }

        fn set_watched_video_track(&self, track_sid: Option<String>) {
            self.watched_video_tracks.lock().unwrap().push(track_sid);
        }

        fn close(&self) {
            self.closed.store(true, Ordering::SeqCst);
            self.close_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct FakeTransport {
        rooms: Mutex<Vec<Arc<FakeRoom>>>,
        gate: Option<Arc<Notify>>,
    }

    impl FakeTransport {
        fn immediate() -> (Arc<Self>, Arc<FakeRoom>) {
            let room = Arc::new(FakeRoom::new());
            let transport = Arc::new(Self { rooms: Mutex::new(vec![room.clone()]), gate: None });
            (transport, room)
        }

        fn gated() -> (Arc<Self>, Arc<FakeRoom>, Arc<Notify>) {
            let room = Arc::new(FakeRoom::new());
            let gate = Arc::new(Notify::new());
            let transport = Arc::new(Self {
                rooms: Mutex::new(vec![room.clone()]),
                gate: Some(gate.clone()),
            });
            (transport, room, gate)
        }
    }

    impl VoiceTransport for FakeTransport {
        fn connect(
            &self,
            _credentials: VoiceCredentials,
        ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
            let room = self.rooms.lock().unwrap().remove(0);
            let gate = self.gate.clone();
            Box::pin(async move {
                if let Some(gate) = gate {
                    gate.notified().await;
                }
                Ok(room as Arc<dyn VoiceRoom>)
            })
        }
    }

    fn test_backend(rt: &tokio::runtime::Runtime) -> (Backend, mpsc::UnboundedReceiver<UiEvent>) {
        let endpoints = Endpoints {
            api_code_version: 1,
            api: "http://127.0.0.1:9/api".into(),
            gateway: "ws://127.0.0.1:9/gateway".into(),
            media: None,
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: None,
            discovery_url: "http://127.0.0.1:9/.well-known/fluxer".into(),
            document: json!({}),
        };
        let mut config = RestConfig::new(&endpoints, ClientIdentity::default());
        config.policy = fluxer_client_core::rest::TransportPolicy { allow_insecure_loopback: true };
        let rest = Arc::new(RestClient::new(config).unwrap());
        Backend::new(rt.handle().clone(), rest, None)
    }

    #[test]
    fn voice_credentials_debug_redacts_token_and_endpoint() {
        let creds = VoiceCredentials {
            attempt_id: 1,
            guild_id: Some(Snowflake(42)),
            channel_id: Snowflake(100),
            connection_id: "conn-1".into(),
            endpoint: "wss://secret-rtc.example.test".into(),
            token: SecretToken::new("super-secret-livekit-jwt"),
        };
        let rendered = format!("{creds:?}");
        assert!(!rendered.contains("super-secret-livekit-jwt"));
        assert!(!rendered.contains("secret-rtc.example.test"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn endpoint_validation_rejects_unsafe_urls_and_empty_ice_servers() {
        for bad_endpoint in [
            "ws://rtc.example.test",
            "https://rtc.example.test",
            "wss://user:secret@rtc.example.test",
            "wss://rtc.example.test?token=secret",
            "wss://rtc.example.test#frag",
        ] {
            let err = parse_voice_server_update(&json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-1",
                "endpoint": bad_endpoint,
                "token": "jwt-secret"
            }))
            .err()
            .expect("bad endpoint must be rejected");
            assert!(!err.contains("secret"));
        }

        // Loopback ws:// and wss:// are accepted.
        for ok_endpoint in ["wss://rtc.example.test/rtc", "ws://127.0.0.1:7880", "ws://[::1]:7880"] {
            assert!(parse_voice_server_update(&json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-1",
                "endpoint": ok_endpoint,
                "token": "jwt-1"
            }))
            .is_ok());
        }

        // Empty ice_servers array `[]` is truthy in JS and must be flagged as P2P mesh.
        let parsed = parse_voice_server_update(&json!({
            "guild_id": null,
            "channel_id": "200",
            "connection_id": "conn-p2p-empty",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-1",
            "ice_servers": []
        }))
        .unwrap();
        assert!(parsed.has_ice_servers);
    }

    #[test]
    fn join_server_update_connect_mute_deaf_and_leave_lifecycle() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let (transport, room) = FakeTransport::immediate();
        backend.set_voice_transport_for_test(transport);

        assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connecting);
        assert_eq!(lock(&backend.sent_voice_states).len(), 1);
        assert_eq!(lock(&backend.sent_voice_states)[0].channel_id.as_deref(), Some("100"));

        // Mismatched guild is ignored.
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 1,
            d: json!({
                "guild_id": "999",
                "channel_id": "100",
                "connection_id": "conn-wrong",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-wrong"
            }),
        });
        assert_eq!(backend.voice_snapshot().connection_id, None);

        // Matching server update connects the transport.
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 2,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-1",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-secret-1"
            }),
        });

        rt.block_on(async {
            for _ in 0..10 {
                if backend.voice_snapshot().state == VoiceConnectionState::Connected {
                    break;
                }
                let _ = rx.recv().await;
            }
        });
        let snap = backend.voice_snapshot();
        assert_eq!(snap.state, VoiceConnectionState::Connected);
        assert_eq!(snap.connection_id.as_deref(), Some("conn-1"));

        // Attach owned mic + playback tasks keyed by participant/track/source.
        let (mic_dropped_tx, mic_dropped_rx) = tokio::sync::oneshot::channel::<()>();
        let mic_task = rt.spawn(async move {
            struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
            impl Drop for DropSignal {
                fn drop(&mut self) {
                    if let Some(tx) = self.0.take() {
                        let _ = tx.send(());
                    }
                }
            }
            let _guard = DropSignal(Some(mic_dropped_tx));
            std::future::pending::<()>().await;
        });
        assert!(lock(&backend.voice).attach_mic_task(snap.attempt_id, mic_task));

        let key = TrackSubscriptionKey {
            participant_identity: "user-2:conn-2".into(),
            participant_sid: "PA_2".into(),
            track_sid: "TR_audio_2".into(),
            source: VoiceTrackSource::Microphone,
        };
        let (playback_dropped_tx, playback_dropped_rx) = tokio::sync::oneshot::channel::<()>();
        let playback_task = rt.spawn(async move {
            struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
            impl Drop for DropSignal {
                fn drop(&mut self) {
                    if let Some(tx) = self.0.take() {
                        let _ = tx.send(());
                    }
                }
            }
            let _guard = DropSignal(Some(playback_dropped_tx));
            std::future::pending::<()>().await;
        });
        assert!(lock(&backend.voice).attach_playback_task(snap.attempt_id, key.clone(), playback_task));
        assert_eq!(backend.voice_snapshot().subscribed_tracks, vec![key.clone()]);

        // Verify explicit per-track detach works too, then re-attach for leave verification.
        let extra_key = TrackSubscriptionKey {
            participant_identity: "user-3:conn-3".into(),
            participant_sid: "PA_3".into(),
            track_sid: "TR_screen_3".into(),
            source: VoiceTrackSource::ScreenShareAudio,
        };
        let extra_task = rt.spawn(std::future::pending::<()>());
        assert!(lock(&backend.voice).attach_playback_task(snap.attempt_id, extra_key.clone(), extra_task));
        assert!(lock(&backend.voice).detach_playback_task(&extra_key));
        assert_eq!(backend.voice_snapshot().subscribed_tracks, vec![key]);

        backend.set_voice_mute(true);
        assert!(room.mic_muted.load(Ordering::SeqCst));
        assert!(!room.playback_muted.load(Ordering::SeqCst));

        backend.set_voice_deaf(true);
        assert!(room.playback_muted.load(Ordering::SeqCst));

        // Leave stops mic + playback tasks, closes room, and sends channel_id: null.
        backend.leave_voice();
        rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(2), mic_dropped_rx)
                .await
                .expect("mic task dropped")
                .expect("mic drop signal");
            tokio::time::timeout(std::time::Duration::from_secs(2), playback_dropped_rx)
                .await
                .expect("playback task dropped")
                .expect("playback drop signal");
        });
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
        assert!(backend.voice_snapshot().subscribed_tracks.is_empty());
        assert!(room.closed.load(Ordering::SeqCst));
        let last_sent = lock(&backend.sent_voice_states).last().unwrap().clone();
        assert_eq!(last_sent.channel_id, None);
        assert_eq!(last_sent.guild_id.as_deref(), Some("42"));
        assert_eq!(last_sent.connection_id.as_deref(), Some("conn-1"));
    }

    #[test]
    fn e2ee_and_p2p_mesh_updates_are_explicitly_rejected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, _rx) = test_backend(&rt);

        backend.join_voice(Some(Snowflake(42)), Snowflake(100));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 1,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-e2ee",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-e2ee",
                "e2ee_key": "base64-key"
            }),
        });
        assert_eq!(
            backend.voice_snapshot().state,
            VoiceConnectionState::Failed(VoiceFailure::UnsupportedE2ee)
        );

        backend.join_voice(None, Snowflake(200));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 2,
            d: json!({
                "guild_id": null,
                "channel_id": "200",
                "connection_id": "conn-p2p",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-p2p",
                "ice_servers": [{"urls": "stun:stun.example.test:3478"}]
            }),
        });
        assert_eq!(
            backend.voice_snapshot().state,
            VoiceConnectionState::Failed(VoiceFailure::UnsupportedP2pMesh)
        );
    }

    #[test]
    fn late_room_completion_after_leave_is_closed_immediately() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, _rx) = test_backend(&rt);
        let (transport, room, gate) = FakeTransport::gated();
        backend.set_voice_transport_for_test(transport);

        backend.join_voice(Some(Snowflake(42)), Snowflake(100));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 1,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-late",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-late"
            }),
        });

        // User leaves while transport.connect is still in flight; then connect resolves.
        let prev_attempt = backend.voice_snapshot().attempt_id;
        let stale_ok = lock(&backend.voice).finish_connect(
            prev_attempt.wrapping_sub(1),
            Some(Snowflake(42)),
            Snowflake(100),
            "conn-late",
            Ok(room.clone()),
        );
        assert!(!stale_ok);
        assert!(room.closed.load(Ordering::SeqCst));

        backend.leave_voice();
        gate.notify_one();
        rt.block_on(tokio::task::yield_now());
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    }

    #[test]
    fn voice_state_dispatch_tracks_members_and_server_disconnect() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let (transport, room) = FakeTransport::immediate();
        backend.set_voice_transport_for_test(transport);

        // Seed current user id = 10 and gateway session id = "sess-1".
        backend.state().apply(&Dispatch {
            t: "READY".into(),
            s: 1,
            d: json!({
                "session_id": "sess-1",
                "user": { "id": "10", "username": "me" },
                "guilds": []
            }),
        });
        on_gateway_ready("sess-1", &backend.voice, None, &backend.events);

        backend.join_voice(Some(Snowflake(42)), Snowflake(100));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 2,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-1",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-1"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv())
                    .await
                    .expect("connected event timeout")
                    .expect("event channel open");
            }
        });
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connected);

        // Another member joins the same channel.
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_STATE_UPDATE".into(),
            s: 3,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "user_id": "20",
                "session_id": "sess-2",
                "self_mute": false,
                "self_deaf": false
            }),
        });
        assert_eq!(backend.voice_snapshot().members_in_channel(Snowflake(100)).len(), 1);

        // Server disconnects our own user from the voice channel.
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_STATE_UPDATE".into(),
            s: 4,
            d: json!({
                "guild_id": "42",
                "channel_id": null,
                "user_id": "10",
                "session_id": "sess-1",
                "connection_id": "conn-1"
            }),
        });
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
        assert!(room.closed.load(Ordering::SeqCst));
    }

    #[test]
    fn same_channel_join_is_serialized_and_duplicate_server_update_is_ignored() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let room1 = Arc::new(FakeRoom::new());
        let room2 = Arc::new(FakeRoom::new());
        let connect_calls = Arc::new(AtomicUsize::new(0));
        struct CountingTransport {
            rooms: Mutex<Vec<Arc<FakeRoom>>>,
            calls: Arc<AtomicUsize>,
        }
        impl VoiceTransport for CountingTransport {
            fn connect(
                &self,
                _credentials: VoiceCredentials,
            ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let room = self.rooms.lock().unwrap().remove(0);
                Box::pin(async move { Ok(room as Arc<dyn VoiceRoom>) })
            }
        }
        backend.set_voice_transport_for_test(Arc::new(CountingTransport {
            rooms: Mutex::new(vec![room1.clone(), room2.clone()]),
            calls: connect_calls.clone(),
        }));

        assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
        let first_attempt = backend.voice_snapshot().attempt_id;
        // Repeated join to the same channel while connecting is serialized (no-op).
        assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
        assert_eq!(backend.voice_snapshot().attempt_id, first_attempt);
        assert_eq!(lock(&backend.sent_voice_states).len(), 1);

        let server_update = Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 1,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-dup",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-1"
            }),
        };
        backend.feed_voice_dispatch_for_test(&server_update);
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });
        assert_eq!(connect_calls.load(Ordering::SeqCst), 1);

        // Duplicate VOICE_SERVER_UPDATE with identical connection_id and endpoint is ignored.
        backend.feed_voice_dispatch_for_test(&server_update);
        assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
        assert!(!room1.closed.load(Ordering::SeqCst));

        // Repeated join while already Connected is also a no-op.
        assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
        assert_eq!(backend.voice_snapshot().attempt_id, first_attempt);
        assert_eq!(lock(&backend.sent_voice_states).len(), 1);
    }

    #[test]
    fn out_of_order_server_update_after_channel_move_is_ignored_and_old_room_closed() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let room_a = Arc::new(FakeRoom::new());
        let room_b = Arc::new(FakeRoom::new());
        let transport = Arc::new(FakeTransport {
            rooms: Mutex::new(vec![room_a.clone(), room_b.clone()]),
            gate: None,
        });
        backend.set_voice_transport_for_test(transport);

        backend.join_voice(Some(Snowflake(42)), Snowflake(100));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 1,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-a",
                "endpoint": "wss://rtc-a.example.test",
                "token": "jwt-a"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });

        // Move to channel 200 -> closes room_a immediately.
        backend.join_voice(Some(Snowflake(42)), Snowflake(200));
        assert!(room_a.closed.load(Ordering::SeqCst));
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connecting);

        // Delayed / out-of-order VOICE_SERVER_UPDATE for channel 100 is ignored while connecting to 200.
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 2,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-a-delayed",
                "endpoint": "wss://rtc-a.example.test",
                "token": "jwt-a-delayed"
            }),
        });
        assert_eq!(backend.voice_snapshot().channel_id, Some(Snowflake(200)));
        assert_eq!(backend.voice_snapshot().connection_id, None);

        // Now channel 200 server update arrives and connects room_b.
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 3,
            d: json!({
                "guild_id": "42",
                "channel_id": "200",
                "connection_id": "conn-b",
                "endpoint": "wss://rtc-b.example.test",
                "token": "jwt-b"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });
        assert_eq!(backend.voice_snapshot().connection_id.as_deref(), Some("conn-b"));
        assert!(!room_b.closed.load(Ordering::SeqCst));
    }

    #[test]
    fn ready_recovers_voice_and_prunes_ghosts_while_invalid_session_resets() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let (transport, room) = FakeTransport::immediate();
        backend.set_voice_transport_for_test(transport);

        on_gateway_ready("sess-1", &backend.voice, None, &backend.events);
        backend.join_voice(Some(Snowflake(42)), Snowflake(100));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 1,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-1",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-1"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });

        // Ghost participant present before reconnect.
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_STATE_UPDATE".into(),
            s: 2,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "user_id": "99",
                "session_id": "ghost-sess"
            }),
        });
        assert_eq!(backend.voice_snapshot().members_in_channel(Snowflake(100)).len(), 1);

        // Fresh READY with a new session prunes ghosts, closes old room, and enters Reconnecting.
        on_gateway_ready("sess-2", &backend.voice, None, &backend.events);
        assert!(room.closed.load(Ordering::SeqCst));
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Reconnecting);
        assert!(backend.voice_snapshot().members_in_channel(Snowflake(100)).is_empty());

        // Hydrating READY guild voice_states populates only live members.
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "READY".into(),
            s: 1,
            d: json!({
                "session_id": "sess-2",
                "guilds": [{
                    "id": "42",
                    "voice_states": [{
                        "channel_id": "100",
                        "user_id": "55",
                        "session_id": "live-sess"
                    }]
                }]
            }),
        });
        let members = backend.voice_snapshot().members_in_channel(Snowflake(100));
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].user_id, Snowflake(55));

        // Non-resumable INVALID_SESSION clears voice state and members.
        on_gateway_reset(&backend.voice, &backend.events);
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
        assert!(backend.voice_snapshot().channel_members.is_empty());
    }

    #[test]
    fn permission_revocation_and_channel_delete_close_room_and_stop_tasks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let (transport, room) = FakeTransport::immediate();
        backend.set_voice_transport_for_test(transport);

        // Seed guild 42, non-owner user 10, @everyone role 42 with VIEW_CHANNEL | CONNECT, voice channel 100.
        let allow_view_connect = (bits::VIEW_CHANNEL | bits::CONNECT).to_string();
        let ready = Dispatch {
            t: "READY".into(),
            s: 1,
            d: json!({
                "session_id": "sess-1",
                "user": { "id": "10", "username": "me" },
                "guilds": [{
                    "id": "42",
                    "properties": { "name": "Test", "owner_id": "1" },
                    "roles": [{ "id": "42", "name": "@everyone", "permissions": allow_view_connect, "position": 0 }],
                    "members": [{ "user": { "id": "10", "username": "me" }, "roles": [] }],
                    "channels": [{ "id": "100", "guild_id": "42", "type": 2, "name": "Voice", "permission_overwrites": [] }]
                }]
            }),
        };
        backend.state().apply(&ready);
        on_gateway_ready("sess-1", &backend.voice, None, &backend.events);

        backend.join_voice(Some(Snowflake(42)), Snowflake(100));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 2,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-perm",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-perm"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connected);

        // Revoke CONNECT permission via CHANNEL_UPDATE overwrite on @everyone.
        let deny_connect = bits::CONNECT.to_string();
        let channel_update = Dispatch {
            t: "CHANNEL_UPDATE".into(),
            s: 3,
            d: json!({
                "id": "100",
                "guild_id": "42",
                "type": 2,
                "name": "Voice",
                "permission_overwrites": [{
                    "id": "42",
                    "type": 0,
                    "allow": "0",
                    "deny": deny_connect
                }]
            }),
        };
        backend.state().apply(&channel_update);
        backend.feed_voice_dispatch_for_test(&channel_update);

        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
        assert!(room.closed.load(Ordering::SeqCst));
    }

    #[test]
    fn leave_voice_and_wait_awaits_task_shutdown() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let (transport, room) = FakeTransport::immediate();
        backend.set_voice_transport_for_test(transport);

        backend.join_voice(Some(Snowflake(42)), Snowflake(100));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 1,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-wait",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-wait"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });

        let dropped = Arc::new(AtomicBool::new(false));
        let flag = dropped.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let attempt_id = backend.voice_snapshot().attempt_id;
        let task = rt.spawn(async move {
            struct Guard(Arc<AtomicBool>);
            impl Drop for Guard {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            let _g = Guard(flag);
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        rt.block_on(started_rx).unwrap();
        assert!(lock(&backend.voice).attach_mic_task(attempt_id, task));

        rt.block_on(backend.leave_voice_and_wait());
        assert!(dropped.load(Ordering::SeqCst));
        assert!(room.closed.load(Ordering::SeqCst));
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    }

    fn seed_guild_voice_channel_and_connect(
        rt: &tokio::runtime::Runtime,
        backend: &Backend,
        rx: &mut mpsc::UnboundedReceiver<UiEvent>,
        permissions: u64,
    ) -> Arc<FakeRoom> {
        let (transport, room) = FakeTransport::immediate();
        backend.set_voice_transport_for_test(transport);

        let allow = permissions.to_string();
        let ready = Dispatch {
            t: "READY".into(),
            s: 1,
            d: json!({
                "session_id": "sess-1",
                "user": { "id": "10", "username": "me" },
                "guilds": [{
                    "id": "42",
                    "properties": { "name": "Test", "owner_id": "1" },
                    "roles": [{ "id": "42", "name": "@everyone", "permissions": allow, "position": 0 }],
                    "members": [{ "user": { "id": "10", "username": "me" }, "roles": [] }],
                    "channels": [{ "id": "100", "guild_id": "42", "type": 2, "name": "Voice", "permission_overwrites": [] }]
                }]
            }),
        };
        backend.state().apply(&ready);
        on_gateway_ready("sess-1", &backend.voice, None, &backend.events);

        assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 2,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-perm",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-perm"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });
        room
    }

    fn spawn_drop_flag_task(
        rt: &tokio::runtime::Runtime,
    ) -> (JoinHandle<()>, Arc<AtomicBool>, tokio::sync::oneshot::Receiver<()>) {
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = dropped.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel::<()>();
        let task = rt.spawn(async move {
            struct Guard(Arc<AtomicBool>, Option<tokio::sync::oneshot::Sender<()>>);
            impl Drop for Guard {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                    if let Some(tx) = self.1.take() {
                        let _ = tx.send(());
                    }
                }
            }
            let _g = Guard(flag, Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        rt.block_on(started_rx).unwrap();
        (task, dropped, dropped_rx)
    }

    fn await_task_drop(rt: &tokio::runtime::Runtime, rx: tokio::sync::oneshot::Receiver<()>) {
        rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("task drop timeout")
                .expect("task drop signal");
        });
    }

    #[test]
    fn start_screen_share_requires_stream_permission_and_sends_no_opcode4() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let room = seed_guild_voice_channel_and_connect(
            &rt,
            &backend,
            &mut rx,
            bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
        );

        let sent_before = lock(&backend.sent_voice_states).len();
        assert!(backend.start_screen_share(true).is_err());

        let snap = backend.voice_snapshot();
        assert!(!snap.self_stream);
        assert!(!snap.screen_share_audio);
        assert!(!room.screen_share_active.load(Ordering::SeqCst));
        assert!(!room.screen_share_audio.load(Ordering::SeqCst));
        assert_eq!(lock(&backend.sent_voice_states).len(), sent_before);
    }

    #[test]
    fn screen_share_separates_video_and_audio_and_prevents_stale_audio() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let room = seed_guild_voice_channel_and_connect(
            &rt,
            &backend,
            &mut rx,
            bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM,
        );
        let attempt_id = backend.voice_snapshot().attempt_id;

        // Stale attempt_id must abort tasks immediately and return false.
        let (stale_v, stale_v_dropped, stale_v_rx) = spawn_drop_flag_task(&rt);
        let (stale_a, stale_a_dropped, stale_a_rx) = spawn_drop_flag_task(&rt);
        assert!(!lock(&backend.voice).attach_screen_share_tasks(
            attempt_id.wrapping_sub(1),
            Some(stale_v),
            Some(stale_a),
        ));
        await_task_drop(&rt, stale_v_rx);
        await_task_drop(&rt, stale_a_rx);
        assert!(stale_v_dropped.load(Ordering::SeqCst));
        assert!(stale_a_dropped.load(Ordering::SeqCst));

        // Start with audio enabled.
        assert!(backend.start_screen_share(true).is_ok());
        let snap = backend.voice_snapshot();
        assert!(snap.self_stream);
        assert!(snap.screen_share_audio);
        assert!(room.screen_share_active.load(Ordering::SeqCst));
        assert!(room.screen_share_audio.load(Ordering::SeqCst));
        assert!(lock(&backend.sent_voice_states).last().unwrap().self_stream);

        let (video_task, video_dropped, video_rx) = spawn_drop_flag_task(&rt);
        let (audio_task, audio_dropped, audio_rx) = spawn_drop_flag_task(&rt);
        assert!(lock(&backend.voice).attach_screen_share_tasks(
            attempt_id,
            Some(video_task),
            Some(audio_task),
        ));

        // Switching to include_audio == false aborts the audio task while keeping video alive.
        assert!(backend.start_screen_share(false).is_ok());
        await_task_drop(&rt, audio_rx);
        assert!(audio_dropped.load(Ordering::SeqCst));
        assert!(!video_dropped.load(Ordering::SeqCst));
        let snap = backend.voice_snapshot();
        assert!(snap.self_stream);
        assert!(!snap.screen_share_audio);
        assert!(room.screen_share_active.load(Ordering::SeqCst));
        assert!(!room.screen_share_audio.load(Ordering::SeqCst));

        // Stopping screen share aborts video too and sends self_stream: false.
        backend.stop_screen_share();
        await_task_drop(&rt, video_rx);
        assert!(video_dropped.load(Ordering::SeqCst));
        let snap = backend.voice_snapshot();
        assert!(!snap.self_stream);
        assert!(!snap.screen_share_audio);
        assert!(!room.screen_share_active.load(Ordering::SeqCst));
        assert!(!lock(&backend.sent_voice_states).last().unwrap().self_stream);
    }

    #[test]
    fn revoking_stream_permission_stops_screen_share_only_and_keeps_voice() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let room = seed_guild_voice_channel_and_connect(
            &rt,
            &backend,
            &mut rx,
            bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM,
        );
        let attempt_id = backend.voice_snapshot().attempt_id;

        let (mic_task, mic_dropped, _mic_rx) = spawn_drop_flag_task(&rt);
        assert!(lock(&backend.voice).attach_mic_task(attempt_id, mic_task));

        assert!(backend.start_screen_share(true).is_ok());
        let (video_task, video_dropped, video_rx) = spawn_drop_flag_task(&rt);
        let (audio_task, audio_dropped, audio_rx) = spawn_drop_flag_task(&rt);
        assert!(lock(&backend.voice).attach_screen_share_tasks(
            attempt_id,
            Some(video_task),
            Some(audio_task),
        ));

        let deny_stream = bits::STREAM.to_string();
        let channel_update = Dispatch {
            t: "CHANNEL_UPDATE".into(),
            s: 3,
            d: json!({
                "id": "100",
                "guild_id": "42",
                "type": 2,
                "name": "Voice",
                "permission_overwrites": [{
                    "id": "42",
                    "type": 0,
                    "allow": "0",
                    "deny": deny_stream
                }]
            }),
        };
        backend.state().apply(&channel_update);
        backend.feed_voice_dispatch_for_test(&channel_update);

        await_task_drop(&rt, video_rx);
        await_task_drop(&rt, audio_rx);

        assert!(video_dropped.load(Ordering::SeqCst));
        assert!(audio_dropped.load(Ordering::SeqCst));
        assert!(!mic_dropped.load(Ordering::SeqCst));

        let snap = backend.voice_snapshot();
        assert_eq!(snap.state, VoiceConnectionState::Connected);
        assert!(!snap.self_stream);
        assert!(!snap.screen_share_audio);
        assert!(!room.closed.load(Ordering::SeqCst));
        assert!(!room.screen_share_active.load(Ordering::SeqCst));
        assert!(!room.screen_share_audio.load(Ordering::SeqCst));

        let last_sent = lock(&backend.sent_voice_states).last().unwrap().clone();
        assert_eq!(last_sent.channel_id.as_deref(), Some("100"));
        assert!(!last_sent.self_stream);
    }

    #[test]
    fn revoking_and_restoring_speak_permission_mutes_mic_without_leaving() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let room = seed_guild_voice_channel_and_connect(
            &rt,
            &backend,
            &mut rx,
            bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
        );

        assert!(!backend.voice_snapshot().speak_denied);
        assert!(!room.mic_muted.load(Ordering::SeqCst));

        let deny_speak = bits::SPEAK.to_string();
        let revoke_update = Dispatch {
            t: "CHANNEL_UPDATE".into(),
            s: 3,
            d: json!({
                "id": "100",
                "guild_id": "42",
                "type": 2,
                "name": "Voice",
                "permission_overwrites": [{
                    "id": "42",
                    "type": 0,
                    "allow": "0",
                    "deny": deny_speak
                }]
            }),
        };
        backend.state().apply(&revoke_update);
        backend.feed_voice_dispatch_for_test(&revoke_update);

        let snap = backend.voice_snapshot();
        assert_eq!(snap.state, VoiceConnectionState::Connected);
        assert!(snap.speak_denied);
        assert!(room.mic_muted.load(Ordering::SeqCst));
        assert!(!room.closed.load(Ordering::SeqCst));

        let restore_update = Dispatch {
            t: "CHANNEL_UPDATE".into(),
            s: 4,
            d: json!({
                "id": "100",
                "guild_id": "42",
                "type": 2,
                "name": "Voice",
                "permission_overwrites": []
            }),
        };
        backend.state().apply(&restore_update);
        backend.feed_voice_dispatch_for_test(&restore_update);

        let snap = backend.voice_snapshot();
        assert_eq!(snap.state, VoiceConnectionState::Connected);
        assert!(!snap.speak_denied);
        assert!(!room.mic_muted.load(Ordering::SeqCst));
    }

    #[test]
    fn leave_voice_and_wait_awaits_mic_screen_share_and_playback_tasks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let room = seed_guild_voice_channel_and_connect(
            &rt,
            &backend,
            &mut rx,
            bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM,
        );
        let attempt_id = backend.voice_snapshot().attempt_id;

        let (mic_task, mic_dropped, _mic_rx) = spawn_drop_flag_task(&rt);
        let (video_task, video_dropped, _video_rx) = spawn_drop_flag_task(&rt);
        let (audio_task, audio_dropped, _audio_rx) = spawn_drop_flag_task(&rt);
        let (playback_task, playback_dropped, _playback_rx) = spawn_drop_flag_task(&rt);

        let key = TrackSubscriptionKey {
            participant_identity: "user-2:conn-2".into(),
            participant_sid: "PA_2".into(),
            track_sid: "TR_audio_2".into(),
            source: VoiceTrackSource::Microphone,
        };
        assert!(lock(&backend.voice).attach_mic_task(attempt_id, mic_task));
        assert!(lock(&backend.voice).attach_screen_share_tasks(
            attempt_id,
            Some(video_task),
            Some(audio_task),
        ));
        assert!(lock(&backend.voice).attach_playback_task(attempt_id, key, playback_task));

        rt.block_on(backend.leave_voice_and_wait());

        assert!(mic_dropped.load(Ordering::SeqCst));
        assert!(video_dropped.load(Ordering::SeqCst));
        assert!(audio_dropped.load(Ordering::SeqCst));
        assert!(playback_dropped.load(Ordering::SeqCst));
        assert!(room.closed.load(Ordering::SeqCst));
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    }

    #[test]
    fn room_disconnected_event_fails_connected_session_unless_already_left() {
        struct EventRoom {
            event_rx: Mutex<Option<mpsc::UnboundedReceiver<VoiceRoomEvent>>>,
            closed: AtomicBool,
        }
        impl VoiceRoom for EventRoom {
            fn set_microphone_muted(&self, _muted: bool) {}
            fn set_playback_muted(&self, _muted: bool) {}
            fn set_screen_share(&self, _active: bool, _with_audio: bool) {}
            fn take_event_rx(&self) -> Option<mpsc::UnboundedReceiver<VoiceRoomEvent>> {
                self.event_rx.lock().unwrap().take()
            }
            fn close(&self) {
                self.closed.store(true, Ordering::SeqCst);
            }
        }
        struct EventTransport {
            rooms: Mutex<Vec<Arc<EventRoom>>>,
        }
        impl VoiceTransport for EventTransport {
            fn connect(
                &self,
                _credentials: VoiceCredentials,
            ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
                let room = self.rooms.lock().unwrap().remove(0);
                Box::pin(async move { Ok(room as Arc<dyn VoiceRoom>) })
            }
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);

        let (ev_tx1, ev_rx1) = mpsc::unbounded_channel();
        let room1 = Arc::new(EventRoom {
            event_rx: Mutex::new(Some(ev_rx1)),
            closed: AtomicBool::new(false),
        });
        let (ev_tx2, ev_rx2) = mpsc::unbounded_channel();
        let room2 = Arc::new(EventRoom {
            event_rx: Mutex::new(Some(ev_rx2)),
            closed: AtomicBool::new(false),
        });
        backend.set_voice_transport_for_test(Arc::new(EventTransport {
            rooms: Mutex::new(vec![room1.clone(), room2.clone()]),
        }));

        // 1. Disconnected event while Connected transitions to Failed(TransportFailed) and emits VoiceChanged.
        assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 1,
            d: json!({
                "guild_id": "42",
                "channel_id": "100",
                "connection_id": "conn-ev-1",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-ev-1"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });

        assert!(backend.start_screen_share(true).is_ok());
        while rx.try_recv().is_ok() {}

        ev_tx1
            .send(VoiceRoomEvent::Disconnected("worker crashed".to_owned()))
            .unwrap();
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
                if matches!(
                    backend.voice_snapshot().state,
                    VoiceConnectionState::Failed(VoiceFailure::TransportFailed(_))
                ) {
                    break;
                }
            }
        });
        let snap = backend.voice_snapshot();
        assert_eq!(
            snap.state,
            VoiceConnectionState::Failed(VoiceFailure::TransportFailed("worker crashed".to_owned()))
        );
        assert!(!snap.self_stream);
        assert!(!snap.screen_share_audio);
        assert!(room1.closed.load(Ordering::SeqCst));

        // 2. Late Disconnected event after leave_voice() is ignored.
        assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(101)));
        backend.feed_voice_dispatch_for_test(&Dispatch {
            t: "VOICE_SERVER_UPDATE".into(),
            s: 2,
            d: json!({
                "guild_id": "42",
                "channel_id": "101",
                "connection_id": "conn-ev-2",
                "endpoint": "wss://rtc.example.test",
                "token": "jwt-ev-2"
            }),
        });
        rt.block_on(async {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while backend.voice_snapshot().state != VoiceConnectionState::Connected {
                tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            }
        });
        backend.leave_voice();
        let _ = ev_tx2.send(VoiceRoomEvent::Disconnected("late disconnect".to_owned()));
        rt.block_on(tokio::task::yield_now());
        assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    }

    #[test]
    fn screen_share_codec_tracker_tracks_stages_and_resets_on_stop_and_leave() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);
        let _room = seed_guild_voice_channel_and_connect(
            &rt,
            &backend,
            &mut rx,
            bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM,
        );

        assert!(backend.start_screen_share(true).is_ok());

        backend.set_screen_share_requested_codec(VideoCodec::H265);
        assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::H264), Ok(true));
        assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::H264), Ok(false));
        backend.record_rendered_screen_share_codec(VideoCodec::H264);

        let snap = backend.voice_snapshot();
        assert_eq!(snap.screen_share_codec.requested, Some(VideoCodec::H265));
        assert_eq!(snap.screen_share_codec.negotiated, Some(VideoCodec::H264));
        assert_eq!(snap.screen_share_codec.rendered, Some(VideoCodec::H264));
        assert_eq!(snap.screen_share_codec.republish_count, 0);

        // Exhaust republish budget (MAX_CODEC_REPUBLISH_ATTEMPTS = 3).
        assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::Vp9), Ok(true));
        assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::Vp8), Ok(true));
        assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::Av1), Ok(true));
        assert!(backend.apply_negotiated_screen_share_codec(VideoCodec::H264).is_err());

        // Stopping screen share resets codec tracker.
        backend.stop_screen_share();
        assert_eq!(
            backend.voice_snapshot().screen_share_codec,
            ScreenShareCodecTracker::default()
        );

        // Leaving voice also resets codec tracker.
        assert!(backend.start_screen_share(false).is_ok());
        backend.set_screen_share_requested_codec(VideoCodec::Av1);
        assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::Vp8), Ok(true));
        backend.record_rendered_screen_share_codec(VideoCodec::Vp8);
        backend.leave_voice();
        assert_eq!(
            backend.voice_snapshot().screen_share_codec,
            ScreenShareCodecTracker::default()
        );
    }

    #[test]
    fn set_voice_audio_settings_applies_on_connect_and_updates_active_room() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);

        // Pre-connect settings are normalized and retained until room connects.
        backend.set_voice_audio_settings(VoiceAudioSettings {
            input_device: Some("   ".to_owned()),
            output_device: Some("Headset".to_owned()),
            input_volume: 250,
            output_volume: 300,
        });

        let room = seed_guild_voice_channel_and_connect(
            &rt,
            &backend,
            &mut rx,
            bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
        );
        let expected_initial = VoiceAudioSettings {
            input_device: None,
            output_device: Some("Headset".to_owned()),
            input_volume: 200,
            output_volume: 200,
        };
        assert_eq!(*room.audio_settings.lock().unwrap(), vec![expected_initial.clone()]);

        // Live update while connected immediately propagates to the active room.
        backend.set_voice_audio_settings(VoiceAudioSettings {
            input_device: Some("USB Mic".to_owned()),
            output_device: Some(String::new()),
            input_volume: 75,
            output_volume: 150,
        });
        let expected_updated = VoiceAudioSettings {
            input_device: Some("USB Mic".to_owned()),
            output_device: None,
            input_volume: 75,
            output_volume: 150,
        };
        assert_eq!(
            *room.audio_settings.lock().unwrap(),
            vec![expected_initial, expected_updated]
        );
    }

    #[test]
    fn watch_video_track_validates_source_replaces_queue_and_clears_on_stop_or_leave() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (backend, mut rx) = test_backend(&rt);

        let screen_key = TrackSubscriptionKey {
            participant_identity: "user-2:conn-2".into(),
            participant_sid: "PA_2".into(),
            track_sid: "TR_screen_2".into(),
            source: VoiceTrackSource::ScreenShare,
        };
        assert!(backend.watch_video_track(screen_key.clone()).is_err());

        let room = seed_guild_voice_channel_and_connect(
            &rt,
            &backend,
            &mut rx,
            bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
        );

        let mic_key = TrackSubscriptionKey {
            participant_identity: "user-2:conn-2".into(),
            participant_sid: "PA_2".into(),
            track_sid: "TR_mic_2".into(),
            source: VoiceTrackSource::Microphone,
        };
        let screen_audio_key = TrackSubscriptionKey {
            participant_identity: "user-2:conn-2".into(),
            participant_sid: "PA_2".into(),
            track_sid: "TR_sa_2".into(),
            source: VoiceTrackSource::ScreenShareAudio,
        };
        assert!(backend.watch_video_track(mic_key).is_err());
        assert!(backend.watch_video_track(screen_audio_key).is_err());
        assert_eq!(backend.voice_snapshot().watched_video_track, None);

        let queue = backend
            .watch_video_track(screen_key.clone())
            .expect("screen share track should be watchable");
        assert_eq!(backend.voice_snapshot().watched_video_track, Some(screen_key));

        let make_frame = |ts: i64, byte: u8| DecodedVideoFrame {
            width: 1280,
            height: 720,
            timestamp_us: ts,
            codec: Some(VideoCodec::H264),
            data: vec![byte],
        };
        assert!(queue.push(make_frame(100, 1)));
        assert!(queue.push(make_frame(200, 2)));
        assert_eq!(queue.dropped_frames(), 1);
        let (gen_id, latest) = backend
            .latest_video_frame(0)
            .expect("latest frame should be available");
        assert_eq!(gen_id, 2);
        assert_eq!(latest.timestamp_us, 200);
        assert_eq!(latest.data, vec![2]);
        assert!(backend.latest_video_frame(gen_id).is_none());

        backend.stop_watching_video();
        assert_eq!(backend.voice_snapshot().watched_video_track, None);
        assert!(!queue.push(make_frame(300, 3)));
        assert!(backend.latest_video_frame(0).is_none());

        let camera_key = TrackSubscriptionKey {
            participant_identity: "user-3:conn-3".into(),
            participant_sid: "PA_3".into(),
            track_sid: "TR_cam_3".into(),
            source: VoiceTrackSource::Camera,
        };
        let cam_queue = backend
            .watch_video_track(camera_key.clone())
            .expect("camera track should be watchable");
        assert_eq!(backend.voice_snapshot().watched_video_track, Some(camera_key));
        assert!(cam_queue.push(make_frame(400, 4)));

        backend.leave_voice();
        assert_eq!(backend.voice_snapshot().watched_video_track, None);
        assert!(!cam_queue.push(make_frame(500, 5)));
        assert!(backend.latest_video_frame(0).is_none());
        assert_eq!(
            *room.watched_video_tracks.lock().unwrap(),
            vec![
                Some("TR_screen_2".to_owned()),
                None,
                Some("TR_cam_3".to_owned()),
                None,
            ]
        );
    }
}
