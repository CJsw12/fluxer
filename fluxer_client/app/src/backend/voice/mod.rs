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

/// Parses a LiveKit participant identity (`"user_<uid>_<session>"`, `"<uid>:<session>"`,
/// or `"<uid>"`) into the corresponding Fluxer user `Snowflake`.
pub fn parse_participant_user_id(identity: &str) -> Option<Snowflake> {
    let trimmed = identity.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("user_") {
        let uid_part = rest.split(['_', ':']).next()?;
        return Snowflake::parse(uid_part);
    }
    let uid_part = trimmed.split(':').next()?;
    Snowflake::parse(uid_part)
}

/// Enumerated desktop/window capture source returned by `fluxer_voice_worker`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScreenSourceInfo {
    pub id: u64,
    pub kind: String,
    pub title: String,
    pub display_id: i64,
}

/// Screen-share encoding and capture source options sent to `fluxer_voice_worker`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScreenShareOptions {
    pub with_audio: bool,
    pub source_id: Option<u64>,
    pub source_kind: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub codec: Option<String>,
    pub max_bitrate: Option<u64>,
}

/// Active speaker audio-level entry forwarded from LiveKit `ActiveSpeakersChanged`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpeakerInfo {
    pub participant_identity: String,
    pub audio_level: f32,
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
    pub should_unmute_on_undeafen: bool,
    pub self_stream: bool,
    pub screen_share_audio: bool,
    pub server_mute: bool,
    pub server_deaf: bool,
    pub speak_denied: bool,
    pub screen_share_codec: ScreenShareCodecTracker,
    pub subscribed_tracks: Vec<TrackSubscriptionKey>,
    pub published_tracks: Vec<TrackSubscriptionKey>,
    pub watched_video_track: Option<TrackSubscriptionKey>,
    pub screen_sources: Vec<ScreenSourceInfo>,
    pub active_speakers: BTreeSet<Snowflake>,
    /// Quantized audio level per speaking participant (`0..=1000`, representing `0.0..=1.0`).
    pub audio_levels: BTreeMap<Snowflake, u16>,
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
            should_unmute_on_undeafen: false,
            self_stream: false,
            screen_share_audio: false,
            server_mute: false,
            server_deaf: false,
            speak_denied: false,
            screen_share_codec: ScreenShareCodecTracker::default(),
            subscribed_tracks: Vec::new(),
            published_tracks: Vec::new(),
            watched_video_track: None,
            screen_sources: Vec::new(),
            active_speakers: BTreeSet::new(),
            audio_levels: BTreeMap::new(),
            channel_members: BTreeMap::new(),
        }
    }
}

impl VoiceSnapshot {
    /// True when `user_id` is currently reported in LiveKit `ActiveSpeakersChanged`.
    pub fn is_speaking(&self, user_id: Snowflake) -> bool {
        self.active_speakers.contains(&user_id)
    }

    /// Quantized audio level (`0..=1000`) for `user_id` (`0` when not speaking).
    pub fn audio_level_milli(&self, user_id: Snowflake) -> u16 {
        self.audio_levels.get(&user_id).copied().unwrap_or(0)
    }
    /// Members currently present in `channel_id`, sorted by user id.
    pub fn members_in_channel(&self, channel_id: Snowflake) -> Vec<ChannelVoiceMember> {
        self.channel_members
            .values()
            .filter(|m| m.channel_id == channel_id)
            .cloned()
            .collect()
    }

    /// Effective microphone muted state (`UserAreaState.ts:selectUserAreaMicrophoneState`).
    pub fn effective_muted(&self) -> bool {
        self.self_mute
            || self.self_deaf
            || self.server_mute
            || self.server_deaf
            || self.speak_denied
    }

    /// Effective speaker deafened state.
    pub fn effective_deafened(&self) -> bool {
        self.self_deaf || self.server_deaf
    }

    /// True when moderator mute/deaf or channel permissions lock local unmute (`UserAreaState.ts:30`).
    pub fn mute_toggle_locked(&self) -> bool {
        self.server_mute || self.server_deaf || self.speak_denied
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
    pub(crate) initial_mic_muted: bool,
    pub(crate) initial_playback_muted: bool,
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum VoiceRoomEvent {
    TracksChanged,
    Disconnected(String),
    VideoFrameReady,
    ScreenSources(Vec<ScreenSourceInfo>),
    ActiveSpeakers(Vec<SpeakerInfo>),
    #[cfg(test)]
    VideoFrame {
        track_sid: String,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
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
    fn set_screen_share_with_options(&self, active: bool, opts: &ScreenShareOptions) {
        self.set_screen_share(active, opts.with_audio);
    }
    fn request_screen_sources(&self) {}
    fn set_audio_settings(&self, _settings: &VoiceAudioSettings) {}
    fn set_watched_video_track(&self, _track_sid: Option<String>) {}
    fn subscribed_tracks(&self) -> Vec<TrackSubscriptionKey> {
        Vec::new()
    }
    fn published_tracks(&self) -> Vec<TrackSubscriptionKey> {
        self.subscribed_tracks()
    }
    fn take_event_rx(&self) -> Option<mpsc::UnboundedReceiver<VoiceRoomEvent>> {
        None
    }
    fn take_pending_video_frame(&self) -> Option<(String, u32, u32, Vec<u8>)> {
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
mod backend_api;
mod gateway;
mod manager;
mod screen_share;
#[cfg(test)]
mod tests;

pub(crate) use gateway::{on_gateway_dispatch, on_gateway_ready, on_gateway_reset};
pub(crate) use manager::VoiceManager;
use gateway::*;
use manager::*;

