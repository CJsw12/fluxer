//! Gateway event batches and the main `refresh` state rebuild.

use super::*;

impl ClientView {
    pub(crate) fn handle_events(&mut self, batch: Vec<UiEvent>, cx: &mut Context<Self>) {
        let mut connected = false;
        let mut guild_available = false;
        for event in batch {
            match event {
                UiEvent::StateChanged => {}
                UiEvent::VoiceChanged => {
                    self.voice = self.backend.voice_snapshot();
                    self.sync_watched_video_frame();
                }
                UiEvent::GuildAvailable { guild_id } => {
                    guild_available |= self.place == Place::Guild(guild_id);
                }
                UiEvent::Gateway(status) => {
                    // READY/RESUMED: subscriptions are sent again (with
                    // `sync`) by the refresh below.
                    if status == GatewayStatus::Ready {
                        self.member_subscriptions.reset_session();
                        connected = true;
                        if !self.backend.state().saved_messages_loaded {
                            self.backend.load_saved_messages();
                        }
                    }
                    self.connection = match status {
                        GatewayStatus::Ready => Connection::Online,
                        GatewayStatus::Connecting | GatewayStatus::Reconnecting { .. } => Connection::Connecting,
                        GatewayStatus::Closed { .. } => Connection::Offline,
                    };
                    self.gateway_status = status;
                    self.refresh_status_text();
                }
                UiEvent::HistoryLoaded { channel_id, stamp, error } => {
                    // A page of a channel left meanwhile neither ends this
                    // channel's load nor shows its error here.
                    if Some(channel_id) == self.selected_channel && self.backend.history_stamp(channel_id) == stamp {
                        self.loading_history = false;
                        if let Some(error) = error {
                            if self.pending_jump.is_some_and(|(ch, _)| ch == channel_id) {
                                self.pending_jump = None;
                            }
                            self.error =
                                Some(format!("{}: {error}", l10n::tr("Messages failed to load")).into());
                        }
                    }
                }
                UiEvent::SendFailed { error } => {
                    self.error = Some(format!("{}: {error}", l10n::tr("Failed to send message.")).into());
                }
                UiEvent::ActionFailed { error } => {
                    self.error = Some(error.into());
                }
                UiEvent::DmOpened { channel_id } => self.open_channel(None, channel_id, cx),
                UiEvent::Notify(notification) => self.notify(&notification, cx),
                UiEvent::NotificationClicked { guild_id, channel_id } => {
                    self.notification_clicked(guild_id, channel_id, cx);
                }
                UiEvent::UploadProgress { nonce, percent } => {
                    self.uploads.insert(nonce, percent);
                }
                UiEvent::UploadEnded { nonce, error } => self.upload_ended(nonce, error),
                UiEvent::ProfileLoaded { user_id, guild_id, result } => {
                    self.profile_loaded(user_id, guild_id, result, cx);
                }
                UiEvent::NoteSaved { user_id, error } => {
                    if let Some(error) = error {
                        self.error = Some(error.into());
                        self.sync_profile_note_from_state(user_id);
                    }
                }
                UiEvent::FriendRequestSent { input, error } => {
                    self.friends_status = Some(match error {
                        Some(err) => Err(err),
                        None => {
                            if self.add_friend_input.read(cx).value().trim() == input.trim() {
                                let (input_entity, handle) = (self.add_friend_input.clone(), self.window_handle);
                                cx.defer(move |cx| {
                                    let _ = handle.update(cx, |_, window, cx| {
                                        input_entity.update(cx, |s, cx| s.set_value("", window, cx));
                                    });
                                });
                            }
                            Ok(l10n::tr("Friend request sent!"))
                        }
                    });
                }
                UiEvent::InvitePreviewLoaded { code, result } => {
                    if let Overlay::JoinGuild(modal) = &mut self.overlay {
                        let current = modal.input.read(cx).value().to_string();
                        if crate::backend::extract_invite_code(&current).as_deref() != Some(code.as_str()) {
                            continue;
                        }
                        modal.loading = false;
                        match result {
                            Ok(preview) => {
                                modal.preview = Some(preview);
                                modal.error = None;
                            }
                            Err(err) => {
                                modal.preview = None;
                                modal.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::InviteAccepted { code, result } => {
                    let is_current = self.pending_accept_code.as_deref() == Some(code.as_str())
                        && match &self.overlay {
                            Overlay::JoinGuild(modal) => {
                                crate::backend::extract_invite_code(&modal.input.read(cx).value()).as_deref()
                                    == Some(code.as_str())
                            }
                            _ => false,
                        };
                    if !is_current {
                        continue;
                    }
                    self.pending_accept_code = None;
                    match result {
                        Ok(accepted) => {
                            if matches!(self.overlay, Overlay::JoinGuild(_)) {
                                self.overlay = Overlay::None;
                                let composer = self.composer.clone();
                                let handle = self.window_handle;
                                cx.defer(move |cx| {
                                    let _ = handle.update(cx, |_, window, cx| {
                                        let focus = composer.read(cx).focus_handle(cx);
                                        window.focus(&focus, cx);
                                    });
                                });
                            }
                            if let Some(id) = accepted.guild_id {
                                self.pending_joined_channel = None;
                                if self.backend.state().guilds.contains_key(&id) {
                                    self.pending_joined_guild = None;
                                    self.select_place(Place::Guild(id), cx);
                                } else {
                                    self.pending_joined_guild = Some(id);
                                }
                            } else if let Some(ch) = accepted.channel_id {
                                self.pending_joined_guild = None;
                                if self.backend.state().channels.contains_key(&ch) {
                                    self.pending_joined_channel = None;
                                    self.open_channel(None, ch, cx);
                                } else {
                                    self.pending_joined_channel = Some(ch);
                                }
                            }
                        }
                        Err(err) => {
                            if let Overlay::JoinGuild(modal) = &mut self.overlay {
                                modal.loading = false;
                                modal.error = Some(err);
                            } else {
                                self.error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::GuildLeft { guild_id, error } => {
                    if let Some(err) = error {
                        self.error = Some(err.into());
                    } else {
                        self.last_channel.remove(&Some(guild_id));
                        if self.last_guild == Some(guild_id) {
                            self.last_guild = None;
                        }
                        if self.place == Place::Guild(guild_id) {
                            self.place = Place::Home;
                            self.selected_channel = None;
                        }
                    }
                }
                UiEvent::SearchLoaded { seq, result } => {
                    self.search_loaded(seq, result, cx);
                }
                UiEvent::GroupDmCreated { seq, result } => {
                    self.group_dm_created(seq, result, cx);
                }
                UiEvent::GroupDmLeft { channel_id, error } => {
                    if let Some(err) = error {
                        self.error = Some(err.into());
                    } else {
                        if self.last_channel.get(&None).copied() == Some(channel_id) {
                            self.last_channel.remove(&None);
                        }
                        if self.selected_channel == Some(channel_id) {
                            self.clear_selection(cx);
                        }
                    }
                }
                UiEvent::ChannelCreated { seq, guild_id, result } => {
                    if !matches!(&self.overlay, Overlay::CreateChannel(m) if m.seq == seq) {
                        continue;
                    }
                    match result {
                        Ok(channel_id) => {
                            self.overlay = Overlay::None;
                            self.focus_composer_later(cx);
                            if self.place == Place::Guild(guild_id) {
                                let opens = self
                                    .backend
                                    .state()
                                    .channels
                                    .get(&channel_id)
                                    .is_some_and(|c| opens_message_pane(c.kind));
                                if opens {
                                    self.select_channel(channel_id, cx);
                                }
                            }
                        }
                        Err(err) => {
                            if let Overlay::CreateChannel(modal) = &mut self.overlay {
                                modal.submitting = false;
                                modal.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::ChannelUpdated { seq, error, .. } => {
                    if !matches!(&self.overlay, Overlay::EditChannel(m) if m.seq == seq) {
                        if self.channel_mgmt_seq == seq && let Some(error) = error {
                            self.error = Some(error.into());
                        }
                        continue;
                    }
                    match error {
                        None => {
                            self.overlay = Overlay::None;
                            self.focus_composer_later(cx);
                        }
                        Some(err) => {
                            if let Overlay::EditChannel(modal) = &mut self.overlay {
                                modal.submitting = false;
                                modal.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::AccessChanged { seq, error } => self.finish_access_change(seq, error, cx),
                UiEvent::ChannelDeleted { seq, channel_id, error } => {
                    match error {
                        Some(err) => {
                            if let Overlay::EditChannel(modal) = &mut self.overlay {
                                if modal.seq != seq {
                                    continue;
                                }
                                modal.submitting = false;
                                modal.confirming_delete = false;
                                modal.error = Some(err);
                            } else {
                                self.error = Some(err.into());
                            }
                        }
                        None => {
                            self.drafts.remove(&channel_id);
                            self.last_channel.retain(|_, v| *v != channel_id);
                            if matches!(&self.overlay, Overlay::EditChannel(m) if m.channel_id == channel_id) {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            if self.selected_channel == Some(channel_id) {
                                let next = {
                                    let state = self.backend.state();
                                    channel_items_with_active(
                                        &state,
                                        self.place,
                                        self.backend.media(),
                                        self.selected_channel,
                                        self.voice.channel_id,
                                    )
                                    .into_iter()
                                    .find(|c| c.has_messages && c.id != channel_id)
                                    .map(|c| c.id)
                                };
                                match next {
                                    Some(id) => self.select_channel(id, cx),
                                    None => self.clear_selection(cx),
                                }
                            }
                        }
                    }
                }
                UiEvent::MemberModerated { seq, guild_id, user_id, error } => {
                    if matches!(&self.overlay, Overlay::MemberModeration(m) if m.seq == seq) {
                        match error {
                            None => {
                                self.profile_cache.remove(&(user_id, Some(guild_id)));
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Some(err) => {
                                if let Overlay::MemberModeration(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    } else if matches!(&self.overlay, Overlay::GuildProfile(m) if m.seq == seq) {
                        match error {
                            None => {
                                self.profile_cache.remove(&(user_id, Some(guild_id)));
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Some(err) => {
                                if let Overlay::GuildProfile(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    } else if seq == self.moderation_seq && matches!(self.overlay, Overlay::None) {
                        if let Some(err) = error {
                            self.error = Some(err.into());
                        } else {
                            self.profile_cache.remove(&(user_id, Some(guild_id)));
                        }
                    }
                }
                UiEvent::GuildBansLoaded { seq, guild_id, result } => {
                    let Overlay::GuildBans(modal) = &mut self.overlay else {
                        continue;
                    };
                    if modal.seq != seq || modal.guild_id != guild_id {
                        continue;
                    }
                    modal.loading = false;
                    match result {
                        Ok(mut bans) => {
                            bans.retain(|b| !modal.recently_unbanned.get(&b.user.id).is_some_and(|&u_seq| u_seq > seq));
                            modal.recently_unbanned.retain(|_, &mut u_seq| u_seq > seq);
                            modal.bans = bans;
                            modal.error = None;
                        }
                        Err(err) => {
                            modal.error = Some(err);
                        }
                    }
                }
                UiEvent::MemberUnbanned { seq, guild_id, user_id, error } => {
                    let Overlay::GuildBans(modal) = &mut self.overlay else {
                        continue;
                    };
                    if modal.guild_id != guild_id || modal.unban_seqs.remove(&user_id) != Some(seq) {
                        continue;
                    }
                    modal.unbanning.remove(&user_id);
                    match error {
                        None => {
                            modal.bans.retain(|b| b.user.id != user_id);
                            modal.recently_unbanned.insert(user_id, seq);
                            modal.error = None;
                        }
                        Some(err) => {
                            modal.error = Some(err);
                        }
                    }
                }
                UiEvent::WebAuthnCredentialsLoaded { result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        modal.passkey_loading = false;
                        match result {
                            Ok(creds) => {
                                modal.passkeys = creds;
                                modal.passkey_error = None;
                            }
                            Err(err) => {
                                modal.passkey_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::WebAuthnMutationFinished { status, error } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        modal.passkey_busy = false;
                        modal.passkey_status = status.map(Into::into);
                        modal.passkey_error = error.clone().map(Into::into);
                        if error.is_none() {
                            modal.passkey_loading = true;
                            self.backend.load_webauthn_credentials();
                        }
                    }
                }
                UiEvent::ConnectionsLoaded { result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        modal.connections_loading = false;
                        modal.connections_loaded = true;
                        match result {
                            Ok(mut items) => {
                                items.sort_by_key(|c| c.sort_order);
                                modal.connections = items;
                                modal.connections_error = None;
                            }
                            Err(err) => {
                                modal.connections_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::ConnectionUpdated { result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        match result {
                            Ok(updated) => {
                                if let Some(slot) = modal.connections.iter_mut().find(|c| c.id == updated.id) {
                                    *slot = updated;
                                } else {
                                    modal.connections.push(updated);
                                }
                                modal.connections.sort_by_key(|c| c.sort_order);
                                modal.connections_error = None;
                            }
                            Err(err) => {
                                modal.connections_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::ApplicationsLoaded { result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        modal.applications_loading = false;
                        modal.applications_loaded = true;
                        match result {
                            Ok(items) => {
                                modal.applications = items;
                                modal.applications_error = None;
                            }
                            Err(err) => {
                                modal.applications_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::ApplicationLoaded { app_id, result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay
                        && modal.selected_application_id.as_deref() == Some(app_id.as_str())
                    {
                        modal.selected_application_loading = false;
                        match result {
                            Ok(app) => {
                                if let Some(slot) = modal.applications.iter_mut().find(|a| a.id == app.id) {
                                    *slot = app;
                                } else {
                                    modal.applications.push(app);
                                }
                                modal.applications_error = None;
                            }
                            Err(err) => {
                                modal.applications_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::PinsLoaded { seq, channel_id, result } => {
                    if let Overlay::ChannelPopout(pop) = &mut self.overlay
                        && let crate::ui::channel_popouts::ChannelPopoutKind::PinnedMessages {
                            channel_id: pop_ch,
                            seq: pop_seq,
                            loading,
                            items,
                            error: pop_err,
                        } = &mut pop.kind
                        && *pop_seq == seq
                        && *pop_ch == channel_id
                    {
                        *loading = false;
                        match result {
                            Ok(msgs) => {
                                *items = msgs;
                                *pop_err = None;
                            }
                            Err(err) => {
                                *pop_err = Some(err);
                            }
                        }
                    }
                }
                UiEvent::SavedMessagesLoaded { error } => {
                    if let Some(err) = error
                        && matches!(
                            &self.overlay,
                            Overlay::ChannelPopout(pop)
                                if matches!(pop.kind, crate::ui::channel_popouts::ChannelPopoutKind::Inbox { tab: crate::ui::channel_popouts::InboxTab::Bookmarks })
                        )
                    {
                        self.error = Some(err.into());
                    }
                }
                UiEvent::MessageForwarded { seq, target_channels: _, error } => {
                    if matches!(&self.overlay, Overlay::ForwardMessage(m) if m.seq == seq) {
                        match error {
                            None => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Some(err) => {
                                if let Overlay::ForwardMessage(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
                UiEvent::ThreadCreated { seq, result } => {
                    if matches!(&self.overlay, Overlay::CreateThread(m) if m.seq == seq) {
                        match result {
                            Ok(thread_id) => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                                self.select_channel(thread_id, cx);
                            }
                            Err(err) => {
                                if let Overlay::CreateThread(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
                UiEvent::InviteCreated { seq, channel_id, result } => {
                    if let Overlay::InviteMembers(modal) = &mut self.overlay
                        && modal.seq == seq
                        && modal.channel_id == channel_id
                    {
                        modal.loading = false;
                        match result {
                            Ok(inv) => {
                                modal.invite = Some(inv);
                                modal.error = None;
                            }
                            Err(err) => {
                                modal.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::GuildMessagesBulkDeleted { seq, guild_id, error } => {
                    if matches!(
                        &self.overlay,
                        Overlay::DeleteMyGuildMessages(m) if m.seq == seq && m.guild_id == guild_id
                    ) {
                        match error {
                            None => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Some(err) => {
                                if let Overlay::DeleteMyGuildMessages(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
                UiEvent::PersonalNotesPurged { seq, channel_id, result } => {
                    if matches!(
                        &self.overlay,
                        Overlay::PurgePersonalNotes(m) if m.seq == seq && m.channel_id == channel_id
                    ) {
                        match result {
                            Ok(_) => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Err(err) => {
                                if let Overlay::PurgePersonalNotes(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
                UiEvent::DiscoveryCategoriesLoaded { result } => {
                    if let Ok(cats) = result && !cats.is_empty() {
                        self.explore.categories = cats;
                    }
                }
                UiEvent::DiscoveryGuildsLoaded { seq, offset, result } => {
                    if self.explore.search_seq == seq {
                        self.explore.loading = false;
                        self.explore.has_loaded = true;
                        match result {
                            Ok(page) => {
                                if offset == 0 {
                                    self.explore.guilds = page.guilds;
                                } else {
                                    for g in page.guilds {
                                        if self.explore.guilds.iter().all(|existing| existing.id != g.id) {
                                            self.explore.guilds.push(g);
                                        }
                                    }
                                }
                                self.explore.total = page.total;
                                self.explore.error = None;
                            }
                            Err(err) => {
                                self.explore.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::DiscoveryGuildJoined { seq, guild_id, error } => {
                    if matches!(&self.overlay, Overlay::DiscoveryPreview(m) if m.join_seq == Some(seq)) {
                        match error {
                            None => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                                self.pending_joined_channel = None;
                                if self.backend.state().guilds.contains_key(&guild_id) {
                                    self.pending_joined_guild = None;
                                    self.select_place(Place::Guild(guild_id), cx);
                                } else {
                                    self.pending_joined_guild = Some(guild_id);
                                }
                            }
                            Some(err) => {
                                if let Overlay::DiscoveryPreview(modal) = &mut self.overlay {
                                    modal.joining = false;
                                    modal.join_seq = None;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
            }
        }
        if connected || guild_available {
            self.reload_cleared_channel();
        }
        self.refresh(cx);
        if self.user.is_some()
            && let Some((link, queued_at)) = self.pending_deep_link.take()
        {
            let expired = queued_at.elapsed() >= std::time::Duration::from_secs(10);
            let (view, handle) = (cx.weak_entity(), self.window_handle);
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = view.update(cx, |v, cx| {
                        if !v.try_apply_deep_link(&link, expired, window, cx) && !expired {
                            v.pending_deep_link = Some((link, queued_at));
                        }
                    });
                });
            });
        }
        if let Some((jump_ch, target)) = self.pending_jump
            && Some(jump_ch) == self.selected_channel
            && !self.guild_unavailable
            && !self.loading_history
        {
            self.pending_jump = None;
            if !self.reveal_jump(jump_ch, target, cx) && self.error.is_none() {
                self.error = Some(l10n::tr(target.missing_label()).into());
            }
            cx.notify();
        }
    }

    /// After READY the windows of a new session are cleared (and those of
    /// the same session invalidated): retry a pending jump or reload the
    /// current channel's latest page.
    fn reload_cleared_channel(&mut self) {
        let Some(channel_id) = self.selected_channel else { return };
        let (can_view, cleared) = {
            let state = self.backend.state();
            if state.is_unavailable_channel(channel_id) {
                return;
            }
            (state.can_view_channel(channel_id), state.messages.window(channel_id).is_none_or(|w| !w.ready))
        };
        if !can_view {
            self.loading_history = false;
            self.pending_jump = None;
            return;
        }
        if cleared {
            self.loading_history = true;
            if let Some((channel, target)) = self.pending_jump
                && channel == channel_id
            {
                self.backend.load_around(channel_id, target.anchor());
            } else {
                self.backend.load_messages(channel_id, None);
            }
        }
    }

    /// The message list scrolled (or its rows changed): remember whether it
    /// shows the newest message (incoming messages trim the far side only
    /// then), and load older / newer pages at either end.
    pub(crate) fn scrolled(&mut self, scrolled_up: bool, at_top: bool, cx: &mut Context<Self>) {
        if self.suppress_jump_scroll {
            if scrolled_up && !at_top {
                self.suppress_jump_scroll = false;
            } else {
                return;
            }
        }
        if let Some(channel_id) = self.selected_channel {
            self.backend.state().messages.set_pinned_to_end(channel_id, !scrolled_up);
        }
        if at_top {
            self.load_older(cx);
        } else if !scrolled_up {
            self.load_newer(cx);
        }
        self.maybe_ack(cx);
    }

    /// Re-reads the state and updates every snapshot, the scroller rows and
    /// the composer.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.voice = self.backend.voice_snapshot();
        let (
            pane,
            can_send,
            title,
            topic,
            glyph,
            typing_text,
            can_manage_guild,
            can_manage_selected,
            dismiss_modal,
            joined_ready,
            joined_channel_ready,
        ) = {
            let state = self.backend.state();
            let joined_ready = self.pending_joined_guild.filter(|id| state.guilds.contains_key(id));
            let joined_channel_ready = self.pending_joined_channel.filter(|id| state.channels.contains_key(id));
            if state.user_settings.raw.is_object() {
                if state.user_settings.raw.get("message_display_compact").is_some() {
                    let server_compact = state.user_settings.message_display_compact();
                    if self.prefs.data.compact != server_compact {
                        self.prefs.set_compact(server_compact);
                    }
                }
                if state.user_settings.raw.get("trusted_domains").is_some() {
                    self.prefs.set_trusted_domains(state.user_settings.trusted_domains());
                }
            }
            self.guild_unavailable = matches!(self.place, Place::Guild(id) if state.unavailable_guilds.contains(&id));
            if self.guild_unavailable {
                self.loading_history = false;
            }
            if let Place::Guild(id) = self.place
                && !state.guilds.contains_key(&id)
                && !self.guild_unavailable
            {
                self.place = Place::Home;
                self.selected_channel = None;
                self.last_channel.remove(&Some(id));
                if self.last_guild == Some(id) {
                    self.last_guild = None;
                }
            }
            let mut fallback_guild_channel: Option<Option<Snowflake>> = None;
            let retains_channel = |id: Snowflake| state.channels.contains_key(&id) || state.is_unavailable_channel(id);
            if let Some(removed_id) = self.selected_channel.filter(|id| !retains_channel(*id)) {
                self.drafts.remove(&removed_id);
                self.last_channel.retain(|_, v| *v != removed_id);
                self.selected_channel = None;
                if matches!(self.place, Place::Guild(_)) {
                    let next = channel_items_with_active(
                        &state,
                        self.place,
                        self.backend.media(),
                        self.selected_channel,
                        self.voice.channel_id,
                    )
                    .into_iter()
                    .find(|c| c.has_messages)
                    .map(|c| c.id);
                    fallback_guild_channel = Some(next);
                }
            } else if self.selected_channel.is_some_and(|id| !state.can_view_channel(id))
                && matches!(self.place, Place::Guild(_))
                && !self.guild_unavailable
            {
                let next = channel_items_with_active(
                    &state,
                    self.place,
                    self.backend.media(),
                    self.selected_channel,
                    self.voice.channel_id,
                )
                .into_iter()
                .find(|c| c.has_messages)
                .map(|c| c.id);
                fallback_guild_channel = Some(next);
            }
            if state.current_user_id().is_some() {
                self.drafts.retain(|id, _| retains_channel(*id));
                self.last_channel.retain(|_, v| retains_channel(*v));
            }
            self.attachments.retain_channels(retains_channel);
            let dismiss_modal = match &self.overlay {
                Overlay::CreateChannel(m) => {
                    !state.guilds.contains_key(&m.guild_id)
                        || match m.parent_id {
                            None => !state.can_manage_guild_channels(m.guild_id),
                            Some(pid) => {
                                !state
                                    .channels
                                    .get(&pid)
                                    .is_some_and(|c| c.guild_id == Some(m.guild_id) && c.kind.is_category())
                                    || !state.can_manage_channel(pid)
                            }
                        }
                }
                Overlay::EditChannel(m) => {
                    !state.guilds.contains_key(&m.guild_id)
                        || !state.channels.contains_key(&m.channel_id)
                        || !state.can_manage_channel(m.channel_id)
                }
                Overlay::MemberModeration(m) => {
                    let now = i64::try_from(crate::accounts::now_ms()).unwrap_or(i64::MAX);
                    !state.guilds.contains_key(&m.guild_id)
                        || !match &m.kind {
                            ui::moderation_modal::ModerationKind::Kick => state.can_kick_member(m.guild_id, m.user_id),
                            ui::moderation_modal::ModerationKind::Ban => state.can_ban_member(m.guild_id, m.user_id),
                            ui::moderation_modal::ModerationKind::Timeout { .. } => {
                                state.can_timeout_member(m.guild_id, m.user_id)
                            }
                            ui::moderation_modal::ModerationKind::Nickname { .. } => {
                                state.can_edit_member_nickname(m.guild_id, m.user_id, now)
                            }
                        }
                }
                Overlay::GuildBans(m) => {
                    self.place != Place::Guild(m.guild_id)
                        || !state.guilds.contains_key(&m.guild_id)
                        || !state.can_ban_members(m.guild_id)
                }
                Overlay::Access(m) => !m.allowed(self.place, &state),
                Overlay::InviteMembers(m) => {
                    !state.guilds.contains_key(&m.guild_id)
                        || !ui::invite_modal::can_invite_in_guild(&state, m.guild_id, Some(m.channel_id))
                }
                Overlay::GuildNotifications(m) => !state.guilds.contains_key(&m.guild_id),
                Overlay::GuildPrivacy(m) => !state.guilds.contains_key(&m.guild_id),
                Overlay::GuildProfile(m) => !state.guilds.contains_key(&m.guild_id),
                Overlay::DeleteMyGuildMessages(m) => {
                    !state.guilds.contains_key(&m.guild_id)
                        || state
                            .current_user_id()
                            .is_some_and(|me| state.guilds.get(&m.guild_id).is_some_and(|g| g.owner_id == Some(me)))
                }
                _ => false,
            };
            if dismiss_modal {
                self.overlay = Overlay::None;
                self.focus_composer_later(cx);
            }
            if let Some(next_opt) = fallback_guild_channel {
                drop(state);
                match next_opt {
                    Some(id) => self.select_channel(id, cx),
                    None => self.clear_selection(cx),
                }
                return;
            }
            if !dismiss_modal && let Overlay::EditChannel(m) = &mut self.overlay {
                m.sync_state(&state);
            }
            self.guilds = guild_items_with_media(&state, self.backend.media());
            self.home_unread = dm_unread(&state);
            self.mention_total = self
                .guilds
                .iter()
                .fold(self.home_unread.mention_count, |n, g| n.saturating_add(g.unread.mention_count));
            self.user = current_user(&state, self.backend.media());
            self.pending_incoming = friends::pending_incoming_count(&state);
            self.friend_rows = if self.place == Place::Home && self.selected_channel.is_none() {
                friends::build_friend_rows(&state, self.backend.media(), self.friends_tab)
            } else {
                Vec::new()
            };
            self.channels = channel_items_with_active(
                &state,
                self.place,
                self.backend.media(),
                self.selected_channel,
                self.voice.channel_id,
            );
            let target = match (self.place, self.selected_channel) {
                (Place::Guild(guild_id), Some(channel_id))
                    if state.channels.get(&channel_id).is_some_and(|c| c.guild_id == Some(guild_id)) =>
                {
                    Some((guild_id, channel_id))
                }
                _ => None,
            };
            if target != self.member_target {
                // Another channel: start at the top with nothing reported.
                self.member_target = target;
                self.member_visible.set(None);
                self.member_scroll = UniformListScrollHandle::new();
            }
            *self.member_panel.borrow_mut() = match target {
                Some((guild_id, channel_id)) => {
                    member_list::member_panel(&state, guild_id, channel_id, self.backend.media())
                }
                None => MemberPanel::default(),
            };
            let include_addable = self.group_dm_panel_open && self.search.is_none() && self.group_dm_adding;
            self.group_dm = match (self.place, self.selected_channel) {
                (Place::Home, Some(id)) => ui::group_dm::build_snapshot(&state, self.backend.media(), id, include_addable),
                _ => None,
            };
            // A reply whose target was deleted is dropped.
            let reply_gone = match self.composer_mode.mode() {
                ComposerMode::Replying(t)
                    if state.messages.window(t.channel_id).is_some_and(|window| {
                        window.ready && window.messages.iter().all(|message| message.id != t.message_id)
                    }) =>
                {
                    Some(t.message_id)
                }
                _ => None,
            };
            if let Some(id) = reply_gone {
                self.composer_mode.message_deleted(id);
            }
            self.can_ban_members = match self.place {
                Place::Guild(gid) => state.can_ban_members(gid),
                Place::Home | Place::Explore => false,
            };
            let can_manage_guild = match self.place {
                Place::Guild(gid) => state.can_manage_guild_channels(gid),
                Place::Home | Place::Explore => false,
            };
            let can_manage_selected = self
                .selected_channel
                .filter(|_| matches!(self.place, Place::Guild(_)))
                .is_some_and(|id| state.can_manage_channel(id));
            match self.selected_channel {
                Some(id) => {
                    if self.unread_anchor.is_none() && self.scroller.read(cx).is_scrolled_up() {
                        self.unread_anchor = state.unread_anchor(id);
                    }
                    let channel = state.channels.get(&id);
                    let topic = channel
                        .and_then(|c| c.topic.as_deref())
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(SharedString::from);
                    (
                        message_pane(&state, id, self.backend.media(), self.unread_anchor),
                        !self.guild_unavailable && state.can_send_messages(id),
                        channel.map(channel_name).unwrap_or_default(),
                        topic,
                        channel.map_or("", |c| ChannelIcon::of(c.kind).glyph()),
                        ui::typing::channel_typing_text(&state, id),
                        can_manage_guild,
                        can_manage_selected,
                        dismiss_modal,
                        joined_ready,
                        joined_channel_ready,
                    )
                }
                None => (
                    Default::default(),
                    false,
                    String::new(),
                    None,
                    "",
                    None,
                    can_manage_guild,
                    false,
                    dismiss_modal,
                    joined_ready,
                    joined_channel_ready,
                ),
            }
        };
        if self.guild_unavailable {
            self.cancel_ack();
        }
        if dismiss_modal {
            self.overlay = Overlay::None;
            self.focus_composer_later(cx);
        }
        if let Some(joined) = joined_ready {
            self.pending_joined_guild = None;
            self.pending_joined_channel = None;
            self.select_place(Place::Guild(joined), cx);
            self.refresh_open_profile();
            return;
        }
        if let Some(ch) = joined_channel_ready {
            self.pending_joined_channel = None;
            self.open_channel(None, ch, cx);
            self.refresh_open_profile();
            return;
        }
        theme::sync_display_global(self.prefs.data.display(), cx);
        self.typing_text = typing_text;
        self._typing_timer = ui::typing::schedule_expiry(&self.backend, cx, Self::refresh);
        self.has_more_before = pane.has_more_before;
        self.has_more_after = pane.has_more_after;
        self.channel_title = title.into();
        self.channel_topic = topic;
        self.channel_glyph = glyph;
        self.can_manage_guild_channels = can_manage_guild;
        self.can_manage_selected_channel = can_manage_selected;
        self.backend.notify_focus().set_viewing(self.selected_channel.filter(|_| !self.guild_unavailable));
        let mut rows = pane.rows;
        for row in &rows {
            if let Some(ui::reply::ReplyLine::NotLoaded { channel_id, message_id }) = row.reply {
                drop(self.backend.fetch_reply_target(channel_id, message_id));
            }
        }
        if !self.uploads.is_empty() {
            for row in rows.iter_mut().filter(|r| r.state == MessageState::Sending) {
                row.upload_percent = self.uploads.get(&row.id).copied();
            }
        }
        self.sync_rows(rows, cx);
        self.set_can_send(can_send, cx);
        self.own_status = self.backend.own_status();
        self.refresh_create_group_dm_candidates();
        let next_sync = self.group_dm.as_ref().map(|g| (g.channel_id, g.custom_name.clone()));
        if next_sync != self.group_dm_rename_synced {
            let old_channel = self.group_dm_rename_synced.as_ref().map(|(id, _)| *id);
            let new_channel = next_sync.as_ref().map(|(id, _)| *id);
            let channel_changed = old_channel != new_channel;
            self.group_dm_rename_synced = next_sync;
            if channel_changed {
                self.group_dm_adding = false;
            }
            let text = self
                .group_dm
                .as_ref()
                .and_then(|g| g.custom_name.clone())
                .unwrap_or_default();
            let (entity, handle) = (self.group_dm_rename.clone(), self.window_handle);
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    if channel_changed || !entity.read(cx).focus_handle(cx).is_focused(window) {
                        entity.update(cx, |s, cx| s.set_value(text, window, cx));
                    }
                });
            });
        }
        self.refresh_open_profile();
        self.subscribe_member_list();
        self.maybe_ack(cx);
        cx.notify();
    }
}
