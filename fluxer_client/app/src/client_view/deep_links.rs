//! Deep link handling (`fluxer://`) and scene links.

use super::*;

impl ClientView {
    /// Routes a validated deep link inside the window, deferring workspace links until `READY`.
    pub(crate) fn open_deep_link(
        &mut self,
        link: crate::deep_links::DeepLink,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(link, crate::deep_links::DeepLink::Settings) {
            let _ = self.try_apply_deep_link(&link, true, window, cx);
            return;
        }
        if self.user.is_some() {
            let _ = self.try_apply_deep_link(&link, true, window, cx);
        } else {
            self.pending_deep_link = Some((link, std::time::Instant::now()));
        }
    }

    /// Tries to navigate to `link`. Returns `true` when resolved (or failed with
    /// `show_missing_error`), or `false` when still waiting for deferred guild/channel data.
    pub(crate) fn try_apply_deep_link(
        &mut self,
        link: &crate::deep_links::DeepLink,
        show_missing_error: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::deep_links::DeepLink;

        self.error = None;
        match link {
            DeepLink::Home => {
                self.close_overlay(window, cx);
                self.place = Place::Home;
                self.clear_selection(cx);
                true
            }
            DeepLink::Explore => {
                self.close_overlay(window, cx);
                self.open_explore(cx);
                true
            }
            DeepLink::DmChannel { channel_id, message_id } => {
                let valid = self
                    .backend
                    .state()
                    .channels
                    .get(channel_id)
                    .is_some_and(|ch| ch.guild_id.is_none() && opens_message_pane(ch.kind));
                if !valid {
                    if show_missing_error {
                        self.error = Some(l10n::tr("Unknown channel").into());
                        cx.notify();
                    }
                    return show_missing_error;
                }
                self.close_overlay(window, cx);
                if let Some(msg) = *message_id {
                    self.jump_to_channel_message(None, *channel_id, msg, cx);
                } else {
                    self.open_channel(None, *channel_id, cx);
                }
                true
            }
            DeepLink::Guild { guild_id } => {
                if !self.backend.state().guilds.contains_key(guild_id) {
                    if show_missing_error {
                        self.error = Some(l10n::tr("Not found").into());
                        cx.notify();
                    }
                    return show_missing_error;
                }
                self.close_overlay(window, cx);
                self.select_place(Place::Guild(*guild_id), cx);
                true
            }
            DeepLink::GuildChannel { guild_id, channel_id, message_id } => {
                let valid = self
                    .backend
                    .state()
                    .channels
                    .get(channel_id)
                    .is_some_and(|ch| ch.guild_id == Some(*guild_id) && opens_message_pane(ch.kind));
                if !valid {
                    if show_missing_error {
                        self.error = Some(l10n::tr("Unknown channel").into());
                        cx.notify();
                    }
                    return show_missing_error;
                }
                self.close_overlay(window, cx);
                if let Some(msg) = *message_id {
                    self.jump_to_channel_message(Some(*guild_id), *channel_id, msg, cx);
                } else {
                    self.open_channel(Some(*guild_id), *channel_id, cx);
                }
                true
            }
            DeepLink::Invite { code } => {
                self.open_join_guild_with_code(code, window, cx);
                true
            }
            DeepLink::UserProfile { user_id } => {
                self.open_profile(
                    *user_id,
                    self.current_guild_id(),
                    gpui_kit::point(px(240.), px(120.)),
                    window,
                    cx,
                );
                true
            }
            DeepLink::Settings => {
                if !matches!(self.overlay, Overlay::Settings(_)) {
                    self.open_settings(window, cx);
                }
                true
            }
            DeepLink::Scene(route) => self.apply_scene_deep_link(route, window, cx),
        }
    }

