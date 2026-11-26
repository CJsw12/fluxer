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

    /// Official sub-sections (`SettingsSectionRegistry.ts`) shown under the active tab in the left sidebar.
    pub const fn sections(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Profile => &[
                ("profile-customization", "Profile customization"),
                ("profile-badges", "Profile & status"),
            ],
            Self::Account => &[
                ("account", "Sign-in details"),
                ("security", "Security & passkeys"),
                ("trusted-domains", "Trusted domains"),
                ("danger-zone", "Danger zone"),
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
                ("message-preview", "Preview"),
                ("theme", "Theme"),
                ("message-display", "Message display"),
                ("chat-font-scaling", "Chat font scaling"),
                ("app-zoom-level", "App zoom level"),
            ],
            Self::Themes => &[
                ("base-theme", "Base theme"),
                ("colors-and-fonts", "Colors and fonts"),
                ("desktop-window", "Desktop window"),
            ],
            Self::Notifications => &[
                ("general", "Desktop notifications"),
                ("sounds", "Sounds"),
                ("unread-badge", "Unread & mention badges"),
            ],
            Self::MessagesMedia => &[
                ("display", "Display & previews"),
                ("spoilers", "Spoilers & reactions"),
                ("input", "Text input & autocomplete"),
            ],
            Self::VoiceVideo => &[
                ("voice-devices", "Devices & volume"),
                ("input-mode", "Input mode"),
                ("audio-processing", "Audio processing"),
                ("mic-test", "Microphone test"),
                ("video-quality", "Video & screen share"),
            ],
            Self::Accessibility => &[
                ("motion", "Motion"),
                ("visual", "Visual & contrast"),
                ("keyboard", "Keyboard & screen reader"),
            ],
            Self::Language => &[
                ("language-settings", "Interface language"),
                ("time-format", "Time & date format"),
            ],
            Self::Keybinds => &[
                ("keybinds-nav", "Navigation"),
                ("keybinds-chat", "Messages & chat"),
                ("keybinds-window", "Window & zoom"),
            ],
            Self::WindowsApp => &[
                ("desktop-window", "Desktop window"),
                ("tray-icon-color", "Tray icon color"),
            ],
            Self::Advanced => &[("updates", "About & updates")],
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
            cx.notify();
        }
    }
}

fn settings_sidebar_bg(theme: &Theme) -> Hsla {
    theme.background_tertiary
}

fn settings_card_bg(theme: &Theme) -> Hsla {
    theme.background_secondary_lighter
}

fn settings_border(theme: &Theme) -> Hsla {
    theme.background_modifier_accent
}

fn settings_sub_selected_bg(theme: &Theme) -> Hsla {
    theme.accent
}

fn settings_divider(theme: &Theme) -> AnyElement {
    div().w_full().h(px(1.)).flex_none().bg(theme.divider).into_any_element()
}

fn sidebar_section_heading(title: String, theme: &Theme) -> impl IntoElement {
    div()
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text_tertiary)
        .child(title)
}

fn section_heading(title: String, theme: &Theme) -> impl IntoElement {
    // `SettingsSection`: 28px title row, 18px/600 title (line-height 1.3), 8px gap to the link icon.
    div()
        .h(px(28.))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .flx("app.settings-section.section-title-row")
        .child(
            div()
                .text_size(px(18.))
                .line_height(px(23.4))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .flx("app.settings-section.section-title")
                .child(title),
        )
        .child(
            div()
                .flx("app.settings-heading-link-button.link-icon")
                .child(Icon::new(AppIcon::LinkSimple).with_size(px(15.)).text_color(theme.muted_foreground)),
        )
}

fn info_card_row(title: String, subtitle: String, badge: Option<String>, theme: &Theme) -> AnyElement {
    div()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(settings_card_bg(theme))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_xs().text_color(theme.muted_foreground).child(subtitle)),
        )
        .when_some(badge, |d, b| {
            d.child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded(px(4.))
                    .bg(theme.background)
                    .border_1()
                    .border_color(settings_border(theme))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(b),
            )
        })
        .into_any_element()
}

fn render_profile_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (avatar_el, display_name, user_id_str, banner_rgb, banner_url) = match &view.user {
        Some(user) => {
            let card = crate::profile::build_profile_card(&view.backend.state(), view.backend.media(), user.id, None, None);
            (
                render_avatar(
                    AvatarSpec {
                        user_id: user.id,
                        avatar_url: user.avatar_url.clone(),
                        display_name: user.name.clone(),
                        size: 64.,
                        status: Some(view.own_status),
                        status_border_bg: Some(theme.background_secondary),
                    },
                    theme,
                ),
                user.name.clone(),
                user.id.to_string(),
                Some(card.banner_color),
                card.banner_url,
            )
        }
        None => (
            div().size(px(64.)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("Fluxer User"),
            String::from("—"),
            None,
            None,
        ),
    };

    let banner = match banner_rgb {
        Some(color) => div().bg(gpui_kit::rgb(color)),
        None => div().bg(theme.brand),
    }
    .relative()
    .w_full()
    .h(px(90.))
    .overflow_hidden()
    .children(banner_url.map(|url| {
        gpui_kit::img(url)
            .id("settings-profile-banner")
            .size_full()
            .object_fit(gpui_kit::ObjectFit::Cover)
    }));

    let header_block = div()
        .relative()
        .w_full()
        .pb(px(42.))
        .child(banner)
        .child(
            div()
                .absolute()
                .left(px(16.))
                .top(px(54.))
                .p(px(6.))
                .rounded_full()
                .bg(theme.background_secondary)
                .child(avatar_el),
        );

    let status_options = [
        (Status::Online, "online"),
        (Status::Idle, "idle"),
        (Status::Dnd, "dnd"),
        (Status::Invisible, "invisible"),
    ];

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .debug_selector(|| "settings-profile-customization".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Profile customization"), theme))
                .child(
                    div()
                        .overflow_hidden()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .child(header_block)
                        .child(
                            div()
                                .px_4()
                                .pb_4()
                                .flex()
                                .flex_col()
                                .gap_4()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_base()
                                                .font_weight(FontWeight::BOLD)
                                                .truncate()
                                                .child(display_name),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(format!("ID: {user_id_str}")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1p5()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Online presence")),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .flex_wrap()
                                                .gap_2()
                                                .children(status_options.into_iter().map(|(st, key)| {
                                            let selected = view.own_status == st;
                                            let dot_col = status_color(st, theme);
                                            let label = status_label(st);
                                            let sel_id = format!("settings-profile-status-{key}");
                                            div()
                                                .id(SharedString::from(sel_id.clone()))
                                                .debug_selector(move || sel_id.clone())
                                                .px_3()
                                                .py_1p5()
                                                .rounded(px(6.))
                                                .border_1()
                                                .border_color(if selected { theme.brand } else { theme.border })
                                                .bg(if selected { theme.accent } else { theme.background })
                                                .cursor_pointer()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_1p5()
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.own_status = st;
                                                    this.backend.set_presence(st);
                                                    cx.notify();
                                                }))
                                                .child(div().size(px(8.)).rounded_full().bg(dot_col))
                                                .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(label))
                                        })),
                                        ),
                                ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Profile & status"), theme))
                .child(info_card_row(
                    l10n::tr("Custom status & avatar"),
                    l10n::tr("Synced with your Fluxer account across all signed-in devices."),
                    Some(l10n::tr("Active")),
                    theme,
                )),
        )
        .into_any_element()
}

fn voice_device_row(
    label: &str,
    device: Option<&str>,
    available_devices: &[String],
    id_prefix: &'static str,
    reset_id: &'static str,
    theme: &Theme,
    make_select: impl Fn(Option<String>) -> SettingsEffect + Copy + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let label_text = l10n::tr(label);
    let default_label = l10n::tr("System default");
    let display_name = device.map(str::to_owned).unwrap_or_else(|| default_label.clone());

    let default_selected = device.is_none();
    let default_sel_id = format!("{id_prefix}-default");
    let mut default_btn = Button::new(default_sel_id.clone())
        .small()
        .label(default_label.clone())
        .accessibility_label(format!("{label_text}: {default_label}"));
    default_btn = if default_selected {
        default_btn.primary()
    } else {
        default_btn.ghost()
    };
    let default_button = div().debug_selector(move || default_sel_id.clone()).child(
        default_btn.on_click(cx.listener(move |this, _, window, cx| {
            this.apply_settings_effect(make_select(None), window, cx);
        })),
    );

    let device_buttons = available_devices.iter().enumerate().map(|(ix, name)| {
        let selected = device == Some(name.as_str());
        let sel_id = format!("{id_prefix}-{ix}");
        let device_name = name.clone();
        let mut btn = Button::new((id_prefix, ix + 1))
            .small()
            .label(name.clone())
            .accessibility_label(format!("{label_text}: {name}"));
        btn = if selected { btn.primary() } else { btn.ghost() };
        div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
            move |this, _, window, cx| {
                this.apply_settings_effect(make_select(Some(device_name.clone())), window, cx);
            },
        )))
    });

    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().text_sm().child(label_text.clone()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child(display_name))
                        .when(device.is_some(), |d| {
                            d.child(
                                div().debug_selector(move || reset_id.to_owned()).child(
                                    Button::new(reset_id)
                                        .ghost()
                                        .small()
                                        .label(l10n::tr("Reset"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.apply_settings_effect(make_select(None), window, cx);
                                        })),
                                ),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .child(default_button)
                .children(device_buttons),
        )
        .into_any_element()
}

fn voice_volume_row(
    label: &str,
    volume: u16,
    dec_id: &'static str,
    inc_id: &'static str,
    reset_id: &'static str,
    make_effect: impl Fn(u16) -> SettingsEffect + Copy + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let clamped = volume.clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME);
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(l10n::tr(label)))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    div().debug_selector(move || dec_id.to_owned()).child(
                        Button::new(dec_id)
                            .ghost()
                            .small()
                            .label("-")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let next = clamped.saturating_sub(10).max(MIN_VOICE_VOLUME);
                                this.apply_settings_effect(make_effect(next), window, cx);
                            })),
                    ),
                )
                .child(div().px_2().text_xs().font_weight(FontWeight::SEMIBOLD).child(format!("{clamped}%")))
                .child(
                    div().debug_selector(move || inc_id.to_owned()).child(
                        Button::new(inc_id)
                            .ghost()
                            .small()
                            .label("+")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let next = clamped.saturating_add(10).min(MAX_VOICE_VOLUME);
                                this.apply_settings_effect(make_effect(next), window, cx);
                            })),
                    ),
                )
                .child(
                    div().debug_selector(move || reset_id.to_owned()).child(
                        Button::new(reset_id)
                            .ghost()
                            .small()
                            .label(l10n::tr("Reset"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply_settings_effect(make_effect(DEFAULT_VOICE_VOLUME), window, cx);
                            })),
                    ),
                ),
        )
        .into_any_element()
}

fn render_account_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let trusted_count = view.effective_trusted_domains().len();
    let switch_label = if view.saved_accounts.len() > 1 {
        format!("{} ({})", l10n::tr("Switch accounts"), view.saved_accounts.len())
    } else {
        l10n::tr("Switch accounts")
    };
    let (user_name, user_id_str) = match &view.user {
        Some(u) => (u.name.to_string(), u.id.to_string()),
        None => (l10n::tr("Signed-in user"), String::from("—")),
    };
    let webauthn_supported = crate::webauthn::is_supported();
    let platform_auth = crate::webauthn::is_platform_authenticator_available();
    let support_note = if platform_auth {
        l10n::tr("Windows Hello & FIDO2 security keys ready")
    } else if webauthn_supported {
        l10n::tr("Hardware security keys (USB/NFC) ready")
    } else {
        l10n::tr("Passkeys are not supported on this platform")
    };

    let mut passkey_list = div().flex().flex_col().gap_1();
    if modal.passkey_loading {
        passkey_list = passkey_list.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Loading passkeys…")),
        );
    } else if modal.passkeys.is_empty() {
        passkey_list = passkey_list.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("No passkeys or security keys registered yet.")),
        );
    } else {
        for (ix, cred) in modal.passkeys.iter().enumerate() {
            let cid = cred.id.clone();
            let display_name = if cred.name.is_empty() {
                cred.id.clone()
            } else {
                cred.name.clone()
            };
            passkey_list = passkey_list.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded(px(6.))
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(display_name))
                            .when_some(cred.created_at.clone(), |d, created| {
                                d.child(div().text_xs().text_color(theme.muted_foreground).child(created))
                            }),
                    )
                    .child(
                        Button::new(("settings-passkey-remove", ix))
                            .danger()
                            .xsmall()
                            .disabled(modal.passkey_busy)
                            .label(l10n::tr("Remove"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.settings_delete_passkey(cid.clone(), window, cx);
                            })),
                    ),
            );
        }
    }

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Sign-in details"), theme))
                .child(info_card_row(
                    user_name,
                    format!("{}: {user_id_str}", l10n::tr("Account ID")),
                    Some(l10n::tr("Verified")),
                    theme,
                )),
        )
        .child(
            div()
                .debug_selector(|| "settings-account-passkeys".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Passkeys & security keys"), theme))
                .child(
                    div()
                        .p_4()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(settings_border(theme))
                        .bg(settings_card_bg(theme))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(support_note),
                                )
                                .child(
                                    Button::new("settings-passkey-refresh")
                                        .ghost()
                                        .xsmall()
                                        .disabled(modal.passkey_loading || modal.passkey_busy)
                                        .label(l10n::tr("Refresh"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Overlay::Settings(m) = &mut this.overlay {
                                                m.passkey_loading = true;
                                                m.passkey_error = None;
                                            }
                                            this.backend.load_webauthn_credentials();
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(passkey_list)
                        .child(Input::new(&modal.passkey_password).mask_toggle().disabled(modal.passkey_busy))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&modal.passkey_name).disabled(modal.passkey_busy)),
                                )
                                .child(
                                    div().debug_selector(|| "settings-passkey-add-btn".to_owned()).child(
                                        Button::new("settings-passkey-add-btn")
                                            .primary()
                                            .small()
                                            .loading(modal.passkey_busy)
                                            .disabled(modal.passkey_busy || !webauthn_supported)
                                            .label(l10n::tr("Add passkey"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.settings_register_passkey(window, cx);
                                            })),
                                    ),
                                ),
                        )
                        .when_some(modal.passkey_status.clone(), |d, msg| {
                            d.child(div().text_xs().text_color(theme.status_online).child(msg))
                        })
                        .when_some(modal.passkey_error.clone(), |d, err| {
                            d.child(div().text_xs().text_color(theme.danger).child(err))
                        }),
                ),
        )
        .when(view.accounts_enabled, |d| {
            d.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_heading(l10n::tr("Accounts"), theme))
                    .child(
                        div()
                            .p_4()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(settings_border(theme))
                            .bg(settings_card_bg(theme))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(l10n::tr("Switch accounts")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(l10n::tr("Add or switch between saved accounts.")),
                                    ),
                            )
                            .child(
                                div().debug_selector(|| "settings-account-switch-accounts".to_owned()).child(
                                    Button::new("settings-account-switch-accounts")
                                        .ghost()
                                        .small()
                                        .label(switch_label)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(SettingsEffect::OpenAccountSwitcher, window, cx);
                                        })),
                                ),
                            ),
                    ),
            )
        })
        .child(
            div()
                .debug_selector(|| "settings-account-trusted".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Trusted domains"), theme))
                .child(
                    div()
                        .p_4()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(settings_border(theme))
                        .bg(settings_card_bg(theme))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .child(format!("{} ({trusted_count})", l10n::tr("Trusted domains"))),
                        )
                        .when(trusted_count > 0, |d| {
                            d.child(
                                div().debug_selector(|| "settings-clear-trusted".to_owned()).child(
                                    Button::new("settings-clear-trusted")
                                        .ghost()
                                        .small()
                                        .label(l10n::tr("Clear"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(SettingsEffect::ClearTrustedDomains, window, cx);
                                        })),
                                ),
                            )
                        }),
                ),
        )
        .into_any_element()
}

