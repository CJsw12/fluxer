//! Voice popouts, voice/DM calls, screen share picker and voice name lookups.

use super::*;

impl ClientView {
    pub(crate) fn toggle_user_area_popout(&mut self, cx: &mut Context<Self>) {
        self.user_area_popout_open = !self.user_area_popout_open;
        self.user_area_submenu = None;
        if self.user_area_popout_open {
            self.voice_popout = None;
            self.guild_header_popout_open = false;
        }
        cx.notify();
    }

    pub(crate) fn toggle_guild_header_popout(&mut self, cx: &mut Context<Self>) {
        self.guild_header_popout_open = !self.guild_header_popout_open;
        if self.guild_header_popout_open {
            self.user_area_popout_open = false;
            self.voice_popout = None;
        }
        cx.notify();
    }

    pub(crate) fn toggle_voice_popout(&mut self, kind: VoicePopoutKind, cx: &mut Context<Self>) {
        if self.voice_popout == Some(kind) {
            self.voice_popout = None;
        } else {
            self.voice_popout = Some(kind);
            self.user_area_popout_open = false;
            self.refresh_audio_devices(cx);
        }
        cx.notify();
    }

    pub(crate) fn open_voice_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.voice_popout = None;
        self.user_area_popout_open = false;
        if !matches!(self.overlay, Overlay::Settings(_)) {
            self.open_settings(window, cx);
        }
        self.select_settings_tab(ui::settings::SettingsTab::VoiceVideo, cx);
    }

    pub(crate) fn join_voice_channel(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        if self.voice.channel_id == Some(channel_id)
            && matches!(
                self.voice.state,
                crate::backend::voice::VoiceConnectionState::Connected
                    | crate::backend::voice::VoiceConnectionState::Connecting
                    | crate::backend::voice::VoiceConnectionState::Reconnecting
            )
        {
            self.voice_stage_open = true;
            cx.notify();
            return;
        }
        let target = {
            let state = self.backend.state();
            let Some(ch) = state.channels.get(&channel_id) else { return };
            if ch.kind != fluxer_client_core::state::ChannelKind::GuildVoice && ch.guild_id.is_some() {
                return;
            }
            if ch.guild_id.is_some() {
                let perms = state.channel_permissions(channel_id);
                let required = fluxer_client_core::state::bits::VIEW_CHANNEL
                    | fluxer_client_core::state::bits::CONNECT;
                if (perms & required) != required {
                    self.error = Some(l10n::tr("You do not have permission to connect to this voice channel.").into());
                    cx.notify();
                    return;
                }
            }
            ch.guild_id
        };
        self.error = None;
        self.backend.join_voice(target, channel_id);
        self.voice = self.backend.voice_snapshot();
        self.voice_stage_open = self.voice.channel_id.is_some();
        cx.notify();
    }

    fn start_dm_call(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        let was_active = self.voice.channel_id == Some(channel_id)
            && matches!(
                self.voice.state,
                crate::backend::voice::VoiceConnectionState::Connected
                    | crate::backend::voice::VoiceConnectionState::Connecting
                    | crate::backend::voice::VoiceConnectionState::Reconnecting
            );
        let ring_info = {
            let state = self.backend.state();
            let me = state.current_user_id();
            let Some(ch) = state.channels.get(&channel_id) else { return };
            let can_ring = matches!(
                ch.kind,
                fluxer_client_core::state::ChannelKind::Dm | fluxer_client_core::state::ChannelKind::GroupDm
            );
            let recipients: Vec<Snowflake> = ch
                .recipients
                .iter()
                .map(|u| u.id)
                .filter(|id| Some(*id) != me)
                .collect();
            (can_ring, (!recipients.is_empty()).then_some(recipients))
        };
        self.join_voice_channel(channel_id, cx);
        if !was_active && ring_info.0 && self.voice.channel_id == Some(channel_id) {
            self.backend.ring_dm_call(channel_id, ring_info.1);
        }
    }

    /// DM header voice-call button (`CallButtons.tsx` `VoiceCallButton`): joins and rings the DM call.
    pub(crate) fn start_dm_voice_call(&mut self, cx: &mut Context<Self>) {
        if let Some(channel_id) = self.selected_channel {
            self.start_dm_call(channel_id, cx);
        }
    }

    /// DM header video-call button (`CallButtons.tsx` `VideoCallButton`): joins and rings the DM call.
    /// TODO(backlog): no camera capture/publish path exists in the voice worker yet, so this only
    /// joins and rings without turning the local camera on.
    pub(crate) fn start_dm_video_call(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel else { return };
        self.start_dm_call(channel_id, cx);
    }

    pub(crate) fn leave_voice_channel(&mut self, cx: &mut Context<Self>) {
        self.backend.leave_voice();
        self.voice = self.backend.voice_snapshot();
        self.voice_stage_open = false;
        self.voice_popout = None;
        self.sync_watched_video_frame();
        cx.notify();
    }

    pub(crate) fn toggle_voice_mute(&mut self, cx: &mut Context<Self>) {
        self.backend.toggle_voice_mute();
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    pub(crate) fn toggle_voice_deaf(&mut self, cx: &mut Context<Self>) {
        self.backend.toggle_voice_deaf();
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    pub(crate) fn handle_screen_share_button(&mut self, cx: &mut Context<Self>) {
        if self.voice.self_stream {
            self.backend.stop_screen_share();
            self.error = None;
            self.voice = self.backend.voice_snapshot();
            cx.notify();
        } else {
            self.open_screen_share_picker(cx);
        }
    }

    pub(crate) fn open_screen_share_picker(&mut self, cx: &mut Context<Self>) {
        if self.voice.state != crate::backend::voice::VoiceConnectionState::Connected {
            return;
        }
        self.prepare_overlay_replace(cx);
        self.backend.request_screen_sources();
        let modal = ui::screen_share_modal::ScreenSharePickerModal::from_prefs(
            &self.prefs.data.video_resolution,
            self.prefs.data.video_fps,
            &self.prefs.data.video_codec,
        );
        self.overlay = Overlay::ScreenSharePicker(modal);
        cx.notify();
    }

    pub(crate) fn set_screen_share_picker_tab(
        &mut self,
        tab: ui::screen_share_modal::ScreenShareTab,
        cx: &mut Context<Self>,
    ) {
        if let Overlay::ScreenSharePicker(modal) = &mut self.overlay {
            modal.tab = tab;
            modal.selected_source_kind = match tab {
                ui::screen_share_modal::ScreenShareTab::Screens => "screen".to_owned(),
                ui::screen_share_modal::ScreenShareTab::Applications => "window".to_owned(),
            };
            modal.selected_source_id = None;
            cx.notify();
        }
    }

    pub(crate) fn select_screen_share_source(
        &mut self,
        source_id: u64,
        source_kind: String,
        cx: &mut Context<Self>,
    ) {
        if let Overlay::ScreenSharePicker(modal) = &mut self.overlay {
            modal.selected_source_id = Some(source_id);
            modal.selected_source_kind = source_kind;
            cx.notify();
        }
    }

    pub(crate) fn set_screen_share_picker_resolution(
        &mut self,
        resolution: String,
        cx: &mut Context<Self>,
    ) {
        if let Overlay::ScreenSharePicker(modal) = &mut self.overlay {
            modal.resolution = resolution;
            cx.notify();
        }
    }

    pub(crate) fn set_screen_share_picker_fps(&mut self, fps: u32, cx: &mut Context<Self>) {
        if let Overlay::ScreenSharePicker(modal) = &mut self.overlay {
            modal.fps = fps;
            cx.notify();
        }
    }

    pub(crate) fn set_screen_share_picker_codec(&mut self, codec: String, cx: &mut Context<Self>) {
        if let Overlay::ScreenSharePicker(modal) = &mut self.overlay {
            modal.codec = codec;
            cx.notify();
        }
    }

    pub(crate) fn set_screen_share_picker_audio(
        &mut self,
        include_audio: bool,
        cx: &mut Context<Self>,
    ) {
        if let Overlay::ScreenSharePicker(modal) = &mut self.overlay {
            modal.include_audio = include_audio;
            cx.notify();
        }
    }

    pub(crate) fn confirm_screen_share_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = (match &self.overlay {
            Overlay::ScreenSharePicker(m) => Some(m.clone()),
            _ => None,
        }) else {
            return;
        };
        let opts = modal.to_options(self.prefs.data.video_bitrate_kbps);
        match self.backend.start_screen_share_with_options(opts) {
            Ok(()) => {
                self.error = None;
                self.close_overlay(window, cx);
            }
            Err(err) => {
                self.error = Some(l10n::tr(err).into());
            }
        }
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn toggle_screen_share(&mut self, include_audio: bool, cx: &mut Context<Self>) {
        if self.voice.self_stream {
            self.backend.stop_screen_share();
            self.error = None;
        } else {
            match self.backend.start_screen_share(include_audio) {
                Ok(()) => self.error = None,
                Err(err) => self.error = Some(l10n::tr(err).into()),
            }
        }
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    pub(crate) fn toggle_screen_share_audio(&mut self, cx: &mut Context<Self>) {
        if !self.voice.self_stream {
            return;
        }
        match self.backend.start_screen_share(!self.voice.screen_share_audio) {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(l10n::tr(err).into()),
        }
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    pub(crate) fn toggle_watch_video_track(
        &mut self,
        key: crate::backend::voice::TrackSubscriptionKey,
        cx: &mut Context<Self>,
    ) {
        if self.voice.watched_video_track.as_ref() == Some(&key) {
            self.backend.stop_watching_video();
            self.error = None;
        } else {
            match self.backend.watch_video_track(key) {
                Ok(_) => self.error = None,
                Err(err) => self.error = Some(l10n::tr(err).into()),
            }
        }
        self.voice = self.backend.voice_snapshot();
        self.sync_watched_video_frame();
        cx.notify();
    }

    pub(crate) fn sync_watched_video_frame(&mut self) {
        let Some(track) = self.voice.watched_video_track.as_ref() else {
            self.watched_video_track_sid = None;
            self.watched_video_frame_seq = 0;
            self.watched_video_frame = None;
            self.watched_video_frame_times.clear();
            self.watched_video_fps = 0;
            return;
        };
        if self.watched_video_track_sid.as_deref() != Some(track.track_sid.as_str()) {
            self.watched_video_track_sid = Some(track.track_sid.clone());
            self.watched_video_frame_seq = 0;
            self.watched_video_frame = None;
            self.watched_video_frame_times.clear();
            self.watched_video_fps = 0;
        }
        if let Some((gen_id, frame)) = self.backend.latest_video_frame(self.watched_video_frame_seq) {
            self.watched_video_frame_seq = gen_id;
            let now = std::time::Instant::now();
            self.watched_video_frame_times.push_back(now);
            while let Some(&front) = self.watched_video_frame_times.front() {
                if now.duration_since(front) > std::time::Duration::from_secs(2) {
                    self.watched_video_frame_times.pop_front();
                } else {
                    break;
                }
            }
            if self.watched_video_frame_times.len() >= 2
                && let (Some(&first), Some(&last)) = (
                    self.watched_video_frame_times.front(),
                    self.watched_video_frame_times.back(),
                )
            {
                let secs = last.duration_since(first).as_secs_f64();
                if secs > 0.05 {
                    let fps = ((self.watched_video_frame_times.len() - 1) as f64 / secs).round();
                    self.watched_video_fps = (fps as u32).clamp(1, 120);
                }
            }
            self.watched_video_frame =
                ui::video_viewer::decoded_frame_to_render_image(&frame).map(|img| (frame.width, frame.height, img));
        }
    }

    pub(crate) fn voice_channel_name(&self, channel_id: Snowflake) -> SharedString {
        let state = self.backend.state();
        state
            .channels
            .get(&channel_id)
            .and_then(|c| c.name.as_deref())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| channel_id.to_string())
            .into()
    }

    pub(crate) fn voice_guild_name(
        &self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
    ) -> Option<SharedString> {
        let state = self.backend.state();
        let resolved_gid = guild_id.or_else(|| state.channels.get(&channel_id).and_then(|c| c.guild_id))?;
        state
            .guilds
            .get(&resolved_gid)
            .map(|g| SharedString::from(g.name.clone()))
    }

    pub(crate) fn voice_member_name(&self, guild_id: Option<Snowflake>, user_id: Snowflake) -> SharedString {
        let state = self.backend.state();
        if let Some(gid) = guild_id
            && let Some(nick) = state
                .members
                .get(&gid)
                .and_then(|m| m.get(&user_id))
                .and_then(|m| m.nick.as_deref())
                .filter(|n| !n.is_empty())
        {
            return nick.to_owned().into();
        }
        if let Some(user) = state.users.get(&user_id) {
            return user.display_name().to_owned().into();
        }
        user_id.to_string().into()
    }

    pub(crate) fn voice_member_avatar_url(
        &self,
        guild_id: Option<Snowflake>,
        user_id: Snowflake,
    ) -> Option<SharedString> {
        let media = self.backend.media()?;
        let state = self.backend.state();
        let user = guild_id
            .and_then(|gid| state.members.get(&gid)?.get(&user_id).map(|m| &m.user))
            .or_else(|| state.users.get(&user_id))
            .or_else(|| state.current_user.as_ref().filter(|u| u.id == user_id))?;
        crate::view_model::avatar_url(media, user).map(Into::into)
    }
}
