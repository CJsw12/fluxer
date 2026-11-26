//! `VoiceManager` session state machine, owned media tasks and gateway payload parsing.

use super::*;

/// Room + microphone + screen share + per-track playback tasks owned by the current voice attempt.
pub(super) struct OwnedVoiceMedia {
    pub(super) room: Arc<dyn VoiceRoom>,
    pub(super) mic_task: Option<JoinHandle<()>>,
    pub(super) screen_share_video_task: Option<JoinHandle<()>>,
    pub(super) screen_share_audio_task: Option<JoinHandle<()>>,
    pub(super) event_task: Option<JoinHandle<()>>,
    pub(super) playback_tasks: BTreeMap<TrackSubscriptionKey, JoinHandle<()>>,
    pub(super) closed: bool,
}

impl OwnedVoiceMedia {
    pub(super) fn new(room: Arc<dyn VoiceRoom>) -> Self {
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

    pub(super) fn set_event_task(&mut self, task: JoinHandle<()>) {
        if let Some(prev) = self.event_task.replace(task) {
            prev.abort();
        }
    }

    #[allow(dead_code)]
    pub(super) fn set_mic_task(&mut self, task: JoinHandle<()>) {
        if let Some(prev) = self.mic_task.replace(task) {
            prev.abort();
        }
    }

    #[allow(dead_code)]
    pub(super) fn set_screen_share_tasks(
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

    pub(super) fn stop_screen_share_audio(&mut self) -> Option<JoinHandle<()>> {
        let task = self.screen_share_audio_task.take()?;
        task.abort();
        Some(task)
    }

    pub(super) fn stop_screen_share(&mut self) -> Vec<JoinHandle<()>> {
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
    pub(super) fn insert_playback_task(&mut self, key: TrackSubscriptionKey, task: JoinHandle<()>) {
        if let Some(prev) = self.playback_tasks.insert(key, task) {
            prev.abort();
        }
    }

    #[allow(dead_code)]
    pub(super) fn remove_playback_task(&mut self, key: &TrackSubscriptionKey) -> bool {
        if let Some(task) = self.playback_tasks.remove(key) {
            task.abort();
            true
        } else {
            false
        }
    }

    pub(super) fn subscribed_keys(&self) -> Vec<TrackSubscriptionKey> {
        let mut keys: BTreeSet<TrackSubscriptionKey> = self.playback_tasks.keys().cloned().collect();
        keys.extend(self.room.subscribed_tracks());
        keys.into_iter().collect()
    }

    pub(super) fn published_keys(&self) -> Vec<TrackSubscriptionKey> {
        let mut keys: BTreeSet<TrackSubscriptionKey> = self.playback_tasks.keys().cloned().collect();
        keys.extend(self.room.published_tracks());
        keys.into_iter().collect()
    }

    pub(super) fn shutdown(&mut self) -> Vec<JoinHandle<()>> {
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

pub(super) enum ParsedVoiceServerUpdate {
    UnsupportedE2ee {
        guild_id: Option<Snowflake>,
        channel_id: Option<Snowflake>,
    },
    UnsupportedP2pMesh {
        guild_id: Option<Snowflake>,
        channel_id: Option<Snowflake>,
    },
    Sfu {
        guild_id: Option<Snowflake>,
        channel_id: Option<Snowflake>,
        endpoint: String,
        token: SecretToken,
        connection_id: String,
    },
}

impl ParsedVoiceServerUpdate {
    pub(super) fn guild_id(&self) -> Option<Snowflake> {
        match self {
            Self::UnsupportedE2ee { guild_id, .. }
            | Self::UnsupportedP2pMesh { guild_id, .. }
            | Self::Sfu { guild_id, .. } => *guild_id,
        }
    }

    pub(super) fn channel_id(&self) -> Option<Snowflake> {
        match self {
            Self::UnsupportedE2ee { channel_id, .. }
            | Self::UnsupportedP2pMesh { channel_id, .. }
            | Self::Sfu { channel_id, .. } => *channel_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedVoiceStateUpdate {
    pub(super) user_id: Snowflake,
    pub(super) guild_id: Option<Snowflake>,
    pub(super) channel_id: Option<Snowflake>,
    pub(super) session_id: String,
    pub(super) connection_id: Option<String>,
    pub(super) self_mute: bool,
    pub(super) self_deaf: bool,
    pub(super) self_video: bool,
    pub(super) self_stream: bool,
    pub(super) mute: bool,
    pub(super) deaf: bool,
    pub(super) suppress: bool,
}

/// Backend-owned voice state machine and media handle owner.
pub(crate) struct VoiceManager {
    pub(super) transport: Arc<dyn VoiceTransport>,
    pub(super) gateway_session_id: Option<String>,
    pub(super) state: VoiceConnectionState,
    pub(super) guild_id: Option<Snowflake>,
    pub(super) channel_id: Option<Snowflake>,
    pub(super) connection_id: Option<String>,
    /// Last accepted voice server endpoint (kept private, never exposed in `VoiceSnapshot`).
    pub(super) endpoint: Option<String>,
    pub(super) attempt_id: u64,
    pub(super) self_mute: bool,
    pub(super) self_deaf: bool,
    pub(super) should_unmute_on_undeafen: bool,
    pub(super) self_stream: bool,
    pub(super) screen_share_audio: bool,
    pub(super) server_mute: bool,
    pub(super) server_deaf: bool,
    pub(super) speak_denied: bool,
    pub(super) screen_share_codec: ScreenShareCodecTracker,
    pub(super) audio_settings: VoiceAudioSettings,
    pub(super) connect_task: Option<JoinHandle<()>>,
    pub(super) media: Option<OwnedVoiceMedia>,
    pub(super) watched_video_track: Option<TrackSubscriptionKey>,
    pub(super) video_frame_queue: LatestVideoFrameQueue,
    pub(super) screen_sources: Vec<ScreenSourceInfo>,
    pub(super) active_speakers: BTreeSet<Snowflake>,
    pub(super) audio_levels: BTreeMap<Snowflake, u16>,
    pub(super) last_screen_share_options: ScreenShareOptions,
    pub(super) channel_members: BTreeMap<(Option<Snowflake>, Snowflake), ChannelVoiceMember>,
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
            should_unmute_on_undeafen: false,
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
            screen_sources: Vec::new(),
            active_speakers: BTreeSet::new(),
            audio_levels: BTreeMap::new(),
            last_screen_share_options: ScreenShareOptions::default(),
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
            should_unmute_on_undeafen: self.should_unmute_on_undeafen,
            self_stream: self.self_stream,
            screen_share_audio: self.screen_share_audio,
            server_mute: self.server_mute,
            server_deaf: self.server_deaf,
            speak_denied: self.speak_denied,
            screen_share_codec: self.screen_share_codec.clone(),
            subscribed_tracks: self.media.as_ref().map_or_else(Vec::new, OwnedVoiceMedia::subscribed_keys),
            published_tracks: self.media.as_ref().map_or_else(Vec::new, OwnedVoiceMedia::published_keys),
            watched_video_track: self.watched_video_track.clone(),
            screen_sources: self.screen_sources.clone(),
            active_speakers: self.active_speakers.clone(),
            audio_levels: self.audio_levels.clone(),
            channel_members: self.channel_members.clone(),
        }
    }

    pub(super) fn watch_video_track(
        &mut self,
        key: TrackSubscriptionKey,
    ) -> Result<LatestVideoFrameQueue, &'static str> {
        if self.state != VoiceConnectionState::Connected {
            return Err("voice session is not connected");
        }
        if !matches!(key.source, VoiceTrackSource::ScreenShare | VoiceTrackSource::Camera) {
            return Err("only video tracks can be watched");
        }
        let published = self
            .media
            .as_ref()
            .map_or_else(Vec::new, OwnedVoiceMedia::published_keys);
        if !published.is_empty() && !published.contains(&key) {
            return Err("video track is not published");
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

    pub(super) fn stop_watching_video(&mut self) -> bool {
        self.video_frame_queue.close();
        let cleared = self.watched_video_track.take().is_some();
        if cleared && let Some(media) = &self.media {
            media.room.set_watched_video_track(None);
        }
        cleared
    }

    pub(super) fn abort_active_media(&mut self) -> Vec<JoinHandle<()>> {
        self.stop_watching_video();
        self.screen_sources.clear();
        self.active_speakers.clear();
        self.audio_levels.clear();
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

    pub(super) fn stop_screen_share_media(&mut self) -> Vec<JoinHandle<()>> {
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

    pub(super) fn current_self_flags(&self) -> (bool, bool) {
        (self.self_mute, self.self_deaf)
    }

    pub(super) fn apply_audio_mute_flags(&self) {
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
    pub(super) fn begin_join(&mut self, guild_id: Option<Snowflake>, channel_id: Snowflake) -> Option<(VoiceStateUpdate, Vec<JoinHandle<()>>)> {
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

    pub(super) fn begin_leave(&mut self) -> (Option<VoiceStateUpdate>, Vec<JoinHandle<()>>) {
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

    pub(super) fn begin_leave_if_attempt(
        &mut self,
        expected_attempt_id: u64,
    ) -> (Option<VoiceStateUpdate>, Vec<JoinHandle<()>>) {
        if self.attempt_id != expected_attempt_id {
            return (None, Vec::new());
        }
        self.begin_leave()
    }

    pub(super) fn mute_denied(&self) -> bool {
        self.server_mute || self.server_deaf || self.speak_denied
    }

    /// Official `LocalVoiceStateMachine.ts:toggleSelfMute` 1:1 transition.
    pub(super) fn toggle_self_mute(&mut self) -> Option<VoiceStateUpdate> {
        let mic_denied = self.mute_denied();
        let new_self_mute = !self.self_mute;
        if self.self_deaf && !new_self_mute {
            if mic_denied {
                self.self_deaf = false;
                self.self_mute = true;
                self.should_unmute_on_undeafen = false;
            } else {
                self.self_mute = false;
                self.self_deaf = false;
                self.should_unmute_on_undeafen = false;
            }
        } else if mic_denied && !new_self_mute {
            self.self_mute = true;
        } else {
            self.self_mute = new_self_mute;
            if !self.self_deaf {
                self.should_unmute_on_undeafen = false;
            }
        }
        self.apply_audio_mute_flags();
        self.current_state_update_payload()
    }

    /// Official `LocalVoiceStateMachine.ts:toggleSelfDeaf` 1:1 transition.
    pub(super) fn toggle_self_deaf(&mut self) -> Option<VoiceStateUpdate> {
        let mic_denied = self.mute_denied();
        let new_self_deaf = !self.self_deaf;
        if new_self_deaf {
            let was_muted_before = self.self_mute || mic_denied;
            self.self_deaf = true;
            self.self_mute = true;
            self.should_unmute_on_undeafen = !was_muted_before;
        } else {
            self.self_deaf = false;
            if self.should_unmute_on_undeafen && !mic_denied {
                self.self_mute = false;
            }
            self.should_unmute_on_undeafen = false;
        }
        self.apply_audio_mute_flags();
        self.current_state_update_payload()
    }

    #[allow(dead_code)]
    pub(super) fn set_self_mute(&mut self, muted: bool) -> Option<VoiceStateUpdate> {
        let mic_denied = self.mute_denied();
        if self.self_deaf && !muted {
            if mic_denied {
                self.self_deaf = false;
                self.self_mute = true;
                self.should_unmute_on_undeafen = false;
            } else {
                self.self_mute = false;
                self.self_deaf = false;
                self.should_unmute_on_undeafen = false;
            }
        } else if mic_denied && !muted {
            self.self_mute = true;
        } else {
            self.self_mute = muted;
            if !self.self_deaf {
                self.should_unmute_on_undeafen = false;
            }
        }
        self.apply_audio_mute_flags();
        self.current_state_update_payload()
    }

    #[allow(dead_code)]
    pub(super) fn set_self_deaf(&mut self, deafened: bool) -> Option<VoiceStateUpdate> {
        let mic_denied = self.mute_denied();
        if deafened {
            if !self.self_deaf {
                let was_muted_before = self.self_mute || mic_denied;
                self.should_unmute_on_undeafen = !was_muted_before;
            }
            self.self_deaf = true;
            self.self_mute = true;
        } else {
            self.self_deaf = false;
            if self.should_unmute_on_undeafen && !mic_denied {
                self.self_mute = false;
            }
            self.should_unmute_on_undeafen = false;
        }
        self.apply_audio_mute_flags();
        self.current_state_update_payload()
    }

    pub(super) fn current_state_update_payload(&self) -> Option<VoiceStateUpdate> {
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

    pub(super) fn replace_guild_voice_states(&mut self, guild_id: Snowflake, voice_states: &[Value]) {
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
    pub(super) fn handle_gateway_ready(&mut self, session_id: String) -> (bool, Option<VoiceStateUpdate>, Vec<JoinHandle<()>>) {
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

    pub(super) fn handle_gateway_reset(&mut self) -> (bool, Vec<JoinHandle<()>>) {
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

    pub(super) fn handle_voice_state_dispatch(
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

    pub(super) fn accept_voice_server_dispatch(&mut self, payload: &Value) -> Result<Option<VoiceCredentials>, VoiceFailure> {
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
        if self.guild_id != parsed.guild_id() {
            return Ok(None);
        }

        let parsed_channel_id = parsed.channel_id();
        let is_channel_move = parsed_channel_id.is_some_and(|ch| ch != expected_channel_id);
        if is_channel_move && self.state != VoiceConnectionState::Connected {
            return Ok(None);
        }

        let (guild_id, endpoint, token, connection_id) = match parsed {
            ParsedVoiceServerUpdate::UnsupportedE2ee { .. } => {
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
            ParsedVoiceServerUpdate::UnsupportedP2pMesh { .. } => {
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
            ParsedVoiceServerUpdate::Sfu {
                guild_id,
                channel_id: _,
                endpoint,
                token,
                connection_id,
            } => (guild_id, endpoint, token, connection_id),
        };

        let resolved_channel_id = parsed_channel_id.unwrap_or(expected_channel_id);
        // Deduplicate repeated VOICE_SERVER_UPDATE frames for the exact same active connection.
        if !is_channel_move
            && matches!(self.state, VoiceConnectionState::Connecting | VoiceConnectionState::Connected)
            && self.connection_id.as_deref() == Some(connection_id.as_str())
            && self.endpoint.as_deref() == Some(endpoint.as_str())
        {
            return Ok(None);
        }

        if self.connect_task.is_some() || self.media.is_some() || is_channel_move {
            self.attempt_id = self.attempt_id.saturating_add(1);
        }
        self.channel_id = Some(resolved_channel_id);
        self.connection_id = Some(connection_id.clone());
        self.endpoint = Some(endpoint.clone());
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
            guild_id,
            channel_id: resolved_channel_id,
            connection_id,
            endpoint,
            token,
            initial_mic_muted: self.self_mute
                || self.self_deaf
                || self.server_mute
                || self.speak_denied,
            initial_playback_muted: self.self_deaf || self.server_deaf,
        }))
    }

    pub(super) fn finish_connect(
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
                if let Some(mut previous) = self.media.take() {
                    let _ = previous.shutdown();
                }
                self.self_stream = false;
                self.screen_share_audio = false;
                self.screen_share_codec.reset();
                self.state = VoiceConnectionState::Failed(VoiceFailure::TransportFailed(err));
                true
            }
        }
    }

    pub(super) fn handle_room_disconnected(
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

    pub(super) fn handle_room_event(&mut self, event: VoiceRoomEvent) -> bool {
        match event {
            VoiceRoomEvent::TracksChanged => {
                if let Some(watched) = &self.watched_video_track {
                    let published = self
                        .media
                        .as_ref()
                        .map_or_else(Vec::new, OwnedVoiceMedia::published_keys);
                    if !published.contains(watched) {
                        self.stop_watching_video();
                    }
                }
                true
            }
            VoiceRoomEvent::ScreenSources(sources) => {
                self.screen_sources = sources;
                true
            }
            VoiceRoomEvent::ActiveSpeakers(speakers) => {
                self.active_speakers.clear();
                self.audio_levels.clear();
                for s in speakers {
                    if let Some(uid) = parse_participant_user_id(&s.participant_identity) {
                        let milli = (s.audio_level.clamp(0.0, 1.0) * 1000.0).round() as u16;
                        self.active_speakers.insert(uid);
                        self.audio_levels.insert(uid, milli);
                    }
                }
                true
            }
            VoiceRoomEvent::Disconnected(_) => false,
            VoiceRoomEvent::VideoFrameReady => {
                let Some((track_sid, width, height, rgba)) = self
                    .media
                    .as_ref()
                    .and_then(|media| media.room.take_pending_video_frame())
                else {
                    return false;
                };
                if self
                    .watched_video_track
                    .as_ref()
                    .is_some_and(|t| t.track_sid == track_sid)
                {
                    return self.video_frame_queue.push(DecodedVideoFrame {
                        width,
                        height,
                        timestamp_us: 0,
                        codec: None,
                        data: rgba,
                    });
                }
                false
            }
            #[cfg(test)]
            VoiceRoomEvent::VideoFrame {
                track_sid,
                width,
                height,
                rgba,
            } => {
                if self
                    .watched_video_track
                    .as_ref()
                    .is_some_and(|t| t.track_sid == track_sid)
                {
                    return self.video_frame_queue.push(DecodedVideoFrame {
                        width,
                        height,
                        timestamp_us: 0,
                        codec: None,
                        data: rgba,
                    });
                }
                false
            }
        }
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
