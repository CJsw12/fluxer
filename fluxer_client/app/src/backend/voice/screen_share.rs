//! `Backend` screen share and watched video track API.

use super::*;

impl Backend {
    /// Requests an updated list of capturable screens and windows from the active voice worker.
    pub fn request_screen_sources(&self) {
        let mgr = lock(&self.voice);
        if mgr.state == VoiceConnectionState::Connected
            && let Some(media) = &mgr.media
        {
            media.room.request_screen_sources();
        }
    }

    /// Starts or updates screen sharing for the active connected voice session.
    pub fn start_screen_share(&self, include_audio: bool) -> Result<(), &'static str> {
        let mut opts = {
            let mgr = lock(&self.voice);
            if mgr.self_stream {
                mgr.last_screen_share_options.clone()
            } else {
                ScreenShareOptions::default()
            }
        };
        opts.with_audio = include_audio;
        self.start_screen_share_with_options(opts)
    }

    /// Starts or updates screen sharing with explicit source and encoding options.
    pub fn start_screen_share_with_options(&self, opts: ScreenShareOptions) -> Result<(), &'static str> {
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
            mgr.screen_share_audio = opts.with_audio;
            mgr.last_screen_share_options = opts.clone();
            if let Some(media) = &mut mgr.media {
                if !opts.with_audio {
                    let _ = media.stop_screen_share_audio();
                }
                media.room.set_screen_share_with_options(true, &opts);
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
}
