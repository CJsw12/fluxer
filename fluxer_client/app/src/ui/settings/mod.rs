//! 2-pane user settings modal (`SettingsModalLayout.tsx` + `DesktopSettingsView.tsx`
//! + `SettingsConstants.tsx` + `SettingsSectionRegistry.ts`):
//! left navigation sidebar (search, clickable 40px profile card -> `Profile` tab,
//! categorized tabs with expandable sub-section tree `SidebarSections`, and sign-out footer)
//! and right scrollable content pane with official section cards.

use fluxer_client_core::rest::{
    CONNECTION_VISIBILITY_EVERYONE, DeveloperApplication, UserConnection, UserSettingsPatch, WebAuthnCredential,
};
use fluxer_client_core::state::Status;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, Hsla, ScrollHandle, SharedString, Subscription, Window, div,
    prelude::*, px,
};

use crate::backend::voice::VoiceAudioSettings;
use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::prefs::{CustomKeybindEntry, DEFAULT_VOICE_VOLUME, MAX_VOICE_VOLUME, MIN_VOICE_VOLUME, ReducedMotionPref};
use crate::settings_model::{FONT_SIZE_OPTIONS, LOCALES, SettingsEffect, THEME_OPTIONS, theme_patch_value};
use crate::shortcuts::{self, DefaultShortcutRowSpec, KeybindSection};
use crate::theme::{self, MAX_FONT_SIZE, MAX_UI_ZOOM, MIN_FONT_SIZE, MIN_UI_ZOOM, Theme, ThemeKind};
use crate::ui::avatar::{AvatarSpec, render_avatar};
use crate::ui::member_list::{status_color, status_label};
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};
use crate::updater::{CURRENT_VERSION, UpdateStatus};

mod accessibility;
mod account;
mod advanced;
mod appearance;
mod applications;
mod chat;
mod common;
mod connections;
mod keybinds;
mod language;
mod notifications;
mod privacy;
mod profile;
mod sidebar;
mod themes;
mod voice_video;
mod windows_app;

use self::accessibility::*;
use self::account::*;
use self::advanced::*;
use self::appearance::*;
use self::applications::*;
use self::chat::*;
use self::common::*;
use self::connections::*;
use self::keybinds::*;
use self::language::*;
use self::notifications::*;
use self::privacy::*;
use self::profile::*;
use self::themes::*;
use self::voice_video::*;
use self::windows_app::*;
pub use self::sidebar::render;

pub const CONTEXT: &str = "SettingsModal";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    Profile,
    Account,
    PrivacySafety,
    Connections,
    #[default]
    Appearance,
    Themes,
    Notifications,
    MessagesMedia,
    VoiceVideo,
    Accessibility,
    Language,
    Keybinds,
    WindowsApp,
    Advanced,
    Applications,
}