fn settings_section_block(title: String, description: Option<String>, content: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .flx("app.settings-section.section")
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flx("app.settings-section.section-header")
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flx("app.settings-section.section-header-text")
                        .child(section_heading(title, theme))
                        .when_some(description, |d, desc| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .flx("app.settings-section.section-description")
                                    .child(desc),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .flx("app.settings-section.section-content")
                .child(content),
        )
        .into_any_element()
}

fn render_privacy_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    const CALL_EVERYONE: u32 = 1 << 2;
    const CALL_FRIENDS_ONLY: u32 = 1 << 3;
    const CALL_NOBODY: u32 = 1 << 4;
    const CALL_SILENT: u32 = 1 << 5;
    const GDM_EVERYONE: u32 = 1 << 2;
    const GDM_FRIENDS_ONLY: u32 = 1 << 3;
    const GDM_NOBODY: u32 = 1 << 4;

    let trusted_count = view.effective_trusted_domains().len();
    let active_friend_req = view.prefs.data.friend_request_access.as_str();
    let active_spoiler = view.prefs.data.spoiler_display.as_str();

    let (
        profile_privacy,
        default_guilds_restricted,
        bot_default_guilds_restricted,
        incoming_call_flags,
        group_dm_flags,
        default_share_voice_activity,
        friend_dm_filter,
        non_friend_dm_filter,
        guild_filter,
    ) = {
        let state = view.backend.state();
        let raw = &state.user_settings.raw;
        (
            raw.get("profile_privacy").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
            raw.get("default_guilds_restricted")
                .and_then(|v| v.as_bool())
                .unwrap_or(!view.prefs.data.allow_guild_dms),
            raw.get("bot_default_guilds_restricted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            raw.get("incoming_call_flags")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::from(CALL_FRIENDS_ONLY)) as u32,
            raw.get("group_dm_add_permission_flags")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::from(GDM_FRIENDS_ONLY)) as u32,
            raw.get("default_share_voice_activity")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            raw.get("sensitive_content_friend_dm_filter")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u8,
            raw.get("sensitive_content_non_friend_dm_filter")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u8,
            raw.get("sensitive_content_guild_filter")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u8,
        )
    };

    let profile_privacy_options: &[(u8, &str, &str)] = &[
        (
            0,
            "Friends and all communities",
            "Your full profile is visible to friends and to anyone in your communities",
        ),
        (
            1,
            "Friends and small communities only",
            "Your full profile is visible to friends and members of your communities with 200 or fewer members",
        ),
        (2, "Friends only", "Your full profile is only visible to your friends"),
    ];
    let friend_req_options: &[(&str, &str, u32)] = &[
        ("everyone", "Everyone", 1 << 2),
        ("friends_of_friends", "Friends of Friends", 1 << 0),
        ("no_one", "No one", 0),
    ];
    let call_options: &[(u32, &str)] = &[
        (CALL_NOBODY, "Block all"),
        (CALL_FRIENDS_ONLY, "Friends only"),
        (CALL_EVERYONE, "Everyone"),
    ];
    let group_dm_options: &[(u32, &str)] = &[
        (GDM_NOBODY, "No one"),
        (GDM_FRIENDS_ONLY, "Friends only"),
        (GDM_EVERYONE, "Everyone"),
    ];
    let sensitive_dm_options: &[(u8, &str)] = &[(0, "Show"), (1, "Blur"), (2, "Block")];
    let sensitive_guild_options: &[(u8, &str)] = &[(0, "Show"), (1, "Blur")];
    let spoiler_options: &[(&str, &str)] = &[
        ("on_click", "On click"),
        ("moderator", "In channels I moderate"),
        ("always", "Always"),
    ];

    let active_call_base = if (incoming_call_flags & CALL_NOBODY) != 0 {
        CALL_NOBODY
    } else if (incoming_call_flags & CALL_EVERYONE) != 0 {
        CALL_EVERYONE
    } else {
        CALL_FRIENDS_ONLY
    };
    let silent_calls = (incoming_call_flags & CALL_SILENT) != 0;
    let active_gdm_base = if (group_dm_flags & GDM_NOBODY) != 0 {
        GDM_NOBODY
    } else if (group_dm_flags & GDM_EVERYONE) != 0 {
        GDM_EVERYONE
    } else {
        GDM_FRIENDS_ONLY
    };

    let profile_privacy_section = settings_section_block(
        l10n::tr("Profile privacy"),
        Some(l10n::tr("Who can see your full profile")),
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(profile_privacy_options.iter().map(|&(level, label, desc)| {
                let selected = profile_privacy == level;
                let sel_id = format!("settings-profile-privacy-{level}");
                div()
                    .id(SharedString::from(sel_id.clone()))
                    .debug_selector(move || sel_id.clone())
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if selected { theme.brand } else { settings_border(theme) })
                    .bg(settings_card_bg(theme))
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_update_privacy_patch(
                            UserSettingsPatch {
                                profile_privacy: Some(level),
                                ..Default::default()
                            },
                            cx,
                        );
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr(label)))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr(desc))),
                    )
                    .when(selected, |d| {
                        d.child(Icon::new(AppIcon::Check).with_size(px(16.)).text_color(theme.brand))
                    })
            }))
            .into_any_element(),
        theme,
    );

    let friends_dms_section = settings_section_block(
        l10n::tr("Friends & direct messages"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Allow direct messages from community members")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Applies to communities you join from now on. You can also change it for each community.",
                                    )),
                            ),
                    )
                    .child(
                        div().debug_selector(|| "settings-allow-guild-dms".to_owned()).child(
                            Switch::new("settings-allow-guild-dms")
                                .checked(!default_guilds_restricted)
                                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                    this.apply_settings_effect(
                                        SettingsEffect::AllowGuildDmsChanged(*checked),
                                        window,
                                        cx,
                                    );
                                    this.settings_update_privacy_patch(
                                        UserSettingsPatch {
                                            default_guilds_restricted: Some(!*checked),
                                            ..Default::default()
                                        },
                                        cx,
                                    );
                                })),
                        ),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Allow direct messages from community bots")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Applies to bots in communities you join from now on. You can also change it for each community.",
                                    )),
                            ),
                    )
                    .child(
                        div().debug_selector(|| "settings-allow-bot-dms".to_owned()).child(
                            Switch::new("settings-allow-bot-dms")
                                .checked(!bot_default_guilds_restricted)
                                .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                    this.settings_update_privacy_patch(
                                        UserSettingsPatch {
                                            bot_default_guilds_restricted: Some(!*checked),
                                            ..Default::default()
                                        },
                                        cx,
                                    );
                                })),
                        ),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Friend requests")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Choose who can send you a friend request.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .children(friend_req_options.iter().map(|&(val, label, flag_bits)| {
                                let selected = active_friend_req == val;
                                let sel_id = format!("settings-friend-req-{val}");
                                let val_owned = val.to_owned();
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::FriendRequestAccessChanged(val_owned.clone()),
                                            window,
                                            cx,
                                        );
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                friend_source_flags: Some(flag_bits),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let communication_section = settings_section_block(
        l10n::tr("Communication"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.incoming-call-base-change.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.incoming-call-base-change.text")
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.incoming-call-base-change.label")
                                    .child(l10n::tr("Allowed callers")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Choose who can start a call with you.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.incoming-call-base-change")
                            .children(call_options.iter().map(|&(bits, label)| {
                                let selected = active_call_base == bits;
                                let sel_id = format!("settings-incoming-call-{bits}");
                                let next_flags = if bits == CALL_NOBODY {
                                    CALL_NOBODY
                                } else if silent_calls {
                                    bits | CALL_SILENT
                                } else {
                                    bits
                                };
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                incoming_call_flags: Some(next_flags),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .when(active_call_base != CALL_NOBODY, |d| {
                d.child(
                    div()
                        .py_2p5()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Receive calls silently")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr("Calls you allow still come through, but they don't ring.")),
                                ),
                        )
                        .child(
                            Switch::new("settings-silent-calls")
                                .checked(silent_calls)
                                .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                    let next = if *checked {
                                        active_call_base | CALL_SILENT
                                    } else {
                                        active_call_base & !CALL_SILENT
                                    };
                                    this.settings_update_privacy_patch(
                                        UserSettingsPatch {
                                            incoming_call_flags: Some(next),
                                            ..Default::default()
                                        },
                                        cx,
                                    );
                                })),
                        ),
                )
            })
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.group-dm-add-base-change.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.group-dm-add-base-change.text")
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.group-dm-add-base-change.label")
                                    .child(l10n::tr("Allowed group chat invites")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Choose who can add you to a group chat without asking first.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.group-dm-add-base-change")
                            .children(group_dm_options.iter().map(|&(bits, label)| {
                                let selected = active_gdm_base == bits;
                                let sel_id = format!("settings-group-dm-add-{bits}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                group_dm_add_permission_flags: Some(bits),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let activity_sharing_section = settings_section_block(
        l10n::tr("Activity sharing"),
        None,
        info_card_row(
            l10n::tr("Share your voice activity with friends"),
            l10n::tr("Friends see which voice channel you're in on their Active now list. You can change this once every 24 hours."),
            Some(if default_share_voice_activity {
                l10n::tr("On")
            } else {
                l10n::tr("Off")
            }),
            theme,
        ),
        theme,
    );

    let sensitive_content_section = settings_section_block(
        l10n::tr("Sensitive content"),
        Some(l10n::tr(
            "Choose what happens to images and videos flagged as sensitive. Show displays them as usual, Blur hides them until you click, and Block hides them completely.",
        )),
        div()
            .flex()
            .flex_col()
            .gap_2()
            .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.rows")
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-friend-dm-filter.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Direct messages from friends")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Media your friends send you in direct messages.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-friend-dm-filter")
                            .children(sensitive_dm_options.iter().map(|&(lvl, label)| {
                                let selected = friend_dm_filter == lvl;
                                let sel_id = format!("settings-sensitive-friend-{lvl}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                sensitive_content_friend_dm_filter: Some(lvl),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-non-friend-dm-filter.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Direct messages from others")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Media in group chats and in direct messages from people who aren't your friends.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-non-friend-dm-filter")
                            .children(sensitive_dm_options.iter().map(|&(lvl, label)| {
                                let selected = non_friend_dm_filter == lvl;
                                let sel_id = format!("settings-sensitive-other-{lvl}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                sensitive_content_non_friend_dm_filter: Some(lvl),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-guild-filter.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Messages in community channels")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Media in channels that are not marked 18+.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-guild-filter")
                            .children(sensitive_guild_options.iter().map(|&(lvl, label)| {
                                let selected = guild_filter == lvl;
                                let sel_id = format!("settings-sensitive-guild-{lvl}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                sensitive_content_guild_filter: Some(lvl),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Spoilers & sensitive media")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Choose when spoiler-tagged text and media are revealed.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .children(spoiler_options.iter().map(|&(val, label)| {
                                let selected = active_spoiler == val;
                                let sel_id = format!("settings-spoiler-mode-{val}");
                                let val_owned = val.to_owned();
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::SpoilerDisplayChanged(val_owned.clone()),
                                            window,
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let link_protection_section = settings_section_block(
        l10n::tr("Link protection"),
        None,
        div()
            .py_2p5()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("{} ({trusted_count})", l10n::tr("Trusted external domains"))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Untrusted external links show a confirmation dialog before opening.")),
                    ),
            )
            .when(trusted_count > 0, |d| {
                d.child(
                    Button::new("settings-privacy-clear-trusted")
                        .ghost()
                        .small()
                        .label(l10n::tr("Clear"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_settings_effect(SettingsEffect::ClearTrustedDomains, window, cx);
                        })),
                )
            })
            .into_any_element(),
        theme,
    );

    let data_export_section = settings_section_block(
        l10n::tr("Data export"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flx("user.privacy-safety-tab.data-export-tab.data-export-tab-content.delete-section")
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .flx("user.privacy-safety-tab.data-export-tab.data-export-tab-content.warning-text")
                    .child(l10n::tr(
                        "Request a copy of your account data. Export execution requires verified confirmation.",
                    )),
            )
            .child(
                div()
                    .debug_selector(|| "settings-privacy-data-export".to_owned())
                    .flx("user.privacy-safety-tab.data-export-tab.data-export-tab-content.button.open-modal")
                    .child(
                        Button::new("settings-privacy-data-export")
                            .primary()
                            .small()
                            .label(l10n::tr("Export my data"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                // TODO(backlog, T3): Connect POST /users/@me/harvest data export flow.
                                this.settings_set_privacy_notice(
                                    l10n::tr("Data export requests are managed in the web client for now."),
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let data_deletion_section = settings_section_block(
        l10n::tr("Data deletion"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flx("user.privacy-safety-tab.data-deletion-tab.data-deletion-tab-content.delete-section")
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .flx("user.privacy-safety-tab.data-deletion-tab.data-deletion-tab-content.warning-text")
                    .child(l10n::tr(
                        "Permanently delete your messages or account data. This action cannot be undone.",
                    )),
            )
            .child(
                div()
                    .debug_selector(|| "settings-privacy-data-deletion".to_owned())
                    .flx("user.privacy-safety-tab.data-deletion-tab.data-deletion-tab-content.button.open-modal")
                    .child(
                        Button::new("settings-privacy-data-deletion")
                            .danger()
                            .small()
                            .label(l10n::tr("Delete my messages"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                // TODO(backlog, T3): Connect verified message/account data deletion flow.
                                this.settings_set_privacy_notice(
                                    l10n::tr("Bulk data deletion requires verified confirmation in the web client."),
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    div()
        .debug_selector(|| "settings-privacy-dm".to_owned())
        .flex()
        .flex_col()
        .gap_5()
        .flx("user.privacy-safety-tab.privacy-dashboard-content.container")
        .when_some(modal.privacy_notice.clone(), |d, notice| {
            d.child(
                div()
                    .debug_selector(|| "settings-privacy-notice".to_owned())
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .bg(settings_card_bg(theme))
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(notice),
            )
        })
        .child(profile_privacy_section)
        .child(settings_divider(theme))
        .child(friends_dms_section)
        .child(settings_divider(theme))
        .child(communication_section)
        .child(settings_divider(theme))
        .child(activity_sharing_section)
        .child(settings_divider(theme))
        .child(sensitive_content_section)
        .child(settings_divider(theme))
        .child(link_protection_section)
        .child(settings_divider(theme))
        .child(data_export_section)
        .child(settings_divider(theme))
        .child(data_deletion_section)
        .into_any_element()
}

fn render_connections_tab(modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let platform_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .flx("user.linked-accounts-tab.platform-row")
        .child(
            div()
                .debug_selector(|| "settings-connections-add-domain".to_owned())
                .flx("user.linked-accounts-tab.platform-icon-button.add-connection--2")
                .child(
                    Button::new("settings-connections-add-domain")
                        .ghost()
                        .small()
                        .icon(AppIcon::LinkSimple)
                        .label(l10n::tr("Add domain"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            // TODO(backlog, T3): Connect POST /users/@me/connections domain verification flow.
                            this.settings_set_connections_notice(
                                l10n::tr("Adding domain connections requires OAuth/DNS verification (coming soon)."),
                                cx,
                            );
                        })),
                ),
        )
        .child(
            div()
                .debug_selector(|| "settings-connections-add-oauth".to_owned())
                .child(
                    Button::new("settings-connections-add-oauth")
                        .ghost()
                        .small()
                        .icon(AppIcon::Plus)
                        .label(l10n::tr("Add external account"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            // TODO(backlog, T3): Connect OAuth external account linking flow.
                            this.settings_set_connections_notice(
                                l10n::tr("Linking external OAuth accounts is not yet enabled in this client."),
                                cx,
                            );
                        })),
                ),
        );

    let list_body: AnyElement = if modal.connections_loading && modal.connections.is_empty() {
        div()
            .debug_selector(|| "settings-connections-loading".to_owned())
            .p_4()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(l10n::tr("Loading connections…"))
            .into_any_element()
    } else if let Some(err) = &modal.connections_error {
        div()
            .debug_selector(|| "settings-connections-error".to_owned())
            .p_4()
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(settings_card_bg(theme))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .child(div().text_xs().text_color(theme.danger).child(err.clone()))
            .child(
                Button::new("settings-connections-retry")
                    .ghost()
                    .small()
                    .label(l10n::tr("Try again"))
                    .on_click(cx.listener(|this, _, _, cx| this.settings_reload_connections(cx))),
            )
            .into_any_element()
    } else if modal.connections.is_empty() {
        div()
            .debug_selector(|| "settings-connections-empty".to_owned())
            .p_6()
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(settings_card_bg(theme))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .flx("user.linked-accounts-tab.empty-state")
            .child(Icon::new(AppIcon::Users).with_size(px(28.)).text_color(theme.muted_foreground))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("No connections yet")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("Verify domain ownership or link accounts to display them on your profile.")),
            )
            .into_any_element()
    } else {
        div()
            .debug_selector(|| "settings-connections-list".to_owned())
            .flex()
            .flex_col()
            .gap_2()
            .flx("user.linked-accounts-tab.connections-list")
            .children(modal.connections.iter().map(|conn| {
                let conn_id = conn.id.clone();
                let conn_kind = conn.kind.clone();
                let conn_name = conn.name.clone();
                let verified = conn.verified;
                let visible = (conn.visibility_flags & CONNECTION_VISIBILITY_EVERYONE) != 0;
                let provider_label = match conn.kind.as_str() {
                    "bsky" => "Bluesky",
                    "domain" => "Domain",
                    "oauth" => "OAuth",
                    other => other,
                };
                let card_sel = format!("settings-connection-card-{}", conn.id);
                let toggle_id = format!("settings-connection-visible-{}", conn.id);
                let remove_id = format!("settings-connection-remove-{}", conn.id);
                let toggle_kind = conn_kind.clone();
                let toggle_conn_id = conn_id.clone();
                let remove_name = conn_name.clone();
                div()
                    .debug_selector(move || card_sel.clone())
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .bg(settings_card_bg(theme))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .flx("user.linked-accounts-tab.connection-card.card")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(conn_name))
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded(px(4.))
                                            .bg(theme.muted)
                                            .text_size(px(11.))
                                            .text_color(if verified { theme.brand } else { theme.muted_foreground })
                                            .child(if verified {
                                                l10n::tr("Verified")
                                            } else {
                                                l10n::tr("Unverified")
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{provider_label} · {}", if visible { "Shown on profile" } else { "Hidden" })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                Switch::new(SharedString::from(toggle_id))
                                    .checked(visible)
                                    .on_change(cx.listener(move |this, _, _, cx| {
                                        this.settings_toggle_connection_visibility(
                                            toggle_kind.clone(),
                                            toggle_conn_id.clone(),
                                            visible,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(remove_id))
                                    .ghost()
                                    .small()
                                    .icon(AppIcon::Trash)
                                    .tooltip(l10n::tr("Remove connection"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        // TODO(backlog, T3): Connect DELETE /users/@me/connections/{type}/{id}.
                                        this.settings_set_connections_notice(
                                            format!(
                                                "{} ({remove_name})",
                                                l10n::tr("Removing connections is deferred to the T3 workflow.")
                                            ),
                                            cx,
                                        );
                                    })),
                            ),
                    )
            }))
            .into_any_element()
    };

    div()
        .debug_selector(|| "settings-connections-tab".to_owned())
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section_block(
            l10n::tr("Connections"),
            Some(l10n::tr(
                "Link external accounts and domains to your Fluxer profile. Verified connections will be displayed on your profile for others to see.",
            )),
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(platform_row)
                .when_some(modal.connections_notice.clone(), |d, notice| {
                    d.child(
                        div()
                            .debug_selector(|| "settings-connections-notice".to_owned())
                            .p_2p5()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(settings_border(theme))
                            .bg(settings_card_bg(theme))
                            .text_xs()
                            .text_color(theme.foreground)
                            .child(notice),
                    )
                })
                .child(list_body)
                .into_any_element(),
            theme,
        ))
        .into_any_element()
}

fn render_appearance_preview(view: &ClientView, theme: &Theme) -> AnyElement {
    let display = view.prefs.data.display();
    let msg_size = px(f32::from(display.font_size));
    let (avatar_el, user_name) = match &view.user {
        Some(user) => (
            render_avatar(
                AvatarSpec {
                    user_id: user.id,
                    avatar_url: user.avatar_url.clone(),
                    display_name: user.name.clone(),
                    size: 40.,
                    status: None,
                    status_border_bg: None,
                },
                theme,
            ),
            user.name.clone(),
        ),
        None => (
            div().size(px(40.)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("Fluxer User"),
        ),
    };

    let preview_body = if display.compact {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap_2()
                    .child(div().text_xs().text_color(theme.muted_foreground).child("12:00"))
                    .child(div().text_size(msg_size).font_weight(FontWeight::SEMIBOLD).child(user_name.clone()))
                    .child(div().text_size(msg_size).child(l10n::tr("This is how messages appear"))),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap_2()
                    .child(div().text_xs().text_color(theme.muted_foreground).child("12:00"))
                    .child(div().text_size(msg_size).font_weight(FontWeight::SEMIBOLD).child(user_name))
                    .child(div().text_size(msg_size).child(l10n::tr("With different display modes available"))),
            )
            .into_any_element()
    } else {
        div()
            .flex()
            .flex_row()
            .items_start()
            .gap_3()
            .child(div().flex_none().child(avatar_el))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_baseline()
                            .gap_2()
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(user_name))
                            .child(div().text_xs().text_color(theme.muted_foreground).child("Today at 12:00 PM")),
                    )
                    .child(div().text_size(msg_size).child(l10n::tr("This is how messages appear")))
                    .child(div().text_size(msg_size).child(l10n::tr("With different display modes available"))),
            )
            .into_any_element()
    };

    div()
        .debug_selector(|| "settings-appearance-preview".to_owned())
        .p_4()
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(settings_card_bg(theme))
        .child(preview_body)
        .into_any_element()
}

fn render_theme_swatch(
    ix: usize,
    kind: ThemeKind,
    label: &'static str,
    selected: bool,
    theme: &Theme,
    cx: &Context<ClientView>,
) -> AnyElement {
    let dark_preview = Theme::preview(ThemeKind::Dark);
    let light_preview = Theme::preview(ThemeKind::Light);
    let preview = Theme::preview(kind);
    let sel_id = format!("settings-theme-swatch-{}", kind.as_str());

    let mini_window = if kind == ThemeKind::System {
        div()
            .w_full()
            .h(px(54.))
            .rounded(px(6.))
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .flex()
            .flex_row()
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(dark_preview.background)
                    .flex()
                    .flex_row()
                    .child(div().w(px(12.)).h_full().bg(dark_preview.background_tertiary))
                    .child(
                        div()
                            .flex_1()
                            .p_1p5()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().w(px(24.)).h(px(5.)).rounded(px(2.)).bg(dark_preview.brand))
                            .child(div().w(px(30.)).h(px(4.)).rounded(px(2.)).bg(dark_preview.muted_foreground)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(light_preview.background)
                    .flex()
                    .flex_row()
                    .child(div().w(px(12.)).h_full().bg(light_preview.background_secondary))
                    .child(
                        div()
                            .flex_1()
                            .p_1p5()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().w(px(24.)).h(px(5.)).rounded(px(2.)).bg(light_preview.brand))
                            .child(div().w(px(30.)).h(px(4.)).rounded(px(2.)).bg(light_preview.muted_foreground)),
                    ),
            )
    } else {
        div()
            .w_full()
            .h(px(54.))
            .rounded(px(6.))
            .overflow_hidden()
            .border_1()
            .border_color(preview.border)
            .bg(preview.background)
            .flex()
            .flex_row()
            .child(div().w(px(14.)).h_full().bg(preview.background_tertiary))
            .child(div().w(px(24.)).h_full().bg(preview.background_secondary))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .p_1p5()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().w(px(34.)).h(px(5.)).rounded(px(2.)).bg(preview.foreground))
                            .child(div().w(px(46.)).h(px(4.)).rounded(px(2.)).bg(preview.muted_foreground)),
                    )
                    .child(div().w(px(22.)).h(px(6.)).rounded(px(3.)).bg(preview.brand)),
            )
    };

    div()
        .id(("settings-theme", ix))
        .role(gpui_kit::Role::Button)
        .aria_label(l10n::tr(label))
        .aria_selected(selected)
        .debug_selector(move || sel_id.clone())
        .relative()
        .w(px(120.))
        .p_2()
        .rounded(px(10.))
        .border_2()
        .border_color(if selected { theme.brand } else { settings_border(theme) })
        .bg(if selected {
            settings_sub_selected_bg(theme)
        } else {
            settings_card_bg(theme)
        })
        .cursor_pointer()
        .hover(|s| s.border_color(theme.brand))
        .flex()
        .flex_col()
        .gap_1p5()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.apply_settings_effect(SettingsEffect::ThemeChanged(kind), window, cx);
        }))
        .child(mini_window)
        .when(selected, |d| {
            d.child(
                div()
                    .absolute()
                    .top_1()
                    .right_1()
                    .size(px(18.))
                    .rounded_full()
                    .bg(theme.brand)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(AppIcon::Check).xsmall().text_color(theme.on_fill)),
            )
        })
        .child(
            div()
                .text_xs()
                .font_weight(if selected { FontWeight::BOLD } else { FontWeight::MEDIUM })
                .text_color(if selected { theme.foreground } else { theme.muted_foreground })
                .truncate()
                .child(l10n::tr(label)),
        )
        .into_any_element()
}

fn render_appearance_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_theme = theme::kind(cx);
    let display = view.prefs.data.display();
    let hover_bg = theme.hover_bg;

    let theme_swatches = THEME_OPTIONS.iter().enumerate().map(|(ix, (kind, label))| {
        let kind = *kind;
        render_theme_swatch(ix, kind, label, active_theme == kind, theme, cx)
    });

    let density_cards = [
        (false, "Comfy", "Spacious layout with clear visual separation between messages.", "settings-density-cozy"),
        (true, "Dense", "Maximizes visible messages with minimal spacing.", "settings-density-compact"),
    ]
    .into_iter()
    .map(|(compact, label, desc, sel_id)| {
        let selected = display.compact == compact;
        div()
            .id(sel_id)
            .role(gpui_kit::Role::Button)
            .aria_label(l10n::tr(label))
            .aria_selected(selected)
            .debug_selector(move || sel_id.to_owned())
            .py_1p5()
            .px_2()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .flex()
            .flex_row()
            .items_start()
            .gap_3()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.apply_settings_effect(SettingsEffect::CompactChanged(compact), window, cx);
            }))
            .child(
                div()
                    .mt(px(2.))
                    .size(px(18.))
                    .flex_none()
                    .rounded_full()
                    .border_2()
                    .border_color(if selected { theme.brand } else { settings_border(theme) })
                    .when(selected, |d| {
                        d.bg(theme.brand)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(6.)).rounded_full().bg(theme.on_fill))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr(label)))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr(desc))),
            )
    });

    let font_fraction = {
        let idx = FONT_SIZE_OPTIONS
            .iter()
            .position(|&s| s == display.font_size)
            .unwrap_or(1);
        idx as f32 / (FONT_SIZE_OPTIONS.len().saturating_sub(1).max(1)) as f32
    };
    let font_stops = FONT_SIZE_OPTIONS.iter().map(|&size| {
        let selected = display.font_size == size;
        let is_default = size == 16;
        let sel_id = format!("settings-font-{size}");
        let btn_id = sel_id.clone();
        div()
            .id(SharedString::from(btn_id))
            .role(gpui_kit::Role::Button)
            .aria_label(format!("{size}px"))
            .aria_selected(selected)
            .debug_selector(move || sel_id.clone())
            .cursor_pointer()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.apply_settings_effect(SettingsEffect::FontSizeChanged(size), window, cx);
            }))
            .child(
                div()
                    .size(px(if selected { 12. } else { 8. }))
                    .rounded_full()
                    .border_2()
                    .border_color(if selected {
                        theme.brand
                    } else if is_default {
                        theme.status_online
                    } else {
                        settings_border(theme)
                    })
                    .bg(if selected { theme.on_fill } else { settings_card_bg(theme) }),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(if selected || is_default { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                    .text_color(if selected {
                        theme.foreground
                    } else if is_default {
                        theme.status_online
                    } else {
                        theme.muted_foreground
                    })
                    .child(format!("{size}px")),
            )
    });

    let font_slider = div()
        .debug_selector(|| "settings-font-slider".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Chat font size")))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(format!("{}px", display.font_size)),
                ),
        )
        .child(
            div()
                .relative()
                .w_full()
                .pt_1()
                .child(
                    div()
                        .w_full()
                        .h(px(4.))
                        .rounded_full()
                        .bg(settings_border(theme))
                        .overflow_hidden()
                        .child(div().h_full().bg(theme.brand).w(gpui_kit::relative(font_fraction))),
                )
                .child(
                    div()
                        .mt_2()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .children(font_stops),
                ),
        );

    let zoom_fraction = (f32::from(display.ui_zoom.saturating_sub(MIN_UI_ZOOM))
        / f32::from(MAX_UI_ZOOM.saturating_sub(MIN_UI_ZOOM).max(1)))
    .clamp(0., 1.);
    let zoom_ticks = [50u16, 75, 100, 125, 150, 175, 200].into_iter().map(|pct| {
        let is_default = pct == 100;
        let is_current = display.ui_zoom == pct;
        div()
            .text_xs()
            .text_color(if is_default {
                theme.status_online
            } else if is_current {
                theme.foreground
            } else {
                theme.muted_foreground
            })
            .child(format!("{pct}%"))
    });
    let zoom_controls = div()
        .debug_selector(|| "settings-zoom-slider".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(section_heading(l10n::tr("App zoom level"), theme))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            div().debug_selector(|| "settings-zoom-out".to_owned()).child(
                                Button::new("settings-zoom-out")
                                    .ghost()
                                    .xsmall()
                                    .label("-")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let next = display.ui_zoom.saturating_sub(10).max(MIN_UI_ZOOM);
                                        this.apply_settings_effect(SettingsEffect::ZoomChanged(next), window, cx);
                                    })),
                            ),
                        )
                        .child(
                            div()
                                .px_2()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(format!("{}%", display.ui_zoom)),
                        )
                        .child(
                            div().debug_selector(|| "settings-zoom-in".to_owned()).child(
                                Button::new("settings-zoom-in")
                                    .ghost()
                                    .xsmall()
                                    .label("+")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let next = display.ui_zoom.saturating_add(10).min(MAX_UI_ZOOM);
                                        this.apply_settings_effect(SettingsEffect::ZoomChanged(next), window, cx);
                                    })),
                            ),
                        )
                        .child(
                            div().debug_selector(|| "settings-zoom-reset".to_owned()).child(
                                Button::new("settings-zoom-reset")
                                    .ghost()
                                    .xsmall()
                                    .label(l10n::tr("Reset"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_settings_effect(SettingsEffect::ZoomChanged(100), window, cx);
                                    })),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .children(zoom_ticks),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(14.))
                .flex()
                .flex_row()
                .items_center()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .h(px(4.))
                        .rounded_full()
                        .bg(settings_border(theme)),
                )
                .child(div().h(px(4.)).rounded_full().bg(theme.brand).w(gpui_kit::relative(zoom_fraction)))
                .child(div().ml(px(-6.)).size(px(12.)).flex_none().rounded_full().bg(theme.foreground)),
        );

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(render_appearance_preview(view, theme))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Theme"), theme))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("You can pick a dark or light theme and customize colors in Themes.")),
                )
                .child(div().pt_1().flex().flex_row().flex_wrap().gap_3().children(theme_swatches)),
        )
        .child(settings_divider(theme))
        .child(zoom_controls)
        .child(settings_divider(theme))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(section_heading(l10n::tr("Messages"), theme))
                .child(font_slider)
                .child(div().flex().flex_col().gap_1p5().children(density_cards)),
        )
        .into_any_element()
}

