//! Keybind recording, shortcut dispatch and keyboard resize handlers.

use super::*;

impl ClientView {
    pub(crate) fn focus_root(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    pub(crate) fn is_recording_keybind(&self) -> bool {
        matches!(
            &self.overlay,
            Overlay::Settings(modal) if modal.keybinds_recording_id.is_some()
        )
    }

    pub(crate) fn should_run_builtin_shortcut(&self, action: &str) -> bool {
        !self.is_recording_keybind() && shortcuts::is_builtin_action_active(&self.prefs.data, action)
    }

    pub(crate) fn is_composer_empty_or_unfocused(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        let composer_focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        !composer_focused || self.composer.read(cx).value().is_empty()
    }

    pub(crate) fn dispatch_keybind_action(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match action {
            "nav_channel_prev" => self.cycle_channel(-1, cx),
            "nav_channel_next" => self.cycle_channel(1, cx),
            "nav_unread_prev" => self.cycle_filtered(crate::nav::ChannelFilter::Unread, -1, cx),
            "nav_unread_next" => self.cycle_filtered(crate::nav::ChannelFilter::Unread, 1, cx),
            "nav_mention_prev" => self.cycle_filtered(crate::nav::ChannelFilter::Mentioned, -1, cx),
            "nav_mention_next" => self.cycle_filtered(crate::nav::ChannelFilter::Mentioned, 1, cx),
            "nav_guild_prev" => self.cycle_guild(-1, cx),
            "nav_guild_next" => self.cycle_guild(1, cx),
            "nav_guild_tab_prev" => self.cycle_guild(-1, cx),
            "nav_guild_tab_next" => self.cycle_guild(1, cx),
            "nav_guild_slot_1" => self.select_guild_slot(0, cx),
            "nav_guild_slot_2" => self.select_guild_slot(1, cx),
            "nav_guild_slot_3" => self.select_guild_slot(2, cx),
            "nav_guild_slot_4" => self.select_guild_slot(3, cx),
            "nav_guild_slot_5" => self.select_guild_slot(4, cx),
            "nav_guild_slot_6" => self.select_guild_slot(5, cx),
            "nav_guild_slot_7" => self.select_guild_slot(6, cx),
            "nav_guild_slot_8" => self.select_guild_slot(7, cx),
            "nav_guild_slot_9" => self.select_guild_slot(8, cx),
            "nav_current_call" => self.jump_to_current_call(cx),
            "nav_history_back" => self.history_back(cx),
            "nav_history_forward" => self.history_forward(cx),
            "nav_toggle_last_guild_dms" => self.toggle_last_guild(cx),
            "nav_add_guild" => self.open_join_guild(window, cx),
            "misc_search" => self.open_search(window, cx),
            "nav_quick_switcher" => self.toggle_quick_switcher(window, cx),
            "chat_focus_textarea" => self.focus_composer(window, cx),
            "chat_upload" => self.pick_files(cx),
            "chat_toggle_emoji" => self.toggle_emoji_picker(window, cx),
            "chat_toggle_gif" => self.toggle_gif_picker(window, cx),
            "chat_toggle_sticker" => self.toggle_sticker_picker(window, cx),
            "chat_toggle_pins" => self.toggle_pins_shortcut(window, cx),
            "chat_toggle_inbox" => self.toggle_inbox_shortcut(window, cx),
            "chat_toggle_member_list" => self.toggle_member_list(cx),
            "chat_scroll_up" => self.scroll_chat_page(-1, cx),
            "chat_scroll_down" => self.scroll_chat_page(1, cx),
            "chat_jump_oldest_unread" => self.jump_to_unread(cx),
            "chat_mark_channel_read" => self.mark_channel_read(cx),
            "chat_mark_guild_read" => self.mark_guild_read(cx),
            "chat_mark_inbox_read" => self.mark_top_inbox_read(cx),
            "chat_mark_all_inbox_read" => self.mark_all_inbox_read(cx),
            "chat_new_dm" => self.open_create_group_dm(window, cx),
            "chat_copy_channel_link" => self.copy_channel_link(cx),
            "system_toggle_settings" => self.open_settings(window, cx),
            "system_zoom_in" => self.step_ui_zoom(10, cx),
            "system_zoom_out" => self.step_ui_zoom(-10, cx),
            "system_zoom_reset" => self.settings_set_zoom(100, cx),
            "voice_toggle_mute" => self.toggle_voice_mute(cx),
            "voice_toggle_deafen" => self.toggle_voice_deaf(cx),
            "voice_toggle_soundboard" => self.toggle_soundboard(cx),
            "voice_start_dm_call" => self.start_dm_voice_call(cx),
            "voice_answer_call" => self.answer_incoming_call(cx),
            "voice_decline_call" => self.decline_incoming_call(cx),
            "voice_toggle_compact_call_view" => self.toggle_compact_call_view(cx),
            "voice_toggle_camera" => self.toggle_voice_camera(cx),
            "voice_switch_channel" => self.switch_voice_channel(cx),
            "voice_disconnect" => self.leave_voice_channel(cx),
            "voice_push_to_talk" => self.trigger_voice_push_to_talk(cx),
            "voice_push_to_talk_priority" => self.trigger_voice_push_to_talk_priority(cx),
            "voice_push_to_mute" => self.trigger_voice_push_to_mute(cx),
            "voice_priority_vad" => self.trigger_voice_priority_vad(cx),
            "voice_toggle_vad" => self.toggle_voice_vad(cx),
            "chat_toggle_saved_media" => self.toggle_saved_media_shortcut(window, cx),
            "chat_send_voice_message" => self.start_voice_message(cx),
            "system_toggle_shortcuts_overlay" => self.toggle_shortcuts_overlay(window, cx),
            "system_toggle_downloads" => self.toggle_downloads_panel(cx),
            "system_open_theme_studio_popout" => self.open_theme_studio_popout(cx),
            "system_toggle_custom_css" => self.toggle_custom_css(cx),
            "misc_help" => self.open_help(cx),
            "misc_open_context_menu" => self.open_cursor_context_menu(window, cx),
            "dnd_start" => self.start_dnd(cx),
            "dnd_move_up" => self.move_dnd(-1, cx),
            "dnd_move_down" => self.move_dnd(1, cx),
            "dnd_drop" => self.drop_dnd(cx),
            "dnd_cancel" => self.cancel_dnd(cx),
            "message_reply" => self.trigger_cursor_message_action(MessageAction::Reply, window, cx),
            "message_reply_prev" => self.step_cursor_message_action(false, -1, window, cx),
            "message_reply_next" => self.step_cursor_message_action(false, 1, window, cx),
            "message_edit" => self.trigger_cursor_message_action(MessageAction::Edit, window, cx),
            "message_edit_prev" => self.step_cursor_message_action(true, -1, window, cx),
            "message_edit_next" => self.step_cursor_message_action(true, 1, window, cx),
            "message_react" => self.trigger_cursor_message_action(MessageAction::React, window, cx),
            "message_forward" => self.trigger_cursor_message_action(MessageAction::Forward, window, cx),
            "message_pin" => self.trigger_cursor_message_action(MessageAction::Pin, window, cx),
            "message_bookmark" => self.trigger_extended_cursor_message_action(MessageAction::Bookmark, window, cx),
            "message_toggle_embeds" => {
                self.trigger_extended_cursor_message_action(MessageAction::SuppressEmbeds, window, cx)
            }
            "message_copy_link" => self.trigger_extended_cursor_message_action(MessageAction::CopyLink, window, cx),
            "message_copy_id" => self.trigger_extended_cursor_message_action(MessageAction::CopyId, window, cx),
            "message_mark_unread" => self.trigger_cursor_message_action(MessageAction::MarkUnread, window, cx),
            "message_copy_text" => self.trigger_cursor_message_action(MessageAction::CopyText, window, cx),
            "message_speak" => self.trigger_cursor_message_action(MessageAction::Speak, window, cx),
            "message_focus_textarea" => self.focus_textarea_from_message_cursor(window, cx),
            _ => return false,
        }
        true
    }

    pub(crate) fn trigger_extended_cursor_message_action(
        &mut self,
        action: MessageAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.resolve_keyboard_message_target() else {
            return;
        };
        let allowed = match action {
            MessageAction::Bookmark | MessageAction::CopyLink | MessageAction::CopyId => target.sent,
            MessageAction::SuppressEmbeds => target.sent && target.can_suppress_embeds,
            _ => false,
        };
        if !allowed {
            return;
        }
        self.keyboard_message = Some(target.message_id);
        self.message_action(action, &target, window, cx);
        self.scroller.update(cx, |_, cx| cx.notify());
    }

    pub(crate) fn open_cursor_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.resolve_keyboard_message_target() else {
            return;
        };
        self.keyboard_message = Some(target.message_id);
        self.open_message_menu(target, window, cx);
    }