    pub(crate) fn apply_scene_deep_link(&mut self, route: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let parts: Vec<&str> = route.split('/').filter(|s| !s.is_empty()).collect();
        self.user_area_popout_open = false;
        self.guild_header_popout_open = false;
        self.voice_popout = None;
        if parts.first().copied() != Some("search") && self.search.is_some() {
            self.close_search(window, cx);
        }
        match parts.as_slice() {
            ["settings", tab] => {
                if !matches!(self.overlay, Overlay::Settings(_)) {
                    self.open_settings(window, cx);
                }
                let target_tab = match *tab {
                    "profile" => ui::settings::SettingsTab::Profile,
                    "account" => ui::settings::SettingsTab::Account,
                    "privacy" => ui::settings::SettingsTab::PrivacySafety,
                    "connections" => ui::settings::SettingsTab::Connections,
                    "appearance" => ui::settings::SettingsTab::Appearance,
                    "themes" => ui::settings::SettingsTab::Themes,
                    "notifications" => ui::settings::SettingsTab::Notifications,
                    "chat" => ui::settings::SettingsTab::MessagesMedia,
                    "voice" => ui::settings::SettingsTab::VoiceVideo,
                    "accessibility" => ui::settings::SettingsTab::Accessibility,
                    "language" => ui::settings::SettingsTab::Language,
                    "keybinds" => ui::settings::SettingsTab::Keybinds,
                    "windows" | "desktop" => ui::settings::SettingsTab::WindowsApp,
                    "advanced" => ui::settings::SettingsTab::Advanced,
                    "applications" => ui::settings::SettingsTab::Applications,
                    _ => ui::settings::SettingsTab::Appearance,
                };
                self.select_settings_tab(target_tab, cx);
                true
            }
            ["voice", gid, cid] => {
                let (Some(g), Some(c)) = (Snowflake::parse(gid), Snowflake::parse(cid)) else {
                    return true;
                };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                self.select_place(Place::Guild(g), cx);
                self.voice_stage_expanded = false;
                self.voice_call_height = 328.0;
                self.join_voice_channel(c, cx);
                true
            }
            ["voice-split", gid, cid] => {
                let (Some(g), Some(c)) = (Snowflake::parse(gid), Snowflake::parse(cid)) else {
                    return true;
                };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                if self.place != Place::Guild(g) {
                    self.select_place(Place::Guild(g), cx);
                }
                self.join_voice_channel(c, cx);
                self.voice_stage_open = true;
                self.voice_stage_expanded = false;
                cx.notify();
                true
            }
            ["voice-popout", kind] => {
                self.close_overlay(window, cx);
                self.voice_popout = match *kind {
                    "input" => Some(VoicePopoutKind::Input),
                    "output" => Some(VoicePopoutKind::Output),
                    "stream" => Some(VoicePopoutKind::Stream),
                    "details" => Some(VoicePopoutKind::Details),
                    _ => None,
                };
                cx.notify();
                true
            }
            ["voice-leave"] => {
                self.close_overlay(window, cx);
                self.voice_call_height = self.prefs.data.voice_call_height as f32;
                self.leave_voice_channel(cx);
                true
            }
            ["voice-mute", mode] => {
                let want = *mode == "on";
                if self.voice.self_mute != want {
                    self.toggle_voice_mute(cx);
                }
                true
            }
            ["voice-deafen", mode] => {
                let want = *mode == "on";
                if self.voice.self_deaf != want {
                    self.toggle_voice_deaf(cx);
                }
                true
            }
            ["user-area-popout"] => {
                self.close_overlay(window, cx);
                self.user_area_popout_open = true;
                cx.notify();
                true
            }
            ["profile-self"] => {
                self.close_overlay(window, cx);
                if let Some(uid) = self.user.as_ref().map(|u| u.id) {
                    self.open_profile(uid, self.current_guild_id(), gpui_kit::point(px(340.), px(180.)), window, cx);
                }
                true
            }
            ["guild-header-popout", gid] => {
                let Some(g) = Snowflake::parse(gid) else { return true };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                if self.place != Place::Guild(g) {
                    self.select_place(Place::Guild(g), cx);
                }
                self.guild_header_popout_open = true;
                cx.notify();
                true
            }
            ["channel-settings", gid, cid, tab] => {
                let (Some(g), Some(c)) = (Snowflake::parse(gid), Snowflake::parse(cid)) else {
                    return true;
                };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                if self.place != Place::Guild(g) {
                    self.select_place(Place::Guild(g), cx);
                }
                if *tab == "permissions" {
                    self.open_access(ui::access::Scope::Channel { guild: g, channel: c }, window, cx);
                } else {
                    self.open_edit_channel(c, window, cx);
                }
                true
            }
            ["guild-settings", gid, tab] => {
                let Some(g) = Snowflake::parse(gid) else { return true };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                if self.place != Place::Guild(g) {
                    self.select_place(Place::Guild(g), cx);
                }
                if *tab == "bans" {
                    self.open_guild_bans(window, cx);
                } else {
                    self.open_access(ui::access::Scope::Guild(g), window, cx);
                }
                true
            }
            ["quick-switcher"] => {
                self.open_quick_switcher(window, cx);
                true
            }
            ["emoji-picker"] => {
                self.close_overlay(window, cx);
                self.open_emoji_picker(
                    ui::emoji_picker::sections::PickerTarget::Composer,
                    gpui_kit::point(px(1010.), px(730.)),
                    window,
                    cx,
                );
                true
            }
            ["search"] => {
                self.close_overlay(window, cx);
                if self.search.is_none() {
                    self.toggle_search(window, cx);
                }
                true
            }
            ["theme", kind] => {
                let Some(kind) = ThemeKind::parse(kind) else {
                    return true;
                };
                self.close_overlay(window, cx);
                theme::set_kind(kind, cx);
                theme::apply(kind, window, cx);
                cx.notify();
                true
            }
            ["close-overlays"] => {
                self.close_overlay(window, cx);
                if self.search.is_some() {
                    self.close_search(window, cx);
                }
                cx.notify();
                true
            }
            _ => true,
        }
    }
}