fn render_themes_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_theme = theme::kind(cx);
    let display = view.prefs.data.display();
    let theme_swatches = THEME_OPTIONS.iter().enumerate().map(|(ix, (kind, label))| {
        let kind = *kind;
        render_theme_swatch(ix + 10, kind, label, active_theme == kind, theme, cx)
    });

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Base theme"), theme))
                .child(div().flex().flex_row().flex_wrap().gap_3().children(theme_swatches)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Colors and fonts"), theme))
                .child(info_card_row(
                    l10n::tr("Fluxer design tokens"),
                    format!(
                        "{}: {}px · {}: {}%",
                        l10n::tr("Chat font size"),
                        display.font_size,
                        l10n::tr("Interface scale"),
                        display.ui_zoom
                    ),
                    Some(active_theme.as_str().to_owned()),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Desktop window"), theme))
                .child(info_card_row(
                    l10n::tr("Native GPUI titlebar & window chrome"),
                    l10n::tr("Uses DirectComposition hardware-accelerated rendering without WebView."),
                    Some(l10n::tr("Active")),
                    theme,
                )),
        )
        .into_any_element()
}

fn render_notifications_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let notifications_row = div()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Desktop notifications")))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Show OS toast notifications for direct messages and @mentions.")),
                ),
        )
        .child(
            div().debug_selector(|| "settings-notifications".to_owned()).child(
                Switch::new("settings-notifications")
                    .checked(view.notifier.enabled)
                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                        this.apply_settings_effect(SettingsEffect::NotificationsChanged(*checked), window, cx);
                    })),
            ),
        );

    let sounds_row = div()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Message sounds")))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Play alert sounds when new messages or mentions arrive.")),
                ),
        )
        .child(
            div().debug_selector(|| "settings-sounds".to_owned()).child(
                Switch::new("settings-sounds")
                    .checked(view.notifier.sounds_enabled)
                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                        this.apply_settings_effect(SettingsEffect::SoundsChanged(*checked), window, cx);
                    })),
            ),
        );

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Notifications"), theme))
                .child(notifications_row)
                .child(sounds_row),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Unread & mention badges"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Unread channel & mention indicators")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Highlight unread channels in the sidebar and show red mention count pills.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-unread-badge".to_owned()).child(
                                Switch::new("settings-unread-badge")
                                    .checked(view.prefs.data.unread_badge_enabled)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::UnreadBadgeEnabledChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                ),
        )
        .into_any_element()
}

