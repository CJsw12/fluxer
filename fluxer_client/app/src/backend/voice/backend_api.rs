//! `Backend` voice API: session, mute/deafen, audio settings and DM ring.

use super::*;

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
    #[allow(dead_code)]
    pub fn set_voice_mute(&self, muted: bool) {
        let payload = lock(&self.voice).set_self_mute(muted);
        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Toggles local self-mute using the official `LocalVoiceStateMachine` transition
    /// (unmuting while deafened automatically undeafens).
    pub fn toggle_voice_mute(&self) {
        let payload = lock(&self.voice).toggle_self_mute();
        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Updates local self-deaf state and forwards opcode 4 if currently in a voice channel.
    #[allow(dead_code)]
    pub fn set_voice_deaf(&self, deafened: bool) {
        let payload = lock(&self.voice).set_self_deaf(deafened);
        if let Some(payload) = payload {
            #[cfg(test)]
            lock(&self.sent_voice_states).push(payload.clone());
            let _ = send_voice_gateway_update(self.gateway.as_ref(), &payload);
        }
        let _ = self.events.send(UiEvent::VoiceChanged);
    }

    /// Toggles local self-deaf using the official `LocalVoiceStateMachine` transition
    /// (deafening forces self-mute and records `should_unmute_on_undeafen`; undeafening restores prior unmute).
    pub fn toggle_voice_deaf(&self) {
        let payload = lock(&self.voice).toggle_self_deaf();
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

    #[cfg(test)]
    pub(crate) fn push_video_frame_for_test(&self, frame: DecodedVideoFrame) -> bool {
        let queue = lock(&self.voice).video_frame_queue.clone();
        queue.push(frame)
    }

    /// Tears down active voice media on session close/logout.
    pub(in crate::backend) fn shutdown_voice(&self) {
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

    /// Rings DM/group DM call recipients (`POST /channels/{channel_id}/call/ring`).
    pub fn ring_dm_call(&self, channel_id: Snowflake, recipients: Option<Vec<Snowflake>>) {
        #[cfg(test)]
        super::super::lock_sent(&self.sent_call_rings).push((channel_id, recipients.clone()));
        let rest = self.rest.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let recipient_strings: Option<Vec<String>> =
                recipients.map(|ids| ids.into_iter().map(|id| id.to_string()).collect());
            if let Err(err) = rest
                .ring_call_recipients(&channel_id.to_string(), recipient_strings.as_deref())
                .await
            {
                let _ = events.send(UiEvent::ActionFailed {
                    error: format!("Could not ring call recipients: {err}"),
                });
            }
        });
    }
}
