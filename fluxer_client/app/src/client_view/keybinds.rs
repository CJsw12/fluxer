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
            _ => return false,
        }
        true
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