fn render_messages_media_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_spoiler = view.prefs.data.spoiler_display.as_str();
    let spoiler_options: &[(&str, &str)] = &[
        ("on_click", "On click"),
        ("moderator", "In channels I moderate"),
        ("always", "Always"),
    ];
    div()
        .debug_selector(|| "settings-chat-display".to_owned())
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Display & previews"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Inline image & GIF previews")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Display uploaded images, animated GIFs, and media attachments directly in chat.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-inline-media".to_owned()).child(
                                Switch::new("settings-inline-media")
                                    .checked(view.prefs.data.inline_media_previews)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::InlineMediaPreviewsChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                )
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Website link previews & embeds")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Show rich embed cards and YouTube thumbnails for links posted in chat.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-render-embeds".to_owned()).child(
                                Switch::new("settings-render-embeds")
                                    .checked(view.prefs.data.render_embeds)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::RenderEmbedsChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Spoilers & reactions"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Spoiler content")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr("Choose how hidden spoiler blocks are revealed in chat.")),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap_1()
                                .children(spoiler_options.iter().map(|&(val, label)| {
                                    let selected = active_spoiler == val;
                                    let sel_id = format!("settings-spoiler-mode-{val}");
                                    let val_owned = val.to_owned();
                                    let mut btn = Button::new(SharedString::from(format!("chat-{sel_id}")))
                                        .small()
                                        .label(l10n::tr(label));
                                    btn = if selected { btn.primary() } else { btn.ghost() };
                                    div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::SpoilerDisplayChanged(val_owned.clone()),
                                                window,
                                                cx,
                                            );
                                        },
                                    )))
                                })),
                        ),
                )
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Emoji reactions")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Display reaction pills under messages and allow quick reaction toggling.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-render-reactions".to_owned()).child(
                                Switch::new("settings-render-reactions")
                                    .checked(view.prefs.data.render_reactions)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::RenderReactionsChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Text input & autocomplete"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Convert emoticons & autocomplete suggestions")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Automatically convert :) emoticons to emoji and show @ / # / : popup suggestions.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-convert-emoticons".to_owned()).child(
                                Switch::new("settings-convert-emoticons")
                                    .checked(view.prefs.data.convert_emoticons)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::ConvertEmoticonsChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                ),
        )
        .into_any_element()
}

fn render_voice_tab(
    view: &ClientView,
    active_section: Option<&'static str>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let active_in_dev = view
        .prefs
        .data
        .voice_input_device
        .clone()
        .unwrap_or_else(|| l10n::tr("System default"));
    let ptt_enabled = view.prefs.data.voice_push_to_talk;
    let ptt_delay = view.prefs.data.voice_ptt_release_delay_ms;
    let vad_auto = view.prefs.data.voice_vad_auto_threshold;
    let vad_threshold = view.prefs.data.voice_vad_threshold;
    let voice_profile = view.prefs.data.voice_processing_mode.as_str();
    let ns_backend = view.prefs.data.voice_noise_suppression_backend.as_str();
    let echo_cancel = view.prefs.data.voice_echo_cancellation;
    let auto_gain = view.prefs.data.voice_auto_gain_control;
    let mic_testing = view.mic_test_active;
    let cam_res = view.prefs.data.camera_resolution.as_str();
    let mirror_cam = view.prefs.data.mirror_camera;
    let stream_mode = view.prefs.data.streaming_mode.as_str();
    let vid_res = view.prefs.data.video_resolution.as_str();
    let vid_fps = view.prefs.data.video_fps;
    let vid_codec = view.prefs.data.video_codec.as_str();
    let vid_bitrate = view.prefs.data.video_bitrate_kbps;
    let hw_accel = view.prefs.data.video_hardware_acceleration;

    let ptt_delays: &[(u16, &str)] = &[
        (20, "20ms"),
        (100, "100ms"),
        (250, "250ms"),
        (500, "500ms"),
        (1000, "1.0s"),
        (2000, "2.0s"),
    ];
    let profiles: &[(&str, &str)] = &[
        ("voice", "Focused Voice"),
        ("studio", "Direct Input"),
        ("custom", "Custom"),
    ];
    let ns_backends: &[(&str, &str)] = &[
        ("off", "Off"),
        ("standard", "Standard (WebRTC)"),
        ("deepfilternet", "DeepFilterNet"),
    ];
    let camera_resolutions: &[(&str, &str)] = &[("480p", "480p"), ("720p", "720p"), ("1080p", "1080p")];
    let stream_presets: &[(&str, &str)] = &[
        ("screenshare", "Screen Share (1080p · 30 FPS)"),
        ("gaming", "Gaming (1080p · 60 FPS)"),
        ("custom", "Custom"),
    ];
    let video_resolutions: &[(&str, &str)] = &[
        ("480p", "480p"),
        ("720p", "720p"),
        ("1080p", "1080p"),
        ("1440p", "1440p"),
        ("source", "Source"),
    ];
    let video_fps_options: &[(u8, &str)] = &[(15, "15 FPS"), (30, "30 FPS"), (60, "60 FPS")];
    let video_codecs: &[(&str, &str)] = &[
        ("auto", "Auto"),
        ("h265", "H.265 / HEVC"),
        ("h264", "H.264"),
        ("vp9", "VP9"),
        ("vp8", "VP8"),
        ("av1", "AV1"),
    ];
    let bitrate_options: &[(u16, &str)] = &[
        (0, "Automatic"),
        (2000, "2 Mbps"),
        (4000, "4 Mbps"),
        (6000, "6 Mbps"),
        (8000, "8 Mbps"),
        (10000, "10 Mbps"),
        (15000, "15 Mbps"),
    ];

    let meter_fill = if mic_testing {
        (f32::from(view.prefs.data.voice_input_volume.min(200)) / 250.).clamp(0.15, 0.92)
    } else {
        0.04
    };

    let devices_section = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(section_heading(l10n::tr("Voice & Audio"), theme))
                .child(
                    div().debug_selector(|| "voice-refresh-devices".to_owned()).child(
                        Button::new("voice-refresh-devices")
                            .ghost()
                            .small()
                            .label(l10n::tr("Refresh devices"))
                            .accessibility_label(l10n::tr("Refresh devices"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_audio_devices(cx);
                            })),
                    ),
                ),
        )
        .child(voice_device_row(
            "Input device",
            view.prefs.data.voice_input_device.as_deref(),
            &view.audio_devices.input_devices,
            "voice-input-device",
            "voice-input-device-reset",
            theme,
            SettingsEffect::VoiceInputDeviceChanged,
            cx,
        ))
        .child(voice_volume_row(
            "Input volume",
            view.prefs.data.voice_input_volume,
            "voice-input-vol-dec",
            "voice-input-vol-inc",
            "voice-input-vol-reset",
            SettingsEffect::VoiceInputVolumeChanged,
            cx,
        ))
        .child(voice_device_row(
            "Output device",
            view.prefs.data.voice_output_device.as_deref(),
            &view.audio_devices.output_devices,
            "voice-output-device",
            "voice-output-device-reset",
            theme,
            SettingsEffect::VoiceOutputDeviceChanged,
            cx,
        ))
        .child(voice_volume_row(
            "Output volume",
            view.prefs.data.voice_output_volume,
            "voice-output-vol-dec",
            "voice-output-vol-inc",
            "voice-output-vol-reset",
            SettingsEffect::VoiceOutputVolumeChanged,
            cx,
        ))
        .into_any_element();

    let input_mode_section = div()
        .debug_selector(|| "settings-voice-input-mode".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Input mode"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Voice transmission mode")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Choose automatic voice activity detection or manual push-to-talk.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap_1()
                                        .child(
                                            div().debug_selector(|| "settings-input-mode-vad".to_owned()).child({
                                                let vad = !ptt_enabled;
                                                let mut btn = Button::new("settings-input-mode-vad")
                                                    .small()
                                                    .label(l10n::tr("Voice Activity"));
                                                btn = if vad { btn.primary() } else { btn.ghost() };
                                                btn.on_click(cx.listener(|this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VoicePushToTalkChanged(false),
                                                        window,
                                                        cx,
                                                    );
                                                }))
                                            }),
                                        )
                                        .child(
                                            div().debug_selector(|| "settings-input-mode-ptt".to_owned()).child({
                                                let mut btn = Button::new("settings-input-mode-ptt")
                                                    .small()
                                                    .label(l10n::tr("Push to Talk"));
                                                btn = if ptt_enabled { btn.primary() } else { btn.ghost() };
                                                btn.on_click(cx.listener(|this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VoicePushToTalkChanged(true),
                                                        window,
                                                        cx,
                                                    );
                                                }))
                                            }),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Auto-set activity threshold")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Automatically calibrate microphone voice activation sensitivity.",
                                                )),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-vad-auto-threshold".to_owned()).child(
                                        Switch::new("settings-vad-auto-threshold")
                                            .checked(vad_auto)
                                            .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                this.apply_settings_effect(
                                                    SettingsEffect::VoiceVadAutoThresholdChanged(*checked),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(div().text_sm().child(l10n::tr("Activity threshold")))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .debug_selector(|| "settings-vad-threshold-dec".to_owned())
                                                .child(
                                                    Button::new("settings-vad-threshold-dec")
                                                        .ghost()
                                                        .small()
                                                        .disabled(vad_auto)
                                                        .label("-")
                                                        .on_click(cx.listener(move |this, _, window, cx| {
                                                            this.apply_settings_effect(
                                                                SettingsEffect::VoiceVadThresholdChanged(
                                                                    vad_threshold.saturating_sub(5),
                                                                ),
                                                                window,
                                                                cx,
                                                            );
                                                        })),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .px_2()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(format!("{vad_threshold}%")),
                                        )
                                        .child(
                                            div()
                                                .debug_selector(|| "settings-vad-threshold-inc".to_owned())
                                                .child(
                                                    Button::new("settings-vad-threshold-inc")
                                                        .ghost()
                                                        .small()
                                                        .disabled(vad_auto)
                                                        .label("+")
                                                        .on_click(cx.listener(move |this, _, window, cx| {
                                                            this.apply_settings_effect(
                                                                SettingsEffect::VoiceVadThresholdChanged(
                                                                    (vad_threshold + 5).min(100),
                                                                ),
                                                                window,
                                                                cx,
                                                            );
                                                        })),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .debug_selector(|| "settings-vad-threshold-reset".to_owned())
                                                .child(
                                                    Button::new("settings-vad-threshold-reset")
                                                        .ghost()
                                                        .small()
                                                        .disabled(vad_auto)
                                                        .label(l10n::tr("Reset"))
                                                        .on_click(cx.listener(|this, _, window, cx| {
                                                            this.apply_settings_effect(
                                                                SettingsEffect::VoiceVadThresholdChanged(50),
                                                                window,
                                                                cx,
                                                            );
                                                        })),
                                                ),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Push-to-talk release delay")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Keep transmitting briefly after releasing the push-to-talk key.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(ptt_delays.iter().map(|&(ms, label)| {
                                            let selected = ptt_delay == ms;
                                            let sel_id = format!("settings-ptt-delay-{ms}");
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(label);
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VoicePttReleaseDelayChanged(ms),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        ),
                )
                .into_any_element();

    let audio_processing_section = div()
        .debug_selector(|| "settings-voice-processing".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Audio processing"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Voice processing profile")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Focused Voice isolates speech; Direct Input passes studio audio untouched.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(profiles.iter().map(|&(val, label)| {
                                            let selected = voice_profile == val;
                                            let sel_id = format!("settings-voice-profile-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VoiceProcessingModeChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Noise suppression")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Remove background keyboard, fan, and room noise from your microphone.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .children(ns_backends.iter().map(|&(val, label)| {
                                            let selected = ns_backend == val;
                                            let sel_id = format!("settings-ns-backend-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VoiceNoiseSuppressionBackendChanged(
                                                            val_owned.clone(),
                                                        ),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        }))
                                        .child(
                                            div().debug_selector(|| "settings-noise-suppression".to_owned()).child(
                                                Switch::new("settings-noise-suppression")
                                                    .checked(view.prefs.data.voice_noise_suppression)
                                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                        this.apply_settings_effect(
                                                            SettingsEffect::VoiceNoiseSuppressionChanged(*checked),
                                                            window,
                                                            cx,
                                                        );
                                                    })),
                                            ),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Echo cancellation")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Prevent speaker output from feeding back into your microphone.",
                                                )),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-echo-cancellation".to_owned()).child(
                                        Switch::new("settings-echo-cancellation")
                                            .checked(echo_cancel)
                                            .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                this.apply_settings_effect(
                                                    SettingsEffect::VoiceEchoCancellationChanged(*checked),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Automatic gain control")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Automatically normalize microphone volume for consistent speech levels.",
                                                )),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-auto-gain-control".to_owned()).child(
                                        Switch::new("settings-auto-gain-control")
                                            .checked(auto_gain)
                                            .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                this.apply_settings_effect(
                                                    SettingsEffect::VoiceAutoGainControlChanged(*checked),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                                ),
                        ),
                )
                .into_any_element();

    let mic_test_section = div()
        .debug_selector(|| "settings-voice-mic-test".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Microphone test"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_2p5()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Input monitor")),
                                        )
                                        .child(
                                            div().text_xs().text_color(theme.muted_foreground).child(format!(
                                                "{}: {active_in_dev} ({}%)",
                                                l10n::tr("Active input"),
                                                view.prefs.data.voice_input_volume
                                            )),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-mic-test-btn".to_owned()).child({
                                        let mut btn = Button::new("settings-mic-test-btn").small().label(if mic_testing {
                                            l10n::tr("Stop Mic Test")
                                        } else {
                                            l10n::tr("Test Microphone")
                                        });
                                        btn = if mic_testing { btn.danger() } else { btn.primary() };
                                        btn.on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(SettingsEffect::ToggleMicTest, window, cx);
                                        }))
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(|| "settings-mic-test-meter".to_owned())
                                .w_full()
                                .h(px(8.))
                                .rounded_full()
                                .bg(theme.background)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .bg(if mic_testing {
                                            theme.status_online
                                        } else {
                                            theme.muted_foreground
                                        })
                                        .w(gpui_kit::relative(meter_fill)),
                                ),
                        ),
                )
                .into_any_element();

    let video_quality_section = div()
        .debug_selector(|| "settings-video-quality".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Video & screen share"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Camera quality")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Target capture resolution for your video camera.")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(camera_resolutions.iter().map(|&(val, label)| {
                                            let selected = cam_res == val;
                                            let sel_id = format!("settings-camera-res-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(label);
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::CameraResolutionChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Mirror camera")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Flip your local camera preview horizontally.")),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-mirror-camera".to_owned()).child(
                                        Switch::new("settings-mirror-camera")
                                            .checked(mirror_cam)
                                            .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                this.apply_settings_effect(
                                                    SettingsEffect::MirrorCameraChanged(*checked),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Screen share preset")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Optimize screen share for readable text or high-motion gaming.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(stream_presets.iter().map(|&(val, label)| {
                                            let selected = stream_mode == val;
                                            let sel_id = format!("settings-stream-mode-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::StreamingModeChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Screen share quality (resolution)")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Choose 480p, 720p, 1080p, 1440p, or native Source resolution.")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(video_resolutions.iter().map(|&(val, label)| {
                                            let selected = vid_res == val;
                                            let sel_id = format!("settings-video-res-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VideoResolutionChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Frame rate")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Target frame rate for screen sharing.")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(video_fps_options.iter().map(|&(fps, label)| {
                                            let selected = vid_fps == fps;
                                            let sel_id = format!("settings-video-fps-{fps}");
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(label);
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VideoFpsChanged(fps),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Video codec")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Preferred hardware or software encoder codec for screen sharing and video.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(video_codecs.iter().map(|&(val, label)| {
                                            let selected = vid_codec == val;
                                            let sel_id = format!("settings-video-codec-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(label);
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VideoCodecChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Screen share bitrate")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Higher bitrates keep fast motion sharper and use more upload bandwidth.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(bitrate_options.iter().map(|&(kbps, label)| {
                                            let selected = vid_bitrate == kbps;
                                            let sel_id = format!("settings-video-bitrate-{kbps}");
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VideoBitrateChanged(kbps),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Hardware acceleration")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Use GPU hardware encoders and decoders (NVENC / AMF / QuickSync / MediaFoundation) when available.",
                                                )),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-video-hw-accel".to_owned()).child(
                                        Switch::new("settings-video-hw-accel")
                                            .checked(hw_accel)
                                            .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                this.apply_settings_effect(
                                                    SettingsEffect::VideoHardwareAccelerationChanged(*checked),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                                ),
                        ),
                )
                .into_any_element();

    let mut sections: Vec<(&'static str, AnyElement)> = vec![
        ("voice-devices", devices_section),
        ("input-mode", input_mode_section),
        ("audio-processing", audio_processing_section),
        ("mic-test", mic_test_section),
        ("video-quality", video_quality_section),
    ];
    if let Some(active_id) = active_section
        && let Some(pos) = sections.iter().position(|(id, _)| *id == active_id)
        && pos > 0
    {
        let item = sections.remove(pos);
        sections.insert(0, item);
    }

    div()
        .flex()
        .flex_col()
        .gap_5()
        .children(sections.into_iter().map(|(_, el)| el))
        .into_any_element()
}