impl SettingsTab {
    const ACCOUNT_TABS: &[SettingsTab] = &[
        SettingsTab::Account,
        SettingsTab::PrivacySafety,
        SettingsTab::Connections,
    ];
    const APP_TABS: &[SettingsTab] = &[
        SettingsTab::Appearance,
        SettingsTab::Themes,
        SettingsTab::Notifications,
        SettingsTab::MessagesMedia,
        SettingsTab::VoiceVideo,
        SettingsTab::Accessibility,
        SettingsTab::Language,
        SettingsTab::Keybinds,
        SettingsTab::WindowsApp,
        SettingsTab::Advanced,
    ];
    const DEVELOPER_TABS: &[SettingsTab] = &[SettingsTab::Applications];
    const ALL: &[SettingsTab] = &[
        SettingsTab::Profile,
        SettingsTab::Account,
        SettingsTab::PrivacySafety,
        SettingsTab::Connections,
        SettingsTab::Appearance,
        SettingsTab::Themes,
        SettingsTab::Notifications,
        SettingsTab::MessagesMedia,
        SettingsTab::VoiceVideo,
        SettingsTab::Accessibility,
        SettingsTab::Language,
        SettingsTab::Keybinds,
        SettingsTab::WindowsApp,
        SettingsTab::Advanced,
        SettingsTab::Applications,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Profile => l10n::tr("Profile"),
            Self::Account => l10n::tr("Account"),
            Self::PrivacySafety => l10n::tr("Privacy"),
            Self::Connections => l10n::tr("Connections"),
            Self::Appearance => l10n::tr("Appearance"),
            Self::Themes => l10n::tr("Themes"),
            Self::Notifications => l10n::tr("Notifications"),
            Self::MessagesMedia => l10n::tr("Chat"),
            Self::VoiceVideo => l10n::tr("Voice & video"),
            Self::Accessibility => l10n::tr("Accessibility"),
            Self::Language => l10n::tr("Language"),
            Self::Keybinds => l10n::tr("Keybinds"),
            Self::WindowsApp => l10n::tr("Windows app"),
            Self::Advanced => l10n::tr("Advanced"),
            Self::Applications => l10n::tr("Applications"),
        }
    }

    pub const fn debug_selector(self) -> &'static str {
        match self {
            Self::Profile => "settings-tab-profile",
            Self::Account => "settings-tab-account",
            Self::PrivacySafety => "settings-tab-privacy",
            Self::Connections => "settings-tab-connections",
            Self::Appearance => "settings-tab-appearance",
            Self::Themes => "settings-tab-themes",
            Self::Notifications => "settings-tab-notifications",
            Self::MessagesMedia => "settings-tab-chat",
            Self::VoiceVideo => "settings-tab-voice",
            Self::Accessibility => "settings-tab-accessibility",
            Self::Language => "settings-tab-language",
            Self::Keybinds => "settings-tab-keybinds",
            Self::WindowsApp => "settings-tab-windows",
            Self::Advanced => "settings-tab-advanced",
            Self::Applications => "settings-tab-applications",
        }
    }

    const fn icon(self) -> AppIcon {
        match self {
            Self::Profile => AppIcon::UserCircle,
            Self::Account => AppIcon::Shield,
            Self::PrivacySafety => AppIcon::EyeSlash,
            Self::Connections => AppIcon::Users,
            Self::Appearance => AppIcon::PaintBrush,
            Self::Themes => AppIcon::Swatches,
            Self::Notifications => AppIcon::Bell,
            Self::MessagesMedia => AppIcon::ChatCircleFill,
            Self::VoiceVideo => AppIcon::Microphone,
            Self::Accessibility => AppIcon::PersonSimpleCircle,
            Self::Language => AppIcon::Translate,
            Self::Keybinds => AppIcon::Keyboard,
            Self::WindowsApp => AppIcon::Monitor,
            Self::Advanced => AppIcon::Gear,
            Self::Applications => AppIcon::Code,
        }
    }

    /// Official deep link URI (`UserSettingsDeepLinks.ts`) for the header copy button.
    pub const fn deep_link_url(self) -> &'static str {
        match self {
            Self::Profile => "fluxer://settings/user?tab=my_profile",
            Self::Account => "fluxer://settings/user?tab=account_security",
            Self::PrivacySafety => "fluxer://settings/user?tab=privacy_safety",
            Self::Connections => "fluxer://settings/user?tab=linked_accounts",
            Self::Appearance => "fluxer://settings/user?tab=appearance",
            Self::Themes => "fluxer://settings/user?tab=themes",
            Self::Notifications => "fluxer://settings/user?tab=notifications",
            Self::MessagesMedia => "fluxer://settings/user?tab=chat_settings",
            Self::VoiceVideo => "fluxer://settings/user?tab=voice_video",
            Self::Accessibility => "fluxer://settings/user?tab=accessibility",
            Self::Language => "fluxer://settings/user?tab=language",
            Self::Keybinds => "fluxer://settings/user?tab=keybinds",
            Self::WindowsApp => "fluxer://settings/user?tab=desktop_settings",
            Self::Advanced => "fluxer://settings/user?tab=advanced_settings",
            Self::Applications => "fluxer://settings/user?tab=applications",
        }
    }

    /// Official sub-sections (`SettingsSectionRegistry.ts` & `AdvancedSettingsCategories.ts`) shown in the left sidebar tree.
    pub const fn sections(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Profile => &[
                ("profile-customization", "Profile customization"),
                ("profile-badges", "Profile & status"),
            ],
            Self::Account => &[
                ("account", "Sign-in details"),
                ("security", "Security"),
                ("danger_zone", "Danger zone"),
                ("blocked_users", "Blocked users"),
            ],
            Self::PrivacySafety => &[
                ("profile-privacy", "Profile privacy"),
                ("connections", "Friends & direct messages"),
                ("communication", "Communication"),
                ("active-now", "Activity sharing"),
                ("sensitive-content", "Sensitive content"),
                ("data-export", "Data export"),
                ("data-deletion", "Data deletion"),
            ],
            Self::Connections => &[],
            Self::Appearance => &[
                ("theme", "Theme"),
                ("hdr", "High Dynamic Range"),
                ("app-zoom-level", "App zoom level"),
                ("messages", "Messages"),
                ("chat-font-scaling", "Chat font scaling"),
                ("interface", "Interface"),
                ("channel-list", "Channel list"),
                ("active-now", "Active now"),
                ("streamer-mode", "Streamer mode"),
            ],
            Self::Themes => &[
                ("this-account", "This account"),
                ("base-theme", "Base theme"),
                ("your-themes", "Your themes"),
                ("colors-and-fonts", "Colors and fonts"),
                ("assets", "Assets"),
                ("transparent-window", "Desktop window"),
                ("share-and-back-up", "Share and back up"),
                ("quick-css", "Quick CSS"),
            ],
            Self::Notifications => &[
                ("notifications", "General"),
                ("mention-preference", "Mention preference"),
                ("sounds", "Sounds"),
                ("text-to-speech", "Text-to-speech notifications"),
            ],
            Self::MessagesMedia => &[("display", "Display"), ("input", "Input")],
            Self::VoiceVideo => &[("audio", "Audio"), ("video", "Video")],
            Self::Accessibility => &[
                ("visual", "Visual"),
                ("screen-reader", "Screen reader"),
                ("tts", "Text-to-speech"),
                ("keyboard", "Keyboard"),
                ("animation", "Animation"),
                ("motion", "Motion"),
            ],
            Self::Language => &[
                ("language-settings", "Interface language"),
                ("regional-format", "Regional format"),
                ("time-format", "Time format"),
                ("date-format", "Date format"),
                ("spellcheck", "Spellcheck"),
            ],
            Self::Keybinds => &[],
            Self::WindowsApp => &[],
            Self::Advanced => &[
                ("advanced-settings-privacy", "Privacy"),
                ("advanced-settings-appearance", "Appearance"),
                ("advanced-settings-accessibility", "Accessibility"),
                ("advanced-settings-chat", "Chat"),
                ("advanced-settings-media", "Media"),
                ("advanced-settings-voice", "Voice"),
                ("advanced-settings-notifications", "Notifications"),
                ("advanced-settings-desktop", "Desktop"),
                ("advanced-settings-developer", "Developer"),
            ],
            Self::Applications => &[],
        }
    }

    fn matches_query(self, query_lower: &str) -> bool {
        if query_lower.is_empty() {
            return true;
        }
        if self.label().to_lowercase().contains(query_lower) {
            return true;
        }
        let keywords: &[&str] = match self {
            Self::Profile => &["profile", "avatar", "bio", "pronouns", "status", "nickname"],
            Self::Account => &[
                "account",
                "switch",
                "trusted",
                "domain",
                "sign out",
                "log out",
                "passkey",
                "security key",
                "webauthn",
                "fido",
                "2fa",
                "mfa",
            ],
            Self::PrivacySafety => &[
                "privacy",
                "safety",
                "profile privacy",
                "direct message",
                "friend request",
                "communication",
                "incoming call",
                "group chat",
                "activity sharing",
                "sensitive",
                "mature",
                "filter",
                "data export",
                "data deletion",
            ],
            Self::Connections => &[
                "connections",
                "linked accounts",
                "bluesky",
                "domain",
                "oauth",
                "external",
                "verify",
            ],
            Self::Appearance => &[
                "appearance",
                "look and feel",
                "theme",
                "dark",
                "light",
                "coal",
                "system",
                "display",
                "font",
                "zoom",
                "compact",
                "cozy",
                "preview",
            ],
            Self::Themes => &["themes", "base theme", "color scheme", "palette", "window chrome"],
            Self::Notifications => &["notification", "sound", "desktop", "message sounds", "badge"],
            Self::MessagesMedia => &[
                "messages",
                "media",
                "embed",
                "inline",
                "link preview",
                "spoiler",
                "emoji",
                "reaction",
                "autocomplete",
            ],
            Self::VoiceVideo => &[
                "voice",
                "video",
                "audio",
                "mic",
                "input",
                "output",
                "volume",
                "device",
                "noise suppression",
                "echo cancellation",
                "push to talk",
                "screen share",
            ],
            Self::Accessibility => &["accessibility", "reduced", "motion", "animation", "contrast", "screen reader"],
            Self::Language => &["language", "locale", "translation", "time format", "date format"],
            Self::Keybinds => &["keybind", "shortcut", "hotkey", "keyboard shortcut"],
            Self::WindowsApp => &[
                "windows",
                "windows app",
                "desktop",
                "startup",
                "autostart",
                "login",
                "tray",
                "minimize",
                "close to tray",
            ],
            Self::Advanced => &["advanced", "update", "version", "about"],
            Self::Applications => &[
                "applications",
                "developer",
                "bot",
                "oauth",
                "api",
                "docs",
                "documentation",
                "client",
            ],
        };
        keywords.iter().any(|kw| kw.contains(query_lower))
    }
}