    pub(crate) fn toggle_compact_call_view(&mut self, cx: &mut Context<Self>) {
        if self.voice.channel_id.is_some() {
            self.voice_stage_open = !self.voice_stage_open;
            cx.notify();
        }
    }

    pub(crate) fn toggle_voice_vad(&mut self, cx: &mut Context<Self>) {
        let next = !self.prefs.data.voice_push_to_talk;
        self.prefs.set_voice_push_to_talk(next);
        cx.notify();
    }

    pub(crate) fn toggle_shortcuts_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            &self.overlay,
            Overlay::Settings(modal) if modal.active_tab == ui::settings::SettingsTab::Keybinds
        ) {
            self.close_overlay(window, cx);
        } else {
            self.open_settings(window, cx);
            self.select_settings_tab(ui::settings::SettingsTab::Keybinds, cx);
        }
    }

    pub(crate) fn toggle_saved_media_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // TODO(backlog): dedicated Saved Media filter tab; currently toggles the Media picker.
        self.toggle_gif_picker(window, cx);
    }

    pub(crate) fn is_dnd_active(&self) -> bool {
        false
    }

    pub(crate) fn has_incoming_call(&self) -> bool {
        false
    }

    pub(crate) fn start_dnd(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): keyboard drag-and-drop mode is not implemented yet.
    }

    pub(crate) fn move_dnd(&mut self, _delta: i32, _cx: &mut Context<Self>) {
        // TODO(backlog): keyboard drag-and-drop move is not implemented yet.
    }

    pub(crate) fn drop_dnd(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): keyboard drag-and-drop drop is not implemented yet.
    }

    pub(crate) fn cancel_dnd(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): keyboard drag-and-drop cancel is not implemented yet.
    }

    pub(crate) fn answer_incoming_call(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): incoming call ring/answer state is not implemented yet.
    }

    pub(crate) fn decline_incoming_call(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): incoming call ring/decline state is not implemented yet.
    }

    pub(crate) fn toggle_soundboard(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): soundboard popout is not implemented yet.
    }

    pub(crate) fn toggle_voice_camera(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): local camera capture/publish is not implemented in voice_worker yet.
    }

    pub(crate) fn switch_voice_channel(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): voice channel switcher shortcut is not implemented yet.
    }

    pub(crate) fn trigger_voice_push_to_talk(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): global push-to-talk keybind hold is not implemented yet.
    }

    pub(crate) fn trigger_voice_push_to_talk_priority(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): global priority push-to-talk keybind hold is not implemented yet.
    }

    pub(crate) fn trigger_voice_push_to_mute(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): global push-to-mute keybind hold is not implemented yet.
    }

    pub(crate) fn trigger_voice_priority_vad(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): priority speaker VAD is not implemented yet.
    }

    pub(crate) fn start_voice_message(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): voice message recording in composer is not implemented yet.
    }

    pub(crate) fn open_help(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): help overlay/documentation shortcut is not implemented yet.
    }

    pub(crate) fn open_theme_studio_popout(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): theme studio / quick CSS popout is not implemented yet.
    }

    pub(crate) fn toggle_custom_css(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): custom CSS runtime toggle is not implemented yet.
    }

    pub(crate) fn toggle_downloads_panel(&mut self, _cx: &mut Context<Self>) {
        // TODO(backlog): downloads panel is not implemented yet.
    }

    pub(crate) fn intercept_keystroke(
        &mut self,
        keystroke: &gpui_kit::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Overlay::Settings(modal) = &self.overlay {
            if let Some(recording_id) = modal.keybinds_recording_id.clone() {
                if keystroke.key == "escape"
                    && !keystroke.modifiers.control
                    && !keystroke.modifiers.alt
                    && !keystroke.modifiers.shift
                    && !keystroke.modifiers.platform
                {
                    self.settings_cancel_record_keybind(cx);
                    return true;
                }
                if let Some(combo) = shortcuts::keystroke_to_combo(keystroke) {
                    self.settings_set_custom_keybind_combo(&recording_id, combo, cx);
                    self.settings_cancel_record_keybind(cx);
                }
                return true;
            }
        }

        if self.overlay.is_open() {
            return false;
        }
        let Some(combo) = shortcuts::keystroke_to_combo(keystroke) else {
            return false;
        };
        let Some(action) = shortcuts::matching_custom_action(&self.prefs.data, &combo).map(str::to_owned) else {
            return false;
        };
        self.dispatch_keybind_action(&action, window, cx)
    }

    pub(crate) fn handle_sidebar_resize_key(
        &mut self,
        ev: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = if ev.keystroke.modifiers.shift { 48.0 } else { 12.0 };
        let next = match ev.keystroke.key.as_str() {
            "left" => Some((self.effective_sidebar_width() - step).clamp(SIDEBAR_WIDTH_MIN as f32, SIDEBAR_WIDTH_MAX as f32)),
            "right" => Some((self.effective_sidebar_width() + step).clamp(SIDEBAR_WIDTH_MIN as f32, SIDEBAR_WIDTH_MAX as f32)),
            "home" => Some(SIDEBAR_WIDTH_MIN as f32),
            "end" => Some(SIDEBAR_WIDTH_MAX as f32),
            "enter" | "backspace" | "delete" => Some(SIDEBAR_WIDTH_DEFAULT as f32),
            _ => None,
        };
        if let Some(next) = next {
            self.sidebar_width = next;
            self.prefs.set_sidebar_width(next.round() as u16);
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(crate) fn handle_voice_resize_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let max_h = self.max_voice_call_height(window);
        let min_h = VOICE_CALL_HEIGHT_MIN as f32;
        let step = if ev.keystroke.modifiers.shift { 48.0 } else { 16.0 };
        let cur = self.effective_voice_call_height(window);
        let next = match ev.keystroke.key.as_str() {
            "up" => Some((cur - step).clamp(min_h, max_h)),
            "down" => Some((cur + step).clamp(min_h, max_h)),
            "home" => Some(min_h),
            "end" => Some(max_h),
            "enter" | "backspace" | "delete" => Some(VOICE_CALL_HEIGHT_DEFAULT as f32),
            _ => None,
        };
        if let Some(next) = next {
            self.voice_call_height = next;
            self.prefs.set_voice_call_height(next.round() as u16);
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(crate) fn handle_emoji_resize_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (max_w, max_h) = self.max_emoji_picker_bounds(window);
        let min_h = EMOJI_PICKER_HEIGHT_MIN as f32;
        let step_w = if ev.keystroke.modifiers.shift { 96.0 } else { 48.0 };
        let step_h = if ev.keystroke.modifiers.shift { 48.0 } else { 16.0 };
        let next = match ev.keystroke.key.as_str() {
            "left" => Some((snap_emoji_picker_width(self.emoji_picker_width + step_w, max_w), self.emoji_picker_height.round() as u16)),
            "right" => Some((snap_emoji_picker_width(self.emoji_picker_width - step_w, max_w), self.emoji_picker_height.round() as u16)),
            "up" => Some((snap_emoji_picker_width(self.emoji_picker_width, max_w), (self.emoji_picker_height + step_h).clamp(min_h, max_h).round() as u16)),
            "down" => Some((snap_emoji_picker_width(self.emoji_picker_width, max_w), (self.emoji_picker_height - step_h).clamp(min_h, max_h).round() as u16)),
            "home" => Some((EMOJI_PICKER_WIDTH_MIN, EMOJI_PICKER_HEIGHT_MIN)),
            "enter" | "backspace" | "delete" => Some((EMOJI_PICKER_WIDTH_DEFAULT, EMOJI_PICKER_HEIGHT_DEFAULT)),
            _ => None,
        };
        if let Some((w, h)) = next {
            self.apply_emoji_picker_size(w, h, true, cx);
            cx.stop_propagation();
            cx.notify();
        }
    }
}