fn render_accessibility_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let display = view.prefs.data.display();
    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(section_heading(l10n::tr("Motion"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Reduced motion")))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr("Pause animated GIF thumbnails and reduce interface transitions.")),
                                ),
                        )
                        .child(
                            div().flex().flex_row().gap_1().children(
                                [
                                    (ReducedMotionPref::System, "System", "reduced-motion-system"),
                                    (ReducedMotionPref::On, "On", "reduced-motion-on"),
                                    (ReducedMotionPref::Off, "Off", "reduced-motion-off"),
                                ]
                                .into_iter()
                                .map(|(pref, label, sel_id)| {
                                    let selected = view.prefs.data.reduced_motion == pref;
                                    let mut btn = Button::new(sel_id).small().label(l10n::tr(label));
                                    btn = if selected { btn.primary() } else { btn.ghost() };
                                    div().debug_selector(move || sel_id.to_owned()).child(btn.on_click(
                                        cx.listener(move |this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::ReducedMotionChanged(pref),
                                                window,
                                                cx,
                                            );
                                        }),
                                    ))
                                }),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Visual & contrast"), theme))
                .child(info_card_row(
                    l10n::tr("Text scaling & interface zoom"),
                    format!("{}px · {}%", display.font_size, display.ui_zoom),
                    Some(l10n::tr("Synced")),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Keyboard & screen reader"), theme))
                .child(info_card_row(
                    l10n::tr("AccessKit assistive tree"),
                    l10n::tr("Exposes semantic roles, labels, and keyboard focus navigation to Windows Narrator and NVDA."),
                    Some(l10n::tr("Enabled")),
                    theme,
                )),
        )
        .into_any_element()
}

fn render_language_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let active_locale = l10n::current().locale();
    let use_24h = view.prefs.data.time_format_24h;
    let locale_list = div()
        .id("settings-locale-list")
        .debug_selector(|| "settings-locale-list".to_owned())
        .max_h(px(480.))
        .overflow_y_scroll()
        .track_scroll(&modal.locale_scroll)
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .p_2()
        .flex()
        .flex_col()
        .gap(px(2.))
        .children(LOCALES.iter().enumerate().map(|(ix, opt)| {
            let selected = active_locale == opt.code;
            let code = opt.code;
            div()
                .id(("settings-locale", ix))
                .debug_selector(|| format!("settings-locale-{code}"))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_3()
                .py(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(selected, |d| d.bg(theme.accent))
                .hover(|s| s.bg(theme.muted))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.apply_settings_effect(SettingsEffect::LocaleChanged(code.to_owned()), window, cx);
                }))
                .child(div().text_sm().child(opt.native_name))
                .child(div().text_xs().text_color(theme.muted_foreground).child(opt.code))
        }));

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(section_heading(l10n::tr("Language"), theme))
                .child(locale_list),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Time & date format"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Regional timestamp formatting")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Choose 12-hour (AM/PM) or 24-hour clock format for message timestamps.",
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_1()
                                .child(
                                    div().debug_selector(|| "settings-time-format-12h".to_owned()).child({
                                        let mut btn = Button::new("settings-time-format-12h")
                                            .small()
                                            .label(l10n::tr("12-hour"));
                                        btn = if !use_24h { btn.primary() } else { btn.ghost() };
                                        btn.on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::TimeFormat24hChanged(false),
                                                window,
                                                cx,
                                            );
                                        }))
                                    }),
                                )
                                .child(
                                    div().debug_selector(|| "settings-time-format-24h".to_owned()).child({
                                        let mut btn = Button::new("settings-time-format-24h")
                                            .small()
                                            .label(l10n::tr("24-hour"));
                                        btn = if use_24h { btn.primary() } else { btn.ghost() };
                                        btn.on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::TimeFormat24hChanged(true),
                                                window,
                                                cx,
                                            );
                                        }))
                                    }),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

fn render_keybind_chip_list(
    chips: &[String],
    overridden: bool,
    disabled: bool,
    theme: &Theme,
) -> AnyElement {
    let dimmed = overridden || disabled;
    let chip_bg = theme.background;
    let chip_border = theme.background_modifier_accent;

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .flex_none()
        .flx("user.keybinds-tab.default-shortcut-chip-list.default-chips")
        .when(chips.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .italic()
                    .flx("user.keybinds-tab.default-shortcut-chip-list.default-chips-empty")
                    .child(l10n::tr("Unassigned")),
            )
        })
        .when(!chips.is_empty(), |d| {
            d.children(chips.iter().map(|chip| {
                div()
                    .min_w(px(24.))
                    .h(px(22.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(chip_border)
                    .bg(chip_bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .when(dimmed, |c| c.opacity(0.5).line_through())
                    .flx("user.keybinds-tab.default-shortcut-chip-list.default-chip")
                    .child(chip.clone())
            }))
        })
        .when(overridden, |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .italic()
                    .flx("user.keybinds-tab.default-shortcut-chip-list.default-chips-empty--2")
                    .child(l10n::tr("Overridden")),
            )
        })
        .when(!overridden && disabled, |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .italic()
                    .flx("user.keybinds-tab.default-shortcut-chip-list.default-chips-empty--3")
                    .child(l10n::tr("Disabled")),
            )
        })
        .into_any_element()
}

fn render_custom_keybind_row(
    entry: &CustomKeybindEntry,
    conflict_label: Option<&str>,
    is_recording: bool,
    action_menu_open: bool,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let id = entry.id.clone();
    let id_sel = id.clone();
    let id_action_btn = id.clone();
    let id_record_btn = id.clone();
    let id_clear_btn = id.clone();
    let id_switch = id.clone();
    let id_delete = id.clone();

    let action_display = entry
        .action
        .as_deref()
        .and_then(shortcuts::action_label)
        .or(entry.action.as_deref())
        .map(l10n::tr)
        .unwrap_or_else(|| l10n::tr("Select an action…"));

    let combo_display = if is_recording {
        l10n::tr("Press a keybind… (Esc to cancel)")
    } else if entry.combo.is_empty() {
        l10n::tr("No keybind set")
    } else {
        shortcuts::format_combo_display(&entry.combo)
    };

    let has_combo = !entry.combo.is_empty();
    let border_col = theme.background_modifier_accent;
    let recorder_border = if is_recording { theme.brand } else { border_col };

    let action_dropdown_menu = action_menu_open.then(|| {
        let id_unassigned = id.clone();
        div()
            .id(SharedString::from(format!("kb-action-menu-{id}")))
            .mt(px(4.))
            .p(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(border_col)
            .bg(theme.background)
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(200.))
            .overflow_y_scroll()
            .debug_selector({
                let id = id.clone();
                move || format!("settings-keybind-action-menu-{id}")
            })
            .child(
                div()
                    .id(SharedString::from(format!("kb-action-opt-{id_unassigned}-none")))
                    .debug_selector({
                        let id = id_unassigned.clone();
                        move || format!("settings-keybind-action-option-{id}-unassigned")
                    })
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.hover_bg))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_set_custom_keybind_action(&id_unassigned, None, cx);
                    }))
                    .child(l10n::tr("(Unassigned)")),
            )
            .children(shortcuts::ASSIGNABLE_ACTIONS.iter().map(|&(act_id, act_label)| {
                let id_opt = id.clone();
                let selected = entry.action.as_deref() == Some(act_id);
                div()
                    .id(SharedString::from(format!("kb-action-opt-{id_opt}-{act_id}")))
                    .debug_selector({
                        let id = id_opt.clone();
                        move || format!("settings-keybind-action-option-{id}-{act_id}")
                    })
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .text_size(px(13.))
                    .text_color(theme.foreground)
                    .when(selected, |d| d.bg(theme.accent).font_weight(FontWeight::MEDIUM))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.hover_bg)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_set_custom_keybind_action(&id_opt, Some(act_id.to_owned()), cx);
                    }))
                    .child(l10n::tr(act_label))
                    .when(selected, |d| {
                        d.child(Icon::new(AppIcon::Check).size(px(14.)).text_color(theme.foreground))
                    })
            }))
    });

    div()
        .debug_selector(move || format!("settings-keybind-custom-row-{id_sel}"))
        .flex()
        .flex_col()
        .gap(px(8.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(border_col)
        .bg(settings_card_bg(theme))
        .when(!entry.enabled, |d| d.opacity(0.62))
        .flx("user.keybinds-tab.custom-keybind-row.custom-row")
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap(px(12.))
                .flx("user.keybinds-tab.custom-keybind-row.custom-row-main")
                .child(
                    div()
                        .flex_1()
                        .min_w(px(150.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flx("user.keybinds-tab.custom-keybind-row.custom-field")
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .flx("user.keybinds-tab.custom-keybind-row.custom-field-label")
                                .child(l10n::tr("Action")),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("kb-action-btn-{id_action_btn}")))
                                .debug_selector({
                                    let id = id_action_btn.clone();
                                    move || format!("settings-keybind-action-{id}")
                                })
                                .h(px(34.))
                                .px(px(10.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(border_col)
                                .bg(settings_sidebar_bg(theme))
                                .cursor_pointer()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .text_size(px(13.))
                                .text_color(if entry.action.is_some() {
                                    theme.foreground
                                } else {
                                    theme.muted_foreground
                                })
                                .hover(|s| s.bg(theme.hover_bg))
                                .flx("user.keybinds-tab.custom-keybind-row.action-dropdown")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.settings_toggle_keybind_action_menu(id_action_btn.clone(), cx);
                                }))
                                .child(div().truncate().child(action_display))
                                .child(
                                    Icon::new(AppIcon::CaretDown)
                                        .size(px(14.))
                                        .text_color(theme.muted_foreground),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(180.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flx("user.keybinds-tab.custom-keybind-row.keybind-recorder.combo")
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .flx("input.keybind-recorder.field-label")
                                .child(l10n::tr("Keybind")),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .id(SharedString::from(format!("kb-record-btn-{id_record_btn}")))
                                        .debug_selector({
                                            let id = id_record_btn.clone();
                                            move || format!("settings-keybind-record-{id}")
                                        })
                                        .flex_1()
                                        .min_w_0()
                                        .h(px(34.))
                                        .px(px(10.))
                                        .rounded(px(6.))
                                        .border_1()
                                        .border_color(recorder_border)
                                        .bg(settings_sidebar_bg(theme))
                                        .cursor_pointer()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.))
                                        .text_size(px(13.))
                                        .text_color(if is_recording {
                                            theme.brand
                                        } else if has_combo {
                                            theme.foreground
                                        } else {
                                            theme.muted_foreground
                                        })
                                        .hover(|s| s.bg(theme.hover_bg))
                                        .flx("input.keybind-recorder.recorder.button")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.settings_toggle_record_keybind(id_record_btn.clone(), window, cx);
                                        }))
                                        .child(
                                            Icon::new(if is_recording {
                                                AppIcon::Keyboard
                                            } else {
                                                AppIcon::PencilSimple
                                            })
                                            .size(px(14.))
                                            .text_color(if is_recording {
                                                theme.brand
                                            } else {
                                                theme.muted_foreground
                                            }),
                                        )
                                        .child(div().truncate().child(combo_display)),
                                )
                                .when(has_combo && !is_recording, |d| {
                                    d.child(
                                        div()
                                            .id(SharedString::from(format!("kb-clear-btn-{id_clear_btn}")))
                                            .debug_selector({
                                                let id = id_clear_btn.clone();
                                                move || format!("settings-keybind-clear-{id}")
                                            })
                                            .size(px(28.))
                                            .flex_none()
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .hover(|s| s.bg(theme.hover_bg))
                                            .flx("input.keybind-recorder.keybind-editor-popout.button.clear")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.settings_set_custom_keybind_combo(
                                                    &id_clear_btn,
                                                    String::new(),
                                                    cx,
                                                );
                                            }))
                                            .child(
                                                Icon::new(AppIcon::X)
                                                    .size(px(14.))
                                                    .text_color(theme.muted_foreground),
                                            ),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .h(px(34.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .flx("user.keybinds-tab.custom-keybind-row.custom-controls")
                        .child(
                            div()
                                .debug_selector({
                                    let id = id_switch.clone();
                                    move || format!("settings-keybind-toggle-{id}")
                                })
                                .flx("user.keybinds-tab.custom-keybind-row.switch.handle-enabled")
                                .child(
                                    Switch::new(SharedString::from(format!("kb-switch-{id_switch}")))
                                        .checked(entry.enabled)
                                        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                            this.settings_set_custom_keybind_enabled(&id_switch, *checked, cx);
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("kb-delete-btn-{id_delete}")))
                                .debug_selector({
                                    let id = id_delete.clone();
                                    move || format!("settings-keybind-delete-{id}")
                                })
                                .size(px(28.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(theme.hover_bg))
                                .flx("user.keybinds-tab.custom-keybind-row.dots-button.open-row-menu")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.settings_remove_custom_keybind(&id_delete, cx);
                                }))
                                .child(Icon::new(AppIcon::Trash).size(px(16.)).text_color(theme.danger)),
                        ),
                ),
        )
        .children(action_dropdown_menu)
        .when(entry.action.is_none(), |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .pl(px(2.))
                    .flx("user.keybinds-tab.custom-keybind-row.custom-hint")
                    .child(l10n::tr("Choose an action.")),
            )
        })
        .when_some(conflict_label, |d, other| {
            let id_conflict = entry.id.clone();
            d.child(
                div()
                    .debug_selector(move || format!("settings-keybind-conflict-{id_conflict}"))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .mt(px(2.))
                    .text_size(px(12.))
                    .text_color(theme.status_idle)
                    .flx("user.keybinds-tab.custom-keybind-row.conflict-warning")
                    .child(
                        Icon::new(AppIcon::WarningCircle)
                            .size(px(14.))
                            .text_color(theme.status_idle),
                    )
                    .child(format!("{} {}", l10n::tr("Conflicts with"), other)),
            )
        })
        .into_any_element()
}