pub struct SettingsModal {
    pub active_tab: SettingsTab,
    pub active_section: Option<&'static str>,
    pub confirm_logout: bool,
    pub search: Entity<InputState>,
    pub passkey_name: Entity<InputState>,
    pub passkey_password: Entity<InputState>,
    pub passkeys: Vec<WebAuthnCredential>,
    pub passkey_loading: bool,
    pub passkey_busy: bool,
    pub passkey_status: Option<SharedString>,
    pub passkey_error: Option<SharedString>,
    pub connections: Vec<UserConnection>,
    pub connections_loaded: bool,
    pub connections_loading: bool,
    pub connections_error: Option<SharedString>,
    pub connections_notice: Option<SharedString>,
    pub applications: Vec<DeveloperApplication>,
    pub applications_loaded: bool,
    pub applications_loading: bool,
    pub applications_error: Option<SharedString>,
    pub selected_application_id: Option<String>,
    pub selected_application_loading: bool,
    pub applications_notice: Option<SharedString>,
    pub privacy_notice: Option<SharedString>,
    pub sidebar_scroll: ScrollHandle,
    pub locale_scroll: ScrollHandle,
    pub _subscription: Subscription,
    pub keybinds_search: Entity<InputState>,
    pub keybinds_recording_id: Option<String>,
    pub keybinds_open_action_menu: Option<String>,
    pub keybinds_open_default_menu: Option<String>,
    pub keybinds_confirm_reset: bool,
    pub _keybinds_search_sub: Subscription,
}

