//! Composer, send/edit/reply/reaction actions and message jumps.

use super::*;

impl ClientView {
    /// Enter in the composer: send, reply or save the edit as the composer
    /// mode decides.
    pub(super) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Enter during IME composition only commits the composition.
        if self.composer.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
            return;
        }
        let Some(channel_id) = self.selected_channel else { return };
        let editing = matches!(self.composer_mode.mode(), ComposerMode::Editing(_));
        let can_submit = {
            let state = self.backend.state();
            if editing {
                state.can_view_channel(channel_id)
            } else {
                self.can_send && state.can_send_messages(channel_id)
            }
        };
        if !can_submit {
            return;
        }
        // Mention / channel / custom emoji tokens go out in wire form.
        let text = composer_text::wire_text(self.composer.read(cx));
        // The server counts UTF-16 units; an over-long message keeps the
        // draft instead of failing after the input was cleared.
        if ui::attachments::is_long_text(text.trim()) {
            self.error = Some(l10n::tr("Message is too long").into());
            cx.notify();
            return;
        }
        if !editing && !self.attachments.files(channel_id).is_empty() {
            self.submit_with_files(channel_id, text, window, cx);
            return;
        }
        match self.composer_mode.submit(&text) {
            Submit::Nothing => {}
            Submit::Send { content } => {
                if self.backend.send_message(channel_id, content) {
                    self.after_send(window, cx);
                } else {
                    self.not_ready(cx);
                }
            }
            Submit::Reply { content, target } => {
                let (reply_to, mention) = (target.message_id, target.mention);
                if self.backend.send_reply(target.channel_id, content, reply_to, mention) {
                    self.after_send(window, cx);
                } else {
                    self.composer_mode.start_reply(target);
                    self.not_ready(cx);
                }
            }
            Submit::Edit { channel_id, message_id, content, restore } => {
                self.backend.edit_message(channel_id, message_id, content);
                self.set_composer(restore, window, cx);
                self.error = None;
                self.refresh(cx);
            }
            Submit::EditUnchanged { restore } => {
                self.set_composer(restore, window, cx);
                cx.notify();
            }
            Submit::ConfirmDelete { channel_id, message_id } => {
                self.confirm_delete(channel_id, message_id, window, cx);
            }
        }
    }

    pub(crate) fn after_send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.manual_unread_channel = None;
        self.set_composer(String::new(), window, cx);
        if let Some(id) = self.selected_channel {
            self.drafts.remove(&id);
        }
        self.typing.message_sent();
        self.error = None;
        self.refresh(cx);
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
    }

    pub(crate) fn not_ready(&mut self, cx: &mut Context<Self>) {
        self.error = Some(l10n::tr("Check your connection and try again.").into());
        cx.notify();
    }

    /// Stores a non-empty draft for `channel_id` (evicting the lowest-ID
    /// entry if the `MAX_DRAFTS` map cap is reached).
    pub(super) fn stash_channel_draft(&mut self, channel_id: Snowflake, content: InputContent) {
        const MAX_DRAFTS: usize = 100;
        if content.text().is_empty() && content.tokens().is_empty() {
            self.drafts.remove(&channel_id);
            return;
        }
        if !self.drafts.contains_key(&channel_id)
            && self.drafts.len() >= MAX_DRAFTS
            && let Some(oldest) = self.drafts.keys().next().copied()
        {
            self.drafts.remove(&oldest);
        }
        self.drafts.insert(channel_id, content);
    }

    pub(super) fn current_composer_content(&mut self, cx: &mut Context<Self>) -> InputContent {
        self.pending_composer.take().unwrap_or_else(|| self.composer.read(cx).content())
    }

    /// Replaces the input (text and tokens). `set_value` emits no change
    /// event, so the popup computed for the old text is dropped here: its
    /// byte range would otherwise be applied to the new text.
    fn set_composer(&mut self, content: impl Into<InputContent>, window: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        self.composer_seq.set(self.composer_seq.get().wrapping_add(1));
        self.pending_composer = None;
        let content = content.into();
        self.composer.update(cx, |s, cx| s.set_value(content, window, cx));
    }

    /// [`Self::set_composer`] from a call without the window (channel
    /// switch): runs after the current update.
    pub(super) fn set_composer_later(&mut self, content: InputContent, cx: &mut Context<Self>) {
        self.autocomplete = None;
        let seq = self.composer_seq.get().wrapping_add(1);
        self.composer_seq.set(seq);
        self.pending_composer = Some(content.clone());
        let (composer_seq, view, composer, handle) =
            (self.composer_seq.clone(), cx.weak_entity(), self.composer.clone(), self.window_handle);
        cx.defer(move |cx| {
            if composer_seq.get() != seq {
                return;
            }
            let _ = view.update(cx, |v, _| v.pending_composer = None);
            let _ = handle.update(cx, |_, window, cx| composer.update(cx, |s, cx| s.set_value(content, window, cx)));
        });
    }

    /// IME composition in progress: Enter / Tab / arrows / Esc belong to
    /// the IME (`ComposerIME.ts`), not to the popup or the composer modes.
    pub(super) fn composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.composer.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some()
    }

    pub(crate) fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.composer.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// The composer text changed: the typing signal.
    pub(super) fn composer_changed(&mut self, cx: &mut Context<Self>) {
        self.pending_composer = None;
        self.composer_seq.set(self.composer_seq.get().wrapping_add(1));
        let Some(channel_id) = self.selected_channel.filter(|_| self.can_send) else { return };
        let text = self.composer.read(cx).value().to_string();
        match self.composer_mode.typing_signal(&text) {
            TypingSignal::Started => self.backend.typing_started(&self.typing, channel_id),
            TypingSignal::Stopped => self.typing.stopped(channel_id),
            TypingSignal::None => {}
        }
    }

    /// Esc in the composer: during IME composition it only ends the
    /// composition (`event.isComposing` returns early in the web client);
    /// otherwise it closes the autocomplete popup, else the quick
    /// reactions, else cancels a reply or edit; with none of those it marks
    /// the channel read (`useChannelComposerGlobalShortcuts` dispatches
    /// `ESCAPE_PRESSED` after the composer affordances). False lets the
    /// input handle it.
    pub(super) fn on_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.composing(window, cx) {
            self.composer.update(cx, |s, cx| s.unmark_text(window, cx));
            return true;
        }
        if self.close_floating_popouts(window, cx) {
            return true;
        }
        if self.autocomplete.take().is_some() {
            cx.notify();
            return true;
        }
        if self.reaction_picker.take().is_some() {
            cx.notify();
            return true;
        }
        if self.cancel_composer_mode(window, cx) {
            return true;
        }
        if self.search.is_some() {
            self.close_search(window, cx);
            return true;
        }
        self.mark_channel_read(cx);
        false
    }

    /// A popout, menu, picker or modal is open.
    pub(super) fn layer_open(&self) -> bool {
        self.overlay.is_open()
            || self.user_area_popout_open
            || self.guild_header_popout_open
            || self.voice_popout.is_some()
    }

    /// Closes the non-overlay popouts (user area, guild header, voice) and returns
    /// focus to the composer; false when none was open.
    pub(super) fn close_floating_popouts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !(self.user_area_popout_open || self.guild_header_popout_open || self.voice_popout.is_some()) {
            return false;
        }
        self.user_area_popout_open = false;
        self.user_area_submenu = None;
        self.guild_header_popout_open = false;
        self.voice_popout = None;
        self.focus_composer(window, cx);
        cx.notify();
        true
    }

    /// Leaves replying / editing (the edit's draft comes back).
    pub(super) fn cancel_composer_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.composer_mode.escape() {
            EscapeOutcome::Ignored => false,
            EscapeOutcome::CancelledReply => {
                cx.notify();
                true
            }
            EscapeOutcome::CancelledEdit { restore } => {
                self.set_composer(restore, window, cx);
                cx.notify();
                true
            }
        }
    }

    /// Up moves in the autocomplete popup; in an empty composer it edits
    /// the last own message.
    pub(super) fn on_up(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.composing(window, cx) {
            return false;
        }
        if self.autocomplete.is_some() {
            self.autocomplete_move(-1, cx);
            return true;
        }
        let Some(channel_id) = self.selected_channel else { return false };
        let empty = self.composer.read(cx).value().is_empty();
        let backend = &self.backend;
        match self.composer_mode.arrow_up(empty, || last_own_message(&backend.state(), channel_id)) {
            Some(text) => {
                self.set_composer(text, window, cx);
                cx.notify();
                true
            }
            None => false,
        }
    }

    /// A button of a message's hover action bar.
    pub(crate) fn message_action(
        &mut self,
        action: MessageAction,
        target: &ActionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (channel_id, message_id) = (target.channel_id, target.message_id);
        match action {
            MessageAction::React => {
                self.reaction_picker = (self.reaction_picker != Some(message_id)).then_some(message_id);
                cx.notify();
            }
            MessageAction::Reply => self.start_reply(channel_id, message_id, window, cx),
            MessageAction::Edit => self.start_edit(channel_id, message_id, window, cx),
            MessageAction::Retry => self.retry_failed_message(channel_id, message_id, cx),
            MessageAction::Delete => {
                if target.sent {
                    self.confirm_delete(channel_id, message_id, window, cx);
                } else {
                    self.delete_message(channel_id, message_id, window, cx);
                }
            }
            MessageAction::DeleteNow => self.delete_message(channel_id, message_id, window, cx),
            MessageAction::CopyText | MessageAction::CopyId => {
                message_actions::copy_to_clipboard(action, target, cx);
            }
            MessageAction::SuppressEmbeds => {
                self.backend.toggle_suppress_embeds(channel_id, message_id);
            }
            MessageAction::More => self.open_message_menu(target.clone(), window, cx),
            MessageAction::Forward => self.open_forward_modal(target.clone(), window, cx),
            MessageAction::CreateThread => self.open_create_thread_modal(target.clone(), window, cx),
            MessageAction::Translate => self.translate_message("google_translate", &target.content, window, cx),
            MessageAction::Pin => {
                let pinned = target.pinned;
                let view = cx.weak_entity();
                message_actions::confirm_pin(pinned, target.content.clone(), window, cx, move |_, cx| {
                    let _ = view.update(cx, |v, cx| {
                        v.backend.toggle_pin(channel_id, message_id, !pinned);
                        v.refresh(cx);
                    });
                });
            }
            MessageAction::PinNow => {
                self.backend.toggle_pin(channel_id, message_id, !target.pinned);
                self.refresh(cx);
            }
            MessageAction::Bookmark => {
                self.backend.toggle_bookmark(channel_id, message_id);
                self.refresh(cx);
            }
            MessageAction::MarkUnread => self.mark_message_unread(channel_id, message_id, cx),
            MessageAction::CopyLink => {
                let url = self.backend.message_link(target.guild_id, channel_id, message_id);
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(url));
            }
            MessageAction::Speak => {
                if self.tts.is_speaking() {
                    self.tts.stop();
                } else if let Err(err) = self.tts.speak(target.content.to_string()) {
                    self.error = Some(err.into());
                }
                cx.notify();
            }
        }
    }

    pub(crate) fn mark_message_unread(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        cx: &mut Context<Self>,
    ) {
        self.cancel_ack();
        let Some(ack_id) = self.backend.mark_message_unread(channel_id, message_id) else {
            return;
        };
        self.manual_unread_channel = Some(channel_id);
        if Some(channel_id) == self.selected_channel {
            self.unread_anchor = Some(ack_id);
        }
        self.refresh(cx);
    }

    pub(crate) fn translate_message(
        &mut self,
        provider_id: &str,
        content: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(url) = message_actions::build_translate_url(provider_id, content) {
            self.launch_external_url(&url, cx);
        }
    }

    /// Something only the rows draw changed (an embed spoiler was revealed):
    /// redraw the list.
    pub(crate) fn redraw_messages(&mut self, cx: &mut Context<Self>) {
        // The rows are drawn inside the scroller entity; redraw it too.
        self.scroller.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(crate) fn start_reply(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = {
            let state = self.backend.state();
            state.messages.message(channel_id, message_id).map(|m| ReplyingTo::of(&state, m))
        };
        let Some(target) = target else { return };
        if let Some(restore) = self.composer_mode.start_reply(target) {
            self.set_composer(restore, window, cx);
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn start_edit(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.backend.state().messages.message(channel_id, message_id).map(EditTarget::of);
        let Some(target) = target else { return };
        let current = self.current_composer_content(cx);
        let text = self.composer_mode.start_edit(target, current);
        self.set_composer(text, window, cx);
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// The "Delete Message" dialog; deletes when confirmed.
    fn confirm_delete(&mut self, channel_id: Snowflake, message_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        let preview = self.backend.state().messages.message(channel_id, message_id).map(|m| m.content.clone());
        let view = cx.weak_entity();
        message_actions::confirm_delete(preview.unwrap_or_default().into(), window, cx, move |window, cx| {
            let _ = view.update(cx, |v, cx| v.delete_message(channel_id, message_id, window, cx));
        });
    }

    fn delete_message(&mut self, channel_id: Snowflake, message_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        self.backend.delete_message(channel_id, message_id);
        if let Some(restore) = self.composer_mode.message_deleted(message_id) {
            self.set_composer(restore, window, cx);
        }
        self.refresh(cx);
    }

    pub(crate) fn retry_failed_message(&mut self, channel_id: Snowflake, message_id: Snowflake, cx: &mut Context<Self>) {
        if self.backend.retry_failed_message(channel_id, message_id) {
            self.error = None;
            self.refresh(cx);
        }
    }

    /// A reaction pill was clicked: adds or removes the user's reaction.
    pub(crate) fn toggle_reaction(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        emoji: PartialEmoji,
        cx: &mut Context<Self>,
    ) {
        if self.backend.toggle_reaction(channel_id, message_id, emoji) {
            self.refresh(cx);
        }
    }

    /// The pointer moved over message `message_id`'s row, or over no
    /// message (a system row, a divider, outside the list): `None`.
    pub(crate) fn hover_message(&mut self, message_id: Option<Snowflake>, cx: &mut Context<Self>) {
        if message_id != self.hovered_message {
            self.hovered_message = message_id;
            // The rows are drawn inside the scroller entity; redraw it too.
            self.scroller.update(cx, |_, cx| cx.notify());
            cx.notify();
        }
    }

    /// An emoji of the quick reactions: closes them and toggles it.
    pub(crate) fn pick_reaction(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        emoji: PartialEmoji,
        cx: &mut Context<Self>,
    ) {
        self.reaction_picker = None;
        self.toggle_reaction(channel_id, message_id, emoji, cx);
        cx.notify();
    }

    /// Opens `channel_id` (and switches guild/Home when needed), scrolls to
    /// `message_id` and highlights it; if outside the current cached window,
    /// loads a `?around=` slice without firing a redundant latest-page fetch.
    pub(crate) fn jump_to_channel_message(
        &mut self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
        message_id: Snowflake,
        cx: &mut Context<Self>,
    ) {
        let (opens, resolved_guild, already_loaded) = {
            let state = self.backend.state();
            let Some(ch) = state.channels.get(&channel_id) else { return };
            let opens = opens_message_pane(ch.kind) && state.can_view_channel(channel_id);
            let resolved_guild = guild_id.or(ch.guild_id);
            let already_loaded = state
                .messages
                .window(channel_id)
                .is_some_and(|w| w.ready && w.messages.iter().any(|m| m.id == message_id));
            (opens, resolved_guild, already_loaded)
        };
        if !opens {
            return;
        }
        let target_place = resolved_guild.map_or(Place::Home, Place::Guild);
        if self.place != target_place {
            self.place = target_place;
            if let Some(gid) = resolved_guild {
                self.last_guild = Some(gid);
                self.backend.subscribe_guild(gid);
            }
        }
        if self.selected_channel != Some(channel_id) {
            if !self.select_channel_inner(channel_id, false, cx) {
                return;
            }
        } else {
            self.refresh(cx);
        }
        if already_loaded {
            self.pending_jump = None;
            self.reveal_jump(channel_id, unread::JumpTarget::Message(message_id), cx);
        } else {
            self.pending_jump = Some((channel_id, unread::JumpTarget::Message(message_id)));
            self.highlighted_message = None;
            self.loading_history = true;
            self.backend.load_around(channel_id, message_id);
        }
        cx.notify();
    }

    /// Opens the message's channel and scrolls to it when it is loaded
    /// (reply lines, the "Replying to" bar).
    pub(crate) fn jump_to_message(&mut self, channel_id: Snowflake, message_id: Snowflake, cx: &mut Context<Self>) {
        self.jump_to_channel_message(None, channel_id, message_id, cx);
    }
}