fn render_default_shortcut_row(
    row: &DefaultShortcutRowSpec,
    is_last: bool,
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let row_key = row.primary_action();
    let menu_open = modal.keybinds_open_default_menu.as_deref() == Some(row_key);
    let specs = row.specs();
    let has_custom = specs.iter().any(|s| {
        view.prefs
            .data
            .custom_keybinds
            .iter()
            .any(|e| e.action.as_deref() == Some(s.action))
    });
    let any_disabled = specs
        .iter()
        .any(|s| view.prefs.is_builtin_action_disabled(s.action));
    let all_disabled = specs
        .iter()
        .all(|s| view.prefs.is_builtin_action_disabled(s.action));

    let border_col = theme.background_modifier_accent;
    let is_pair = matches!(row, DefaultShortcutRowSpec::Pair(_, _));
    let row_flx = if is_pair {
        "user.keybinds-tab.default-shortcut-row.default-row"
    } else {
        "user.keybinds-tab.default-shortcut-row.default-row--2"
    };
    let label_flx = if is_pair {
        "user.keybinds-tab.default-shortcut-row.default-label"
    } else {
        "user.keybinds-tab.default-shortcut-row.default-label--2"
    };
    let actions_flx = if is_pair {
        "user.keybinds-tab.default-shortcut-row.default-row-actions"
    } else {
        "user.keybinds-tab.default-shortcut-row.default-row-actions--2"
    };
    let dots_flx = if is_pair {
        "user.keybinds-tab.default-shortcut-row.dots-button.open-row-menu"
    } else {
        "user.keybinds-tab.default-shortcut-row.dots-button.open-row-menu--2"
    };

    let chips_el: AnyElement = match row {
        DefaultShortcutRowSpec::Single(s) => {
            let overridden = shortcuts::is_builtin_action_overridden(&view.prefs.data, s.action);
            let disabled = view.prefs.data.disable_builtin_keybinds
                || view.prefs.is_builtin_action_disabled(s.action);
            render_keybind_chip_list(&shortcuts::chips_for_default_spec(s), overridden, disabled, theme)
        }
        DefaultShortcutRowSpec::Pair(a, b) => {
            let a_over = shortcuts::is_builtin_action_overridden(&view.prefs.data, a.action);
            let a_dis = view.prefs.data.disable_builtin_keybinds
                || view.prefs.is_builtin_action_disabled(a.action);
            let b_over = shortcuts::is_builtin_action_overridden(&view.prefs.data, b.action);
            let b_dis = view.prefs.data.disable_builtin_keybinds
                || view.prefs.is_builtin_action_disabled(b.action);
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .flex_none()
                .flx("user.keybinds-tab.default-shortcut-row.default-chips-merged")
                .child(render_keybind_chip_list(
                    &shortcuts::chips_for_default_spec(a),
                    a_over,
                    a_dis,
                    theme,
                ))
                .child(render_keybind_chip_list(
                    &shortcuts::chips_for_default_spec(b),
                    b_over,
                    b_dis,
                    theme,
                ))
                .into_any_element()
        }
    };

    let row_menu = menu_open.then(|| {
        let actions_add: Vec<&'static str> = specs.iter().map(|s| s.action).collect();
        let actions_disable: Vec<&'static str> = specs.iter().map(|s| s.action).collect();
        let actions_enable: Vec<&'static str> = specs.iter().map(|s| s.action).collect();
        let actions_reset: Vec<&'static str> = specs.iter().map(|s| s.action).collect();

        div()
            .px(px(12.))
            .pb(px(10.))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .debug_selector(move || format!("settings-keybind-default-actions-{row_key}"))
            .child(
                div()
                    .debug_selector(move || format!("settings-keybind-default-add-{row_key}"))
                    .child(
                        Button::new(SharedString::from(format!("kb-def-add-{row_key}")))
                            .ghost()
                            .small()
                            .icon(AppIcon::PencilSimple)
                            .label(if has_custom {
                                l10n::tr("Add another custom keybind")
                            } else {
                                l10n::tr("Set custom keybind")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                for act in &actions_add {
                                    this.settings_add_custom_keybind(Some((*act).to_owned()), cx);
                                }
                            })),
                    ),
            )
            .when(has_custom, |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("settings-keybind-default-reset-{row_key}"))
                        .child(
                            Button::new(SharedString::from(format!("kb-def-reset-{row_key}")))
                                .danger()
                                .small()
                                .icon(AppIcon::ArrowClockwise)
                                .label(l10n::tr("Reset to built-in"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    for act in &actions_reset {
                                        this.settings_remove_custom_keybinds_for_action(act, cx);
                                    }
                                })),
                        ),
                )
            })
            .when(any_disabled, |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("settings-keybind-default-enable-{row_key}"))
                        .child(
                            Button::new(SharedString::from(format!("kb-def-enable-{row_key}")))
                                .ghost()
                                .small()
                                .icon(AppIcon::ArrowClockwise)
                                .label(l10n::tr("Re-enable built-in keybind"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    for act in &actions_enable {
                                        this.settings_enable_builtin_action(act, cx);
                                    }
                                })),
                        ),
                )
            })
            .when(!has_custom && !all_disabled, |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("settings-keybind-default-disable-{row_key}"))
                        .child(
                            Button::new(SharedString::from(format!("kb-def-disable-{row_key}")))
                                .ghost()
                                .small()
                                .icon(AppIcon::EyeSlash)
                                .label(l10n::tr("Disable built-in keybind"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    for act in &actions_disable {
                                        this.settings_disable_builtin_action(act, cx);
                                    }
                                })),
                        ),
                )
            })
    });

    div()
        .flex()
        .flex_col()
        .when(!is_last, |d| d.border_b_1().border_color(border_col))
        .when(is_last, |d| d.border_b_0())
        .flx(row_flx)
        .child(
            div()
                .min_h(px(44.))
                .p(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .text_color(theme.foreground)
                        .flx(label_flx)
                        .child(l10n::tr(row.label())),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .flex_none()
                        .flx(actions_flx)
                        .child(chips_el)
                        .child(
                            div()
                                .id(SharedString::from(format!("kb-def-menu-{row_key}")))
                                .debug_selector(move || format!("settings-keybind-default-menu-{row_key}"))
                                .size(px(28.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(menu_open, |d| d.bg(theme.hover_bg))
                                .hover(|s| s.bg(theme.hover_bg))
                                .flx(dots_flx)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.settings_toggle_keybind_default_menu(row_key.to_owned(), cx);
                                }))
                                .child(
                                    Icon::new(AppIcon::DotsThreeBold)
                                        .size(px(16.))
                                        .text_color(theme.muted_foreground),
                                ),
                        ),
                ),
        )
        .children(row_menu)
        .into_any_element()
}

fn render_keybinds_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let raw_query = modal.keybinds_search.read(cx).value().trim().to_owned();
    let query = raw_query.to_lowercase();
    let border_col = theme.background_modifier_accent;

    let custom_keybinds = &view.prefs.data.custom_keybinds;
    let conflicts = shortcuts::find_custom_conflicts(custom_keybinds);
    let filtered_custom: Vec<&CustomKeybindEntry> = custom_keybinds
        .iter()
        .filter(|entry| {
            if query.is_empty() {
                return true;
            }
            let label = entry
                .action
                .as_deref()
                .and_then(shortcuts::action_label)
                .or(entry.action.as_deref())
                .unwrap_or_default()
                .to_lowercase();
            if label.contains(&query) {
                return true;
            }
            if entry.combo.to_lowercase().contains(&query) {
                return true;
            }
            shortcuts::format_combo_display(&entry.combo)
                .to_lowercase()
                .contains(&query)
        })
        .collect();

    let has_any_overrides = !custom_keybinds.is_empty()
        || view.prefs.data.disable_builtin_keybinds
        || !view.prefs.data.disabled_builtin_actions.is_empty();

    let search_row = div()
        .relative()
        .w_full()
        .flex()
        .flex_col()
        .flx("user.keybinds-tab.search-row")
        .child(
            div()
                .w_full()
                .h(px(44.))
                .pl(px(36.))
                .pr(px(16.))
                .rounded(px(6.))
                .border_1()
                .border_color(border_col)
                .bg(settings_sidebar_bg(theme))
                .flex()
                .flex_row()
                .items_center()
                .debug_selector(|| "settings-keybinds-search".to_owned())
                .flx("user.keybinds-tab.input.set-search-query")
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&modal.keybinds_search).appearance(false)),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px(12.))
                .top(px(14.))
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .flx("user.keybinds-tab.magnifying-glass-icon")
                .child(
                    Icon::new(AppIcon::MagnifyingGlass)
                        .size(px(16.))
                        .text_color(theme.muted_foreground),
                ),
        );

    let custom_header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .flx("user.keybinds-tab.custom-keybinds-list.custom-header")
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w_0()
                .flx("user.keybinds-tab.custom-keybinds-list.custom-header-text")
                .child(
                    div()
                        .text_size(px(15.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .flx("user.keybinds-tab.custom-keybinds-list.custom-title")
                        .child(l10n::tr("Custom keybinds")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(theme.muted_foreground)
                        .flx("user.keybinds-tab.custom-keybinds-list.custom-subtitle")
                        .child(l10n::tr("Keybinds are paused while recording.")),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .flex_none()
                .flx("user.keybinds-tab.custom-keybinds-list.custom-header-actions")
                .when(has_any_overrides, |d| {
                    d.child(
                        div()
                            .h(px(34.))
                            .flex()
                            .items_center()
                            .debug_selector(|| "settings-keybinds-reset-all".to_owned())
                            .flx("user.keybinds-tab.custom-keybinds-list.button.reset-all")
                            .child(
                                Button::new("settings-keybinds-reset-all")
                                    .ghost()
                                    .small()
                                    .h(px(34.))
                                    .icon(AppIcon::ArrowClockwise)
                                    .label(l10n::tr("Reset to defaults"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_set_keybinds_confirm_reset(true, cx);
                                    })),
                            ),
                    )
                })
                .child(
                    div()
                        .h(px(34.))
                        .flex()
                        .items_center()
                        .debug_selector(|| "settings-keybinds-add".to_owned())
                        .flx("user.keybinds-tab.custom-keybinds-list.button.add")
                        .child(
                            Button::new("settings-keybinds-add")
                                .primary()
                                .small()
                                .h(px(34.))
                                .icon(AppIcon::Plus)
                                .label(l10n::tr("Add keybind"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings_add_custom_keybind(None, cx);
                                })),
                        ),
                ),
        );

    let reset_confirm_banner = modal.keybinds_confirm_reset.then(|| {
        div()
            .debug_selector(|| "settings-keybinds-reset-confirm".to_owned())
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.danger)
            .bg(settings_card_bg(theme))
            .flex()
            .flex_col()
            .gap(px(8.))
            .flx("user.keybinds-tab.handle-reset-all.confirm-modal")
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(l10n::tr("Reset all keybinds?")),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr(
                        "This removes every custom keybind and re-enables all built-in keybinds. This cannot be undone.",
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .debug_selector(|| "settings-keybinds-reset-cancel".to_owned())
                            .child(
                                Button::new("settings-keybinds-reset-cancel")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_set_keybinds_confirm_reset(false, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "settings-keybinds-reset-confirm-btn".to_owned())
                            .child(
                                Button::new("settings-keybinds-reset-confirm-btn")
                                    .danger()
                                    .small()
                                    .label(l10n::tr("Reset to defaults"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_reset_all_keybinds(cx);
                                    })),
                            ),
                    ),
            )
    });

    let shortcut_options = div()
        .px(px(12.))
        .py(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(border_col)
        .bg(settings_card_bg(theme))
        .flex()
        .flex_col()
        .gap(px(8.))
        .flx("user.keybinds-tab.custom-keybinds-list.shortcut-options")
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .debug_selector(|| "settings-keybinds-disable-builtins".to_owned())
                .flx("user.keybinds-tab.custom-keybinds-list.switch.set-disable-builtin-keybinds")
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(l10n::tr("Disable built-ins")),
                )
                .child(
                    Switch::new("settings-keybinds-disable-builtins")
                        .checked(view.prefs.data.disable_builtin_keybinds)
                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                            this.settings_set_disable_builtin_keybinds(*checked, cx);
                        })),
                ),
        );

    let custom_list = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .flx("user.keybinds-tab.custom-keybinds-list.custom-list")
        .when(custom_keybinds.is_empty(), |d| {
            d.child(
                div()
                    .py(px(8.))
                    .text_center()
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .debug_selector(|| "settings-keybinds-custom-empty".to_owned())
                    .flx("user.keybinds-tab.custom-keybinds-list.empty-state")
                    .child(l10n::tr("No custom keybinds yet.")),
            )
        })
        .when(!custom_keybinds.is_empty() && filtered_custom.is_empty(), |d| {
            d.child(
                div()
                    .py(px(8.))
                    .text_center()
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .debug_selector(|| "settings-keybinds-custom-no-match".to_owned())
                    .flx("user.keybinds-tab.custom-keybinds-list.empty-state--2")
                    .child(l10n::tr("No custom keybinds match that search.")),
            )
        })
        .when(!filtered_custom.is_empty(), |d| {
            d.children(filtered_custom.into_iter().map(|entry| {
                let conflict = conflicts.get(&entry.id).map(String::as_str);
                let is_rec = modal.keybinds_recording_id.as_deref() == Some(entry.id.as_str());
                let menu_open = modal.keybinds_open_action_menu.as_deref() == Some(entry.id.as_str());
                render_custom_keybind_row(entry, conflict, is_rec, menu_open, theme, cx)
            }))
        });

    let custom_section = div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .flx("user.keybinds-tab.custom-keybinds-list.custom-section")
        .child(custom_header)
        .children(reset_confirm_banner)
        .child(shortcut_options)
        .child(custom_list);

    let filtered_sections: Vec<(KeybindSection, Vec<DefaultShortcutRowSpec>)> = KeybindSection::ALL
        .iter()
        .filter_map(|&section| {
            let rows: Vec<DefaultShortcutRowSpec> = shortcuts::default_shortcut_rows(section)
                .into_iter()
                .filter(|row| {
                    if query.is_empty() {
                        return true;
                    }
                    if row.label().to_lowercase().contains(&query) {
                        return true;
                    }
                    row.specs().iter().any(|s| {
                        s.combo.to_lowercase().contains(&query)
                            || shortcuts::chips_for_default_spec(s)
                                .iter()
                                .any(|c| c.to_lowercase().contains(&query))
                    })
                })
                .collect();
            (!rows.is_empty()).then_some((section, rows))
        })
        .collect();

    let defaults_list = if filtered_sections.is_empty() && !query.is_empty() {
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .flx("user.keybinds-tab.default-keybinds-list.defaults-list")
            .child(
                div()
                    .py(px(8.))
                    .text_center()
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .debug_selector(|| "settings-keybinds-defaults-no-match".to_owned())
                    .flx("user.keybinds-tab.default-keybinds-list.empty-state")
                    .child(l10n::tr("No keybinds match that search.")),
            )
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .mb(px(8.))
            .flx("user.keybinds-tab.default-keybinds-list.defaults-list--2")
            .children(filtered_sections.into_iter().map(|(section, rows)| {
                let count = rows.len();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .flx("user.keybinds-tab.default-keybinds-list.defaults-section")
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .flx("user.keybinds-tab.default-keybinds-list.defaults-section-title")
                            .child(l10n::tr(section.label())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(border_col)
                            .bg(theme.background_secondary_lighter)
                            .overflow_hidden()
                            .flx("user.keybinds-tab.default-keybinds-list.defaults-section-rows")
                            .children(rows.iter().enumerate().map(|(idx, row)| {
                                render_default_shortcut_row(row, idx + 1 == count, view, modal, theme, cx)
                            })),
                    )
            }))
    };

    div()
        .debug_selector(|| "settings-keybinds-list".to_owned())
        .flex()
        .flex_col()
        .gap(px(16.))
        .flx("user.keybinds-tab.container")
        .child(search_row)
        .child(custom_section)
        .child(defaults_list)
        .into_any_element()
}