impl ClientView {
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        if matches!(self.overlay, Overlay::Settings(_)) {
            self.close_overlay(window, cx);
            return;
        }
        self.prepare_overlay_replace(cx);
        self.refresh_audio_devices(cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search settings...")));
        let passkey_name = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Passkey or security key name")));
        let passkey_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(l10n::tr("Current password (sudo verification)"))
                .masked(true)
        });
        let subscription = cx.subscribe_in(&search, window, |_, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                cx.notify();
            }
        });
        let keybinds_search = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search keybinds…")));
        let keybinds_search_sub = cx.subscribe_in(&keybinds_search, window, |_, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                cx.notify();
            }
        });
        let default_tab = SettingsTab::default();
        let default_section = default_tab.sections().first().map(|(id, _)| *id);
        self.overlay = Overlay::Settings(SettingsModal {
            active_tab: default_tab,
            active_section: default_section,
            confirm_logout: false,
            search,
            passkey_name,
            passkey_password,
            passkeys: Vec::new(),
            passkey_loading: false,
            passkey_busy: false,
            passkey_status: None,
            passkey_error: None,
            connections: Vec::new(),
            connections_loaded: false,
            connections_loading: false,
            connections_error: None,
            connections_notice: None,
            applications: Vec::new(),
            applications_loaded: false,
            applications_loading: false,
            applications_error: None,
            selected_application_id: None,
            selected_application_loading: false,
            applications_notice: None,
            privacy_notice: None,
            sidebar_scroll: ScrollHandle::new(),
            locale_scroll: ScrollHandle::new(),
            _subscription: subscription,
            keybinds_search,
            keybinds_recording_id: None,
            keybinds_open_action_menu: None,
            keybinds_open_default_menu: None,
            keybinds_confirm_reset: false,
            _keybinds_search_sub: keybinds_search_sub,
        });
        cx.notify();
    }

    fn trigger_tab_loads(&mut self, tab: SettingsTab) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            match tab {
                SettingsTab::Account if modal.passkeys.is_empty() && !modal.passkey_loading => {
                    modal.passkey_loading = true;
                    self.backend.load_webauthn_credentials();
                }
                SettingsTab::Connections if !modal.connections_loaded && !modal.connections_loading => {
                    modal.connections_loading = true;
                    modal.connections_error = None;
                    self.backend.load_connections();
                }
                SettingsTab::Applications if !modal.applications_loaded && !modal.applications_loading => {
                    modal.applications_loading = true;
                    modal.applications_error = None;
                    self.backend.load_applications();
                }
                _ => {}
            }
        }
    }

    pub(crate) fn select_settings_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.active_tab = tab;
            modal.active_section = tab.sections().first().map(|(id, _)| *id);
            if tab != SettingsTab::Applications {
                modal.selected_application_id = None;
                modal.selected_application_loading = false;
            }
            if tab != SettingsTab::Keybinds {
                modal.keybinds_recording_id = None;
                modal.keybinds_open_action_menu = None;
                modal.keybinds_open_default_menu = None;
                modal.keybinds_confirm_reset = false;
            }
        }
        self.trigger_tab_loads(tab);
        cx.notify();
    }

    pub(crate) fn select_settings_section(
        &mut self,
        tab: SettingsTab,
        section_id: &'static str,
        cx: &mut Context<Self>,
    ) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.active_tab = tab;
            modal.active_section = Some(section_id);
            if tab != SettingsTab::Keybinds {
                modal.keybinds_recording_id = None;
                modal.keybinds_open_action_menu = None;
                modal.keybinds_open_default_menu = None;
                modal.keybinds_confirm_reset = false;
            }
        }
        self.trigger_tab_loads(tab);
        cx.notify();
    }

    pub(crate) fn settings_add_custom_keybind(
        &mut self,
        action: Option<String>,
        cx: &mut Context<Self>,
    ) -> String {
        let id = self.prefs.add_custom_keybind(action, String::new());
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.keybinds_open_default_menu = None;
            modal.keybinds_open_action_menu = None;
        }
        cx.notify();
        id
    }

    pub(crate) fn settings_set_custom_keybind_action(
        &mut self,
        id: &str,
        action: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.prefs.set_custom_keybind_action(id, action);
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.keybinds_open_action_menu = None;
        }
        cx.notify();
    }

    pub(crate) fn settings_set_custom_keybind_combo(
        &mut self,
        id: &str,
        combo: String,
        cx: &mut Context<Self>,
    ) {
        let normalized = shortcuts::normalize_combo(&combo);
        self.prefs.set_custom_keybind_combo(id, normalized);
        cx.notify();
    }

    pub(crate) fn settings_set_custom_keybind_enabled(
        &mut self,
        id: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.prefs.set_custom_keybind_enabled(id, enabled);
        cx.notify();
    }

    pub(crate) fn settings_remove_custom_keybind(&mut self, id: &str, cx: &mut Context<Self>) {
        self.prefs.remove_custom_keybind(id);
        if let Overlay::Settings(modal) = &mut self.overlay {
            if modal.keybinds_recording_id.as_deref() == Some(id) {
                modal.keybinds_recording_id = None;
            }
            if modal.keybinds_open_action_menu.as_deref() == Some(id) {
                modal.keybinds_open_action_menu = None;
            }
        }
        cx.notify();
    }

    pub(crate) fn settings_remove_custom_keybinds_for_action(&mut self, action: &str, cx: &mut Context<Self>) {
        self.prefs.remove_custom_keybinds_for_action(action);
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.keybinds_open_default_menu = None;
        }
        cx.notify();
    }

    pub(crate) fn settings_set_disable_builtin_keybinds(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.prefs.set_disable_builtin_keybinds(disabled);
        cx.notify();
    }

    pub(crate) fn settings_disable_builtin_action(&mut self, action: &str, cx: &mut Context<Self>) {
        self.prefs.disable_builtin_action(action);
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.keybinds_open_default_menu = None;
        }
        cx.notify();
    }

    pub(crate) fn settings_enable_builtin_action(&mut self, action: &str, cx: &mut Context<Self>) {
        self.prefs.enable_builtin_action(action);
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.keybinds_open_default_menu = None;
        }
        cx.notify();
    }

    pub(crate) fn settings_toggle_record_keybind(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut started = false;
        if let Overlay::Settings(modal) = &mut self.overlay {
            if modal.keybinds_recording_id.as_deref() == Some(id.as_str()) {
                modal.keybinds_recording_id = None;
            } else {
                modal.keybinds_recording_id = Some(id);
                modal.keybinds_open_action_menu = None;
                modal.keybinds_open_default_menu = None;
                started = true;
            }
            cx.notify();
        }
        if started {
            self.focus_root(window, cx);
        }
    }

    pub(crate) fn settings_cancel_record_keybind(&mut self, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            if modal.keybinds_recording_id.take().is_some() {
                cx.notify();
            }
        }
    }

    pub(crate) fn settings_toggle_keybind_action_menu(&mut self, id: String, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            if modal.keybinds_open_action_menu.as_deref() == Some(id.as_str()) {
                modal.keybinds_open_action_menu = None;
            } else {
                modal.keybinds_open_action_menu = Some(id);
                modal.keybinds_open_default_menu = None;
                modal.keybinds_recording_id = None;
            }
            cx.notify();
        }
    }

    pub(crate) fn settings_toggle_keybind_default_menu(&mut self, key: String, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            if modal.keybinds_open_default_menu.as_deref() == Some(key.as_str()) {
                modal.keybinds_open_default_menu = None;
            } else {
                modal.keybinds_open_default_menu = Some(key);
                modal.keybinds_open_action_menu = None;
                modal.keybinds_recording_id = None;
            }
            cx.notify();
        }
    }

    pub(crate) fn settings_set_keybinds_confirm_reset(&mut self, confirm: bool, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.keybinds_confirm_reset = confirm;
            if confirm {
                modal.keybinds_recording_id = None;
                modal.keybinds_open_action_menu = None;
                modal.keybinds_open_default_menu = None;
            }
            cx.notify();
        }
    }

    pub(crate) fn settings_reset_all_keybinds(&mut self, cx: &mut Context<Self>) {
        self.prefs.reset_all_keybinds();
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.keybinds_confirm_reset = false;
            modal.keybinds_recording_id = None;
            modal.keybinds_open_action_menu = None;
            modal.keybinds_open_default_menu = None;
        }
        cx.notify();
    }

    pub(crate) fn settings_reload_connections(&mut self, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.connections_loading = true;
            modal.connections_error = None;
            self.backend.load_connections();
            cx.notify();
        }
    }

    pub(crate) fn settings_toggle_connection_visibility(
        &mut self,
        kind: String,
        connection_id: String,
        currently_visible: bool,
        cx: &mut Context<Self>,
    ) {
        let next_flags = if currently_visible { 0 } else { CONNECTION_VISIBILITY_EVERYONE };
        self.backend
            .update_connection_visibility(kind, connection_id, next_flags);
        cx.notify();
    }

    pub(crate) fn settings_set_connections_notice(&mut self, message: String, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.connections_notice = Some(message.into());
            cx.notify();
        }
    }

    pub(crate) fn settings_reload_applications(&mut self, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.applications_loading = true;
            modal.applications_error = None;
            self.backend.load_applications();
            cx.notify();
        }
    }

    pub(crate) fn settings_select_application(&mut self, app_id: String, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.selected_application_id = Some(app_id.clone());
            modal.selected_application_loading = true;
            modal.applications_notice = None;
            self.backend.load_application_detail(app_id);
            cx.notify();
        }
    }

    pub(crate) fn settings_back_to_applications(&mut self, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.selected_application_id = None;
            modal.selected_application_loading = false;
            modal.applications_notice = None;
            cx.notify();
        }
    }

    pub(crate) fn settings_set_applications_notice(&mut self, message: String, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.applications_notice = Some(message.into());
            cx.notify();
        }
    }

    pub(crate) fn settings_set_privacy_notice(&mut self, message: String, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.privacy_notice = Some(message.into());
            cx.notify();
        }
    }

    pub(crate) fn settings_update_privacy_patch(&mut self, patch: UserSettingsPatch, cx: &mut Context<Self>) {
        self.backend.update_user_settings(patch);
        cx.notify();
    }

    pub(crate) fn settings_register_passkey(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (name, sudo_password) = {
            let Overlay::Settings(modal) = &self.overlay else { return };
            if modal.passkey_busy {
                return;
            }
            let raw = modal.passkey_name.read(cx).value().trim().to_owned();
            let name = if raw.is_empty() {
                "Windows Passkey".to_owned()
            } else {
                raw
            };
            let pw = modal.passkey_password.read(cx).value().to_string();
            let sudo_password = (!pw.is_empty()).then(|| fluxer_client_core::rest::Secret::new(pw));
            (name, sudo_password)
        };
        if !crate::webauthn::is_supported() {
            if let Overlay::Settings(modal) = &mut self.overlay {
                modal.passkey_error =
                    Some(l10n::tr("Passkeys and hardware security keys are not supported on this device.").into());
                cx.notify();
            }
            return;
        }
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.passkey_busy = true;
            modal.passkey_error = None;
            modal.passkey_status = None;
            modal.passkey_name.update(cx, |s, cx| s.set_value("", window, cx));
            modal.passkey_password.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.backend.register_webauthn_passkey(name, sudo_password);
        cx.notify();
    }

    pub(crate) fn settings_delete_passkey(
        &mut self,
        credential_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sudo_password = {
            let Overlay::Settings(modal) = &mut self.overlay else { return };
            if modal.passkey_busy {
                return;
            }
            let pw = modal.passkey_password.read(cx).value().to_string();
            modal.passkey_busy = true;
            modal.passkey_error = None;
            modal.passkey_status = None;
            modal.passkey_password.update(cx, |s, cx| s.set_value("", window, cx));
            (!pw.is_empty()).then(|| fluxer_client_core::rest::Secret::new(pw))
        };
        self.backend.delete_webauthn_credential(credential_id, sudo_password);
        cx.notify();
    }

    pub(crate) fn apply_settings_effect(&mut self, effect: SettingsEffect, window: &mut Window, cx: &mut Context<Self>) {
        match effect {
            SettingsEffect::ThemeChanged(kind) => self.settings_set_theme(kind, window, cx),
            SettingsEffect::LocaleChanged(code) => self.settings_set_locale(&code, cx),
            SettingsEffect::NotificationsChanged(enabled) => self.settings_set_notifications(enabled, cx),
            SettingsEffect::SoundsChanged(enabled) => self.settings_set_sounds(enabled, cx),
            SettingsEffect::AutostartChanged(enabled) => self.settings_set_autostart(enabled, cx),
            SettingsEffect::CloseToTrayChanged(enabled) => self.settings_set_close_to_tray(enabled, cx),
            SettingsEffect::FontSizeChanged(size) => self.settings_set_font_size(size, cx),
            SettingsEffect::ZoomChanged(zoom) => self.settings_set_zoom(zoom, cx),
            SettingsEffect::CompactChanged(compact) => self.settings_set_compact(compact, cx),
            SettingsEffect::VoiceInputDeviceChanged(device) => self.settings_set_voice_input_device(device, cx),
            SettingsEffect::VoiceOutputDeviceChanged(device) => self.settings_set_voice_output_device(device, cx),
            SettingsEffect::VoiceInputVolumeChanged(volume) => self.settings_set_voice_input_volume(volume, cx),
            SettingsEffect::VoiceOutputVolumeChanged(volume) => self.settings_set_voice_output_volume(volume, cx),
            SettingsEffect::VoicePushToTalkChanged(ptt) => {
                self.prefs.set_voice_push_to_talk(ptt);
                cx.notify();
            }
            SettingsEffect::VoiceNoiseSuppressionChanged(enabled) => {
                self.prefs.set_voice_noise_suppression(enabled);
                cx.notify();
            }
            SettingsEffect::VoiceProcessingModeChanged(mode) => {
                self.prefs.set_voice_processing_mode(&mode);
                cx.notify();
            }
            SettingsEffect::VoiceEchoCancellationChanged(enabled) => {
                self.prefs.set_voice_echo_cancellation(enabled);
                cx.notify();
            }
            SettingsEffect::VoiceNoiseSuppressionBackendChanged(backend) => {
                self.prefs.set_voice_noise_suppression_backend(&backend);
                cx.notify();
            }
            SettingsEffect::VoiceAutoGainControlChanged(enabled) => {
                self.prefs.set_voice_auto_gain_control(enabled);
                cx.notify();
            }
            SettingsEffect::VoiceVadAutoThresholdChanged(enabled) => {
                self.prefs.set_voice_vad_auto_threshold(enabled);
                cx.notify();
            }
            SettingsEffect::VoiceVadThresholdChanged(threshold) => {
                self.prefs.set_voice_vad_threshold(threshold);
                cx.notify();
            }
            SettingsEffect::VoicePttReleaseDelayChanged(delay_ms) => {
                self.prefs.set_voice_ptt_release_delay_ms(delay_ms);
                cx.notify();
            }
            SettingsEffect::ToggleMicTest => {
                self.mic_test_active = !self.mic_test_active;
                cx.notify();
            }
            SettingsEffect::CameraResolutionChanged(res) => {
                self.prefs.set_camera_resolution(&res);
                cx.notify();
            }
            SettingsEffect::MirrorCameraChanged(mirror) => {
                self.prefs.set_mirror_camera(mirror);
                cx.notify();
            }
            SettingsEffect::StreamingModeChanged(mode) => {
                self.prefs.set_streaming_mode(&mode);
                cx.notify();
            }
            SettingsEffect::VideoResolutionChanged(res) => {
                self.prefs.set_video_resolution(&res);
                cx.notify();
            }
            SettingsEffect::VideoFpsChanged(fps) => {
                self.prefs.set_video_fps(fps);
                cx.notify();
            }
            SettingsEffect::VideoCodecChanged(codec) => {
                self.prefs.set_video_codec(&codec);
                cx.notify();
            }
            SettingsEffect::VideoBitrateChanged(kbps) => {
                self.prefs.set_video_bitrate_kbps(kbps);
                cx.notify();
            }
            SettingsEffect::VideoHardwareAccelerationChanged(enabled) => {
                self.prefs.set_video_hardware_acceleration(enabled);
                cx.notify();
            }
            SettingsEffect::AllowGuildDmsChanged(enabled) => {
                self.prefs.set_allow_guild_dms(enabled);
                cx.notify();
            }
            SettingsEffect::FriendRequestAccessChanged(access) => {
                self.prefs.set_friend_request_access(&access);
                cx.notify();
            }
            SettingsEffect::SpoilerDisplayChanged(mode) => {
                self.prefs.set_spoiler_display(&mode);
                cx.notify();
            }
            SettingsEffect::InlineMediaPreviewsChanged(enabled) => {
                self.prefs.set_inline_media_previews(enabled);
                cx.notify();
            }
            SettingsEffect::RenderEmbedsChanged(enabled) => {
                self.prefs.set_render_embeds(enabled);
                cx.notify();
            }
            SettingsEffect::RenderReactionsChanged(enabled) => {
                self.prefs.set_render_reactions(enabled);
                cx.notify();
            }
            SettingsEffect::ConvertEmoticonsChanged(enabled) => {
                self.prefs.set_convert_emoticons(enabled);
                cx.notify();
            }
            SettingsEffect::UnreadBadgeEnabledChanged(enabled) => {
                self.prefs.set_unread_badge_enabled(enabled);
                cx.notify();
            }
            SettingsEffect::TimeFormat24hChanged(enabled) => {
                self.prefs.set_time_format_24h(enabled);
                cx.notify();
            }
            SettingsEffect::ReducedMotionChanged(pref) => {
                self.prefs.set_reduced_motion(pref);
                cx.set_reduce_motion(crate::platform::resolve_reduced_motion(pref));
                cx.notify();
            }
            SettingsEffect::ClearTrustedDomains => self.settings_clear_trusted_domains(cx),
            SettingsEffect::OpenAccountSwitcher => self.open_account_switcher(window, cx),
            SettingsEffect::LogOutRequested => {
                self.close_overlay(window, cx);
                self.log_out(cx);
            }
        }
    }

    pub(crate) fn settings_set_theme(&mut self, kind: ThemeKind, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.set_theme(kind);
        theme::set_kind(kind, cx);
        theme::apply(kind, window, cx);
        self.backend.update_user_settings(UserSettingsPatch {
            theme: Some(theme_patch_value(kind).to_owned()),
            ..Default::default()
        });
        cx.notify();
    }

    pub(crate) fn settings_set_notifications(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.prefs.set_notifications(enabled);
        self.notifier.enabled = enabled;
        cx.notify();
    }

    pub(crate) fn settings_set_sounds(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.prefs.set_sounds(enabled);
        self.notifier.sounds_enabled = enabled;
        cx.notify();
    }

    pub(crate) fn settings_set_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Err(err) = crate::platform::set_autostart_enabled(enabled) {
            tracing::warn!(error = %format!("{err:#}"), "could not update autostart setting");
            self.error = Some(l10n::tr("Something went wrong. Try again.").into());
            cx.notify();
            return;
        }
        self.prefs.set_autostart(enabled);
        cx.notify();
    }

    pub(crate) fn settings_set_close_to_tray(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.prefs.set_close_to_tray(enabled);
        crate::tray::set_close_to_tray(enabled);
        cx.notify();
    }

    pub(crate) fn settings_set_font_size(&mut self, size: u8, cx: &mut Context<Self>) {
        let clamped = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.prefs.set_font_size(clamped);
        theme::set_display(self.prefs.data.display(), cx);
        cx.notify();
    }

    pub(crate) fn settings_set_zoom(&mut self, zoom: u16, cx: &mut Context<Self>) {
        let clamped = zoom.clamp(MIN_UI_ZOOM, MAX_UI_ZOOM);
        self.prefs.set_ui_zoom(clamped);
        theme::set_display(self.prefs.data.display(), cx);
        cx.notify();
    }

    pub(crate) fn step_ui_zoom(&mut self, delta: i16, cx: &mut Context<Self>) {
        let current = i16::try_from(self.prefs.data.ui_zoom).unwrap_or(100);
        let next = (current + delta).clamp(MIN_UI_ZOOM as i16, MAX_UI_ZOOM as i16) as u16;
        self.settings_set_zoom(next, cx);
    }

    pub(crate) fn settings_set_compact(&mut self, compact: bool, cx: &mut Context<Self>) {
        self.prefs.set_compact(compact);
        theme::set_display(self.prefs.data.display(), cx);
        self.backend.update_user_settings(UserSettingsPatch {
            message_display_compact: Some(compact),
            ..Default::default()
        });
        cx.notify();
    }

    fn sync_voice_audio_settings(&self) {
        self.backend.set_voice_audio_settings(VoiceAudioSettings {
            input_device: self.prefs.data.voice_input_device.clone(),
            output_device: self.prefs.data.voice_output_device.clone(),
            input_volume: self.prefs.data.voice_input_volume,
            output_volume: self.prefs.data.voice_output_volume,
        });
    }

    pub(crate) fn settings_set_voice_input_device(&mut self, device: Option<String>, cx: &mut Context<Self>) {
        self.prefs.set_voice_input_device(device);
        self.sync_voice_audio_settings();
        cx.notify();
    }

    pub(crate) fn settings_set_voice_output_device(&mut self, device: Option<String>, cx: &mut Context<Self>) {
        self.prefs.set_voice_output_device(device);
        self.sync_voice_audio_settings();
        cx.notify();
    }

    pub(crate) fn settings_set_voice_input_volume(&mut self, volume: u16, cx: &mut Context<Self>) {
        self.prefs.set_voice_input_volume(volume);
        self.sync_voice_audio_settings();
        cx.notify();
    }

    pub(crate) fn settings_set_voice_output_volume(&mut self, volume: u16, cx: &mut Context<Self>) {
        self.prefs.set_voice_output_volume(volume);
        self.sync_voice_audio_settings();
        cx.notify();
    }

    pub(crate) fn settings_clear_trusted_domains(&mut self, cx: &mut Context<Self>) {
        self.prefs.clear_trusted_domains();
        self.backend.update_user_settings(UserSettingsPatch {
            trusted_domains: Some(Vec::new()),
            ..Default::default()
        });
        cx.notify();
    }

    pub(crate) fn settings_set_locale(&mut self, code: &str, cx: &mut Context<Self>) {
        let resolved = fluxer_client_i18n::resolve_locale(code);
        self.prefs.set_locale(resolved);
        l10n::set_locale(resolved);
        self.error = None;
        self.refresh_status_text();
        self.refresh(cx);
        self.backend.update_user_settings(UserSettingsPatch {
            locale: Some(resolved.to_owned()),
            ..Default::default()
        });
        cx.refresh_windows();
        cx.notify();
    }

    pub(crate) fn settings_prompt_logout(&mut self, confirm: bool, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.confirm_logout = confirm;
            if confirm {
                modal.sidebar_scroll.scroll_to_bottom();
            }
            cx.notify();
        }
    }
}