fn render_windows_app_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let autostart_row = div()
        .debug_selector(|| "settings-windows-autostart".to_owned())
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(settings_card_bg(theme))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Desktop startup")))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Launch Fluxer automatically when you sign in to Windows.")),
                ),
        )
        .child(
            Switch::new("settings-autostart")
                .checked(view.prefs.data.autostart)
                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                    this.apply_settings_effect(SettingsEffect::AutostartChanged(*checked), window, cx);
                })),
        );

    let close_to_tray_row = div()
        .debug_selector(|| "settings-windows-close-to-tray".to_owned())
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(settings_card_bg(theme))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Minimize to tray")))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Keep Fluxer running in the system tray when closing the window.")),
                ),
        )
        .child(
            Switch::new("settings-close-to-tray")
                .checked(view.prefs.data.close_to_tray)
                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                    this.apply_settings_effect(SettingsEffect::CloseToTrayChanged(*checked), window, cx);
                })),
        );

    let desktop_window_section = settings_section_block(
        l10n::tr("Desktop window"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(autostart_row)
            .child(close_to_tray_row)
            .into_any_element(),
        theme,
    );

    let tray_icon_section = settings_section_block(
        l10n::tr("Tray icon color"),
        None,
        info_card_row(
            l10n::tr("Match the taskbar"),
            l10n::tr("Pick white or black from the color of your taskbar or panel."),
            Some(l10n::tr("Auto")),
            theme,
        ),
        theme,
    );

    div()
        .debug_selector(|| "settings-windows-app-tab".to_owned())
        .flex()
        .flex_col()
        .gap_5()
        .child(desktop_window_section)
        .child(settings_divider(theme))
        .child(tray_icon_section)
        .into_any_element()
}

fn render_advanced_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let updates_section = settings_section_block(
        l10n::tr("About & Updates"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .bg(settings_card_bg(theme))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(format!("v{CURRENT_VERSION}")))
                    .child(
                        div().debug_selector(|| "settings-check-updates".to_owned()).child(
                            Button::new("settings-check-updates")
                                .ghost()
                                .small()
                                .label(l10n::tr("Check for updates"))
                                .accessibility_label(l10n::tr("Check for updates"))
                                .disabled(matches!(
                                    view.update_status,
                                    UpdateStatus::Checking | UpdateStatus::Downloading { .. }
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.check_for_updates(cx);
                                })),
                        ),
                    ),
            )
            .map(|d| match &view.update_status {
                UpdateStatus::Idle => d,
                UpdateStatus::Checking => d.child(
                    div()
                        .debug_selector(|| "settings-update-checking".to_owned())
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Checking for updates…")),
                ),
                UpdateStatus::UpToDate { .. } => d.child(
                    div()
                        .debug_selector(|| "settings-update-uptodate".to_owned())
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Up to date")),
                ),
                UpdateStatus::Available {
                    latest,
                    release_url,
                    asset_name,
                    asset_url,
                    ..
                } => {
                    let release_url = release_url.clone();
                    let has_asset = asset_name.is_some() && asset_url.is_some();
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(|| "settings-update-available".to_owned())
                                    .text_xs()
                                    .text_color(theme.foreground)
                                    .child(format!("{}: v{latest}", l10n::tr("Update available"))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .when(has_asset, |row| {
                                        row.child(
                                            div()
                                                .debug_selector(|| "settings-download-update".to_owned())
                                                .child(
                                                    Button::new("settings-download-update")
                                                        .primary()
                                                        .small()
                                                        .label(l10n::tr("Download update"))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.download_available_update(cx);
                                                        })),
                                                ),
                                        )
                                    })
                                    .child(
                                        div().debug_selector(|| "settings-open-release".to_owned()).child(
                                            Button::new("settings-open-release")
                                                .ghost()
                                                .small()
                                                .label(l10n::tr("Open release page"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    #[cfg(test)]
                                                    this.launch_external_url(&release_url, cx);
                                                    #[cfg(not(test))]
                                                    {
                                                        let _ = this;
                                                        cx.open_url(&release_url);
                                                    }
                                                })),
                                        ),
                                    ),
                            ),
                    )
                }
                UpdateStatus::Downloading { latest } => d.child(
                    div()
                        .debug_selector(|| "settings-update-downloading".to_owned())
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{} v{latest}…", l10n::tr("Downloading"))),
                ),
                UpdateStatus::Downloaded {
                    latest,
                    path,
                    sha256,
                    verified,
                } => {
                    let short_hash = &sha256[..sha256.len().min(16)];
                    let verify_label = if *verified {
                        l10n::tr("SHA-256 verified")
                    } else {
                        l10n::tr("Unverified")
                    };
                    d.child(
                        div()
                            .debug_selector(|| "settings-update-downloaded".to_owned())
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.foreground)
                            .child(format!(
                                "{} v{latest} ({verify_label} · {short_hash})",
                                l10n::tr("Downloaded")
                            ))
                            .child(div().text_color(theme.muted_foreground).child(path.clone())),
                    )
                }
                UpdateStatus::Error(err) => d.child(
                    div()
                        .debug_selector(|| "settings-update-error".to_owned())
                        .text_xs()
                        .text_color(theme.danger)
                        .child(err.clone()),
                ),
            })
            .into_any_element(),
        theme,
    );

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(updates_section)
        .into_any_element()
}

fn render_applications_tab(modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    const DOCS_URL: &str = "https://docs.fluxer.app";

    if let Some(selected_id) = &modal.selected_application_id {
        let selected_app = modal.applications.iter().find(|a| &a.id == selected_id);
        let detail_body: AnyElement = if let Some(app) = selected_app {
            let app_name = app.name.clone();
            let app_id = app.id.clone();
            let redirect_summary = if app.redirect_uris.is_empty() {
                l10n::tr("None configured")
            } else {
                app.redirect_uris.join(", ")
            };
            let bot_summary = match &app.bot {
                Some(bot) => format!("{}#{} (ID: {})", bot.username, bot.discriminator, bot.id),
                None => l10n::tr("No bot user attached"),
            };
            div()
                .debug_selector(|| "settings-application-detail".to_owned())
                .flex()
                .flex_col()
                .gap_3()
                .child(info_card_row(
                    l10n::tr("Application name"),
                    app_name.clone(),
                    Some(format!("ID: {app_id}")),
                    theme,
                ))
                .child(info_card_row(
                    l10n::tr("Bot user"),
                    bot_summary,
                    Some(if app.bot_public {
                        l10n::tr("Public")
                    } else {
                        l10n::tr("Private")
                    }),
                    theme,
                ))
                .child(info_card_row(
                    l10n::tr("OAuth2 redirect URIs"),
                    redirect_summary,
                    Some(if app.bot_require_code_grant {
                        l10n::tr("Code grant required")
                    } else {
                        l10n::tr("Standard")
                    }),
                    theme,
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("settings-application-reset-token")
                                .ghost()
                                .small()
                                .label(l10n::tr("Reset bot token"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // TODO(backlog, T3): Connect bot token reset flow with sudo confirmation.
                                    this.settings_set_applications_notice(
                                        l10n::tr("Resetting bot tokens is deferred to the T3 workflow."),
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            Button::new("settings-application-delete")
                                .danger()
                                .small()
                                .label(l10n::tr("Delete application"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // TODO(backlog, T3): Connect DELETE /oauth2/applications/{id} with confirmation.
                                    this.settings_set_applications_notice(
                                        format!(
                                            "{} ({app_name})",
                                            l10n::tr("Deleting applications is deferred to the T3 workflow.")
                                        ),
                                        cx,
                                    );
                                })),
                        ),
                )
                .into_any_element()
        } else if modal.selected_application_loading {
            div()
                .p_4()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Loading application details…"))
                .into_any_element()
        } else {
            div()
                .p_4()
                .text_xs()
                .text_color(theme.danger)
                .child(
                    modal
                        .applications_error
                        .clone()
                        .unwrap_or_else(|| l10n::tr("Unable to load application details.").into()),
                )
                .into_any_element()
        };

        return div()
            .debug_selector(|| "settings-applications-tab".to_owned())
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .debug_selector(|| "settings-application-back".to_owned())
                    .child(
                        Button::new("settings-application-back")
                            .ghost()
                            .small()
                            .icon(AppIcon::ArrowBendUpLeft)
                            .label(l10n::tr("Back to applications"))
                            .on_click(cx.listener(|this, _, _, cx| this.settings_back_to_applications(cx))),
                    ),
            )
            .when_some(modal.applications_notice.clone(), |d, notice| {
                d.child(
                    div()
                        .debug_selector(|| "settings-applications-notice".to_owned())
                        .p_2p5()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(settings_border(theme))
                        .bg(settings_card_bg(theme))
                        .text_xs()
                        .text_color(theme.foreground)
                        .child(notice),
                )
            })
            .child(settings_section_block(l10n::tr("Application details"), None, detail_body, theme))
            .into_any_element();
    }

    let action_bar = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_3()
        .flx("user.applications-tab.button-container")
        .child(
            div()
                .debug_selector(|| "settings-applications-create".to_owned())
                .flx("user.applications-tab.button.open-create-modal--2")
                .child(
                    Button::new("settings-applications-create")
                        .primary()
                        .small()
                        .label(l10n::tr("Create application"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            // TODO(backlog, T3): Connect POST /oauth2/applications creation modal.
                            this.settings_set_applications_notice(
                                l10n::tr("Creating applications is deferred to the T3 workflow."),
                                cx,
                            );
                        })),
                ),
        )
        .child(
            div()
                .debug_selector(|| "settings-applications-docs".to_owned())
                .flx("user.applications-tab.documentation-link")
                .child(
                    Button::new("settings-applications-docs")
                        .ghost()
                        .small()
                        .icon(AppIcon::ArrowSquareOut)
                        .label(l10n::tr("Read the documentation (docs.fluxer.app)"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            #[cfg(test)]
                            this.launch_external_url(DOCS_URL, cx);
                            #[cfg(not(test))]
                            {
                                let _ = this;
                                cx.open_url(DOCS_URL);
                            }
                        })),
                ),
        );

    let list_body: AnyElement = if modal.applications_loading && modal.applications.is_empty() {
        div()
            .debug_selector(|| "settings-applications-loading".to_owned())
            .p_4()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(l10n::tr("Loading applications…"))
            .into_any_element()
    } else if let Some(err) = &modal.applications_error {
        div()
            .debug_selector(|| "settings-applications-error".to_owned())
            .p_4()
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(settings_card_bg(theme))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .child(div().text_xs().text_color(theme.danger).child(err.clone()))
            .child(
                Button::new("settings-applications-retry")
                    .ghost()
                    .small()
                    .label(l10n::tr("Try again"))
                    .on_click(cx.listener(|this, _, _, cx| this.settings_reload_applications(cx))),
            )
            .into_any_element()
    } else if modal.applications.is_empty() {
        div()
            .debug_selector(|| "settings-applications-empty".to_owned())
            .p_6()
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(settings_card_bg(theme))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .flx("user.applications-tab.applications-list.empty-state")
            .child(Icon::new(AppIcon::Code).with_size(px(28.)).text_color(theme.muted_foreground))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("No applications yet")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("Create your first application to get started with the Fluxer API.")),
            )
            .into_any_element()
    } else {
        div()
            .debug_selector(|| "settings-applications-list".to_owned())
            .flex()
            .flex_col()
            .gap_2()
            .flx("user.applications-tab.applications-list.list-container")
            .children(modal.applications.iter().map(|app| {
                let app_id = app.id.clone();
                let click_id = app.id.clone();
                let sel_id = format!("settings-application-item-{}", app.id);
                let initial = app
                    .name
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_else(|| "A".to_owned());
                div()
                    .id(SharedString::from(sel_id.clone()))
                    .debug_selector(move || sel_id.clone())
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .bg(settings_card_bg(theme))
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .flx("user.applications-tab.applications-list.item-button.select-application")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_select_application(click_id.clone(), cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .min_w_0()
                            .child(
                                div()
                                    .size(px(36.))
                                    .rounded(px(8.))
                                    .bg(theme.muted)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(initial),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .min_w_0()
                                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(app.name.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(format!("ID: {app_id}")),
                                    ),
                            ),
                    )
                    .child(Icon::new(AppIcon::CaretRight).with_size(px(16.)).text_color(theme.muted_foreground))
            }))
            .into_any_element()
    };

    div()
        .debug_selector(|| "settings-applications-tab".to_owned())
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section_block(
            l10n::tr("Applications"),
            None,
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(action_bar)
                .when_some(modal.applications_notice.clone(), |d, notice| {
                    d.child(
                        div()
                            .debug_selector(|| "settings-applications-notice".to_owned())
                            .p_2p5()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(settings_border(theme))
                            .bg(settings_card_bg(theme))
                            .text_xs()
                            .text_color(theme.foreground)
                            .child(notice),
                    )
                })
                .child(list_body)
                .into_any_element(),
            theme,
        ))
        .into_any_element()
}

fn render_tab_body(
    tab: SettingsTab,
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    match tab {
        SettingsTab::Profile => render_profile_tab(view, theme, cx),
        SettingsTab::Account => render_account_tab(view, modal, theme, cx),
        SettingsTab::PrivacySafety => render_privacy_tab(view, modal, theme, cx),
        SettingsTab::Connections => render_connections_tab(modal, theme, cx),
        SettingsTab::Appearance => render_appearance_tab(view, theme, cx),
        SettingsTab::Themes => render_themes_tab(view, theme, cx),
        SettingsTab::Notifications => render_notifications_tab(view, theme, cx),
        SettingsTab::MessagesMedia => render_messages_media_tab(view, theme, cx),
        SettingsTab::VoiceVideo => render_voice_tab(view, modal.active_section, theme, cx),
        SettingsTab::Accessibility => render_accessibility_tab(view, theme, cx),
        SettingsTab::Language => render_language_tab(view, modal, theme, cx),
        SettingsTab::Keybinds => render_keybinds_tab(view, modal, theme, cx),
        SettingsTab::WindowsApp => render_windows_app_tab(view, theme, cx),
        SettingsTab::Advanced => render_advanced_tab(view, theme, cx),
        SettingsTab::Applications => render_applications_tab(modal, theme, cx),
    }
}

fn render_sidebar_tab(
    tab: SettingsTab,
    selected: bool,
    active_section: Option<&'static str>,
    show_sections: bool,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let sel_id = tab.debug_selector();
    let sections = tab.sections();
    let selected_bg = settings_sub_selected_bg(theme);
    let active_text = theme.foreground;
    let idle_text = theme.muted_foreground;
    let sub_idle_text = theme.text_tertiary;
    let hover_bg = theme.hover_bg;
    let has_sections = sections.len() > 1;
    let has_tree = selected && show_sections && has_sections;

    let mut item = div().w_full().flex().flex_col().gap(px(3.)).child(
        div()
            .id(sel_id)
            .debug_selector(move || sel_id.to_owned())
            .w_full()
            .h(px(34.))
            .px(px(11.))
            .rounded(px(8.))
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .text_color(if selected { active_text } else { idle_text })
            .when(selected, |d| d.bg(selected_bg).font_weight(FontWeight::MEDIUM))
            .when(!selected, |d| {
                d.hover(move |s| s.bg(hover_bg).text_color(active_text))
            })
            .flx("app.settings-modal-layout.settings-modal-sidebar-item.button")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_settings_tab(tab, cx);
            }))
            .child(
                div()
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-icon")
                    .child(Icon::new(tab.icon()).with_size(px(20.))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(18.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .truncate()
                    .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-label")
                    .child(tab.label()),
            )
            .when(has_sections, |d| {
                d.child(
                    div()
                        .size(px(16.))
                        .ml(px(4.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-trailing")
                        .child(
                            div()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-chevron")
                                .child(
                                    Icon::new(if has_tree {
                                        AppIcon::CaretDown
                                    } else {
                                        AppIcon::CaretRight
                                    })
                                    .with_size(px(16.))
                                    .text_color(sub_idle_text),
                                ),
                        ),
                )
            }),
    );

    if has_tree {
        let mut sub_list = div()
            .ml(px(21.))
            .pl(px(6.))
            .border_l_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap(px(2.))
            .flx("app.settings-modal-layout.settings-modal-sidebar-sub-items.sidebar-sub-items");
        for (idx, &(sec_id, sec_label)) in sections.iter().enumerate() {
            let is_active_sec = active_section == Some(sec_id) || (active_section.is_none() && idx == 0);
            let sub_sel = format!("settings-subitem-{sec_id}");
            let sub_id = SharedString::from(sub_sel.clone());
            sub_list = sub_list.child(
                div()
                    .id(sub_id)
                    .debug_selector(move || sub_sel.clone())
                    .w_full()
                    .h(px(30.))
                    .px(px(11.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(if is_active_sec { active_text } else { sub_idle_text })
                    .when(is_active_sec, |d| d.font_weight(FontWeight::SEMIBOLD).bg(selected_bg))
                    .when(!is_active_sec, |d| {
                        d.hover(move |s| s.bg(hover_bg).text_color(active_text))
                    })
                    .flx("app.settings-modal-layout.settings-modal-sidebar-sub-item.sidebar-sub-item.click.button")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_settings_section(tab, sec_id, cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(18.))
                            .line_height(px(18.))
                            .truncate()
                            .flx("app.settings-modal-layout.settings-modal-sidebar-sub-item.sidebar-sub-item-label")
                            .child(l10n::tr(sec_label)),
                    ),
            );
        }
        item = item.child(
            div()
                .w_full()
                .mt(px(3.))
                .flx("app.settings-modal-layout.settings-modal-sidebar-sub-items.sidebar-sub-items-wrap")
                .child(sub_list),
        );
    }

    item.into_any_element()
}

fn render_sidebar(
    view: &ClientView,
    modal: &SettingsModal,
    query_lower: &str,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let sidebar_bg = settings_sidebar_bg(theme);
    let hover_bg = theme.hover_bg;
    let foreground = theme.foreground;
    let danger_hover = theme.danger.opacity(0.10);
    let (avatar_el, user_name) = match &view.user {
        Some(user) => (
            div()
                .debug_selector(|| "settings-profile-avatar".to_owned())
                .child(render_avatar(
                    AvatarSpec {
                        user_id: user.id,
                        avatar_url: user.avatar_url.clone(),
                        display_name: user.name.clone(),
                        size: 40.,
                        status: Some(view.own_status),
                        status_border_bg: Some(sidebar_bg),
                    },
                    theme,
                ))
                .into_any_element(),
            user.name.clone(),
        ),
        None => (
            div().size(px(40.)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("…"),
        ),
    };

    let account_tabs: Vec<SettingsTab> = SettingsTab::ACCOUNT_TABS
        .iter()
        .copied()
        .filter(|t| t.matches_query(query_lower))
        .collect();
    let app_tabs: Vec<SettingsTab> = SettingsTab::APP_TABS
        .iter()
        .copied()
        .filter(|t| t.matches_query(query_lower))
        .collect();
    let developer_tabs: Vec<SettingsTab> = SettingsTab::DEVELOPER_TABS
        .iter()
        .copied()
        .filter(|t| t.matches_query(query_lower))
        .collect();
    let show_sections = query_lower.is_empty();
    let profile_selected = modal.active_tab == SettingsTab::Profile;

    let footer = if modal.confirm_logout {
        div()
            .flex_none()
            .px_3()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(settings_divider(theme))
            .child(div().pt_1().text_sm().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Sign out?")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("You can sign back in at any time.")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div().debug_selector(|| "settings-logout-confirm".to_owned()).child(
                            Button::new("settings-logout-confirm")
                                .danger()
                                .small()
                                .label(l10n::tr("Sign out"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.apply_settings_effect(SettingsEffect::LogOutRequested, window, cx);
                                })),
                        ),
                    )
                    .child(
                        Button::new("settings-logout-cancel")
                            .ghost()
                            .small()
                            .label(l10n::tr("Stay signed in"))
                            .on_click(cx.listener(|this, _, _, cx| this.settings_prompt_logout(false, cx))),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .opacity(0.65)
                    .child(format!("Fluxer Rust v{CURRENT_VERSION}")),
            )
            .into_any_element()
    } else {
        div()
            .flex_none()
            .px_2p5()
            .pb_3()
            .pt_1()
            .flex()
            .flex_col()
            .gap_1()
            .child(settings_divider(theme))
            .when(view.accounts_enabled, |d| {
                let switch_label = if view.saved_accounts.len() > 1 {
                    format!("{} ({})", l10n::tr("Switch accounts"), view.saved_accounts.len())
                } else {
                    l10n::tr("Switch accounts")
                };
                d.child(
                    div()
                        .id("settings-switch-accounts")
                        .debug_selector(|| "settings-switch-accounts".to_owned())
                        .h(px(32.))
                        .px_2p5()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2p5()
                        .text_color(theme.muted_foreground)
                        .hover(move |s| s.bg(hover_bg).text_color(foreground))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_settings_effect(SettingsEffect::OpenAccountSwitcher, window, cx);
                        }))
                        .child(Icon::new(AppIcon::Users).with_size(px(16.)))
                        .child(div().text_size(px(13.)).truncate().child(switch_label)),
                )
            })
            .child(
                div()
                    .id("settings-logout")
                    .debug_selector(|| "settings-logout".to_owned())
                    .h(px(34.))
                    .px_2p5()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2p5()
                    .text_color(theme.danger)
                    .hover(move |s| s.bg(danger_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.settings_prompt_logout(true, cx)))
                    .child(Icon::new(AppIcon::SignOut).with_size(px(16.)).text_color(theme.danger))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(l10n::tr("Sign out"))),
            )
            .child(
                div()
                    .px_2p5()
                    .pt_2()
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .opacity(0.65)
                    .child(format!("Fluxer Rust v{CURRENT_VERSION}")),
            )
            .into_any_element()
    };

    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .px(px(12.))
        .pt(px(16.))
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .h(px(106.))
                .pb(px(8.))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(10.))
                .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-header")
                .child(
                    div()
                        .w_full()
                        .h(px(34.))
                        .flex_none()
                        .flx("app.settings-search.container")
                        .child(
                            div()
                                .debug_selector(|| "settings-search".to_owned())
                                .w_full()
                                .h(px(34.))
                                .px_2p5()
                                .rounded(px(8.))
                                .bg(sidebar_bg)
                                .border_1()
                                .border_color(settings_border(theme))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .flx("app.settings-search.input-container")
                                .child(
                                    div()
                                        .size(px(16.))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flx("app.settings-search.magnifying-glass-icon")
                                        .child(
                                            Icon::new(AppIcon::MagnifyingGlass)
                                                .with_size(px(16.))
                                                .text_color(theme.muted_foreground),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .h(px(34.))
                                        .flex()
                                        .items_center()
                                        .flx("app.settings-search.input.query-change.text")
                                        .child(Input::new(&modal.search).small()),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("settings-profile-card")
                        .debug_selector(|| "settings-profile-card".to_owned())
                        .w_full()
                        .h(px(54.))
                        .flex_none()
                        .px(px(9.))
                        .py(px(7.))
                        .rounded(px(8.))
                        .bg(if profile_selected {
                            settings_sub_selected_bg(theme)
                        } else {
                            gpui_kit::transparent_black()
                        })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_settings_tab(SettingsTab::Profile, cx);
                        }))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2p5()
                        .child(div().flex_none().child(avatar_el))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(user_name))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(Icon::new(AppIcon::PencilBold).with_size(px(12.)))
                                        .child(div().truncate().child(l10n::tr("Edit your profile"))),
                                ),
                        ),
                ),
        )
        .child(
            div()
                .w_full()
                .mt(px(4.))
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav-wrapper")
                .child(
                    div()
                        .id("settings-sidebar-scroll")
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&modal.sidebar_scroll)
                        .flex()
                        .flex_col()
                        .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav")
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav-content")
                                .child(
                                    div()
                                        .w_full()
                                        .flex()
                                        .flex_col()
                                        .gap(px(3.))
                                        .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav-list.sidebar-nav-key-down")
                                        .when(!account_tabs.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .w_full()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(3.))
                                                    .flx("app.settings-modal-layout.settings-modal-sidebar-category.sidebar-category")
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .px(px(10.))
                                                            .pt(px(4.))
                                                            .pb(px(3.))
                                                            .flx("app.settings-modal-layout.settings-modal-sidebar-category-title.sidebar-category-title")
                                                            .child(sidebar_section_heading(
                                                                l10n::tr("Your account"),
                                                                theme,
                                                            )),
                                                    )
                                                    .children(account_tabs.iter().map(|&tab| {
                                                        render_sidebar_tab(
                                                            tab,
                                                            modal.active_tab == tab,
                                                            modal.active_section,
                                                            show_sections,
                                                            theme,
                                                            cx,
                                                        )
                                                    })),
                                            )
                                        })
                                        .when(!app_tabs.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .w_full()
                                                    .mt(px(8.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(3.))
                                                    .flx("app.settings-modal-layout.settings-modal-sidebar-category.sidebar-category")
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .px(px(10.))
                                                            .pt(px(4.))
                                                            .pb(px(3.))
                                                            .flx("app.settings-modal-layout.settings-modal-sidebar-category-title.sidebar-category-title")
                                                            .child(sidebar_section_heading(
                                                                l10n::tr("App settings"),
                                                                theme,
                                                            )),
                                                    )
                                                    .children(app_tabs.iter().map(|&tab| {
                                                        render_sidebar_tab(
                                                            tab,
                                                            modal.active_tab == tab,
                                                            modal.active_section,
                                                            show_sections,
                                                            theme,
                                                            cx,
                                                        )
                                                    })),
                                            )
                                        })
                                        .when(!developer_tabs.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .w_full()
                                                    .mt(px(8.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(3.))
                                                    .flx("app.settings-modal-layout.settings-modal-sidebar-category.sidebar-category")
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .px(px(10.))
                                                            .pt(px(4.))
                                                            .pb(px(3.))
                                                            .flx("app.settings-modal-layout.settings-modal-sidebar-category-title.sidebar-category-title")
                                                            .child(sidebar_section_heading(
                                                                l10n::tr("Developer"),
                                                                theme,
                                                            )),
                                                    )
                                                    .children(developer_tabs.iter().map(|&tab| {
                                                        render_sidebar_tab(
                                                            tab,
                                                            modal.active_tab == tab,
                                                            modal.active_section,
                                                            show_sections,
                                                            theme,
                                                            cx,
                                                        )
                                                    })),
                                            )
                                        })
                                        .when(
                                            account_tabs.is_empty() && app_tabs.is_empty() && developer_tabs.is_empty(),
                                            |d| {
                                                d.child(
                                                    div()
                                                        .px_2()
                                                        .py_2()
                                                        .text_xs()
                                                        .text_color(theme.muted_foreground)
                                                        .child(l10n::tr("No results")),
                                                )
                                            },
                                        ),
                                ),
                        ),
                ),
        )
        .child(footer)
        .into_any_element()
}

pub fn render(view: &ClientView, modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let query = modal.search.read(cx).value().trim().to_owned();
    let query_lower = query.to_lowercase();

    let sidebar = render_sidebar(view, modal, &query_lower, theme, cx);

    let (title, body): (SharedString, AnyElement) = if query_lower.is_empty() {
        (
            SharedString::from(modal.active_tab.label()),
            render_tab_body(modal.active_tab, view, modal, theme, cx),
        )
    } else {
        let matching: Vec<SettingsTab> = SettingsTab::ALL
            .iter()
            .copied()
            .filter(|t| t.matches_query(&query_lower))
            .collect();
        let search_body = if matching.is_empty() {
            div()
                .py_6()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("No results"))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_6()
                .children(matching.into_iter().map(|tab| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(div().text_base().font_weight(FontWeight::BOLD).child(tab.label()))
                        .child(render_tab_body(tab, view, modal, theme, cx))
                }))
                .into_any_element()
        };
        (SharedString::from(l10n::tr("Search results")), search_body)
    };

    let view_weak = cx.weak_entity();
    div()
        .id("settings-modal")
        .key_context(CONTEXT)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            if this.is_recording_keybind() {
                this.settings_cancel_record_keybind(cx);
            } else {
                this.close_overlay(window, cx);
            }
        }))
        .child(overlay::settings_modal(
            sidebar,
            title,
            body,
            theme,
            move |window, cx| {
                let _ = view_weak.update(cx, |v, cx| v.close_overlay(window, cx));
            },
        ))
        .into_any_element()
}
