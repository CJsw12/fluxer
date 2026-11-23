//! 2-pane user settings modal (`SettingsModalLayout.tsx` + `DesktopSettingsView.tsx`
//! + `SettingsConstants.tsx` + `SettingsSectionRegistry.ts`):
//! left navigation sidebar (search, clickable 40px profile card -> `Profile` tab,
//! categorized tabs with expandable sub-section tree `SidebarSections`, and sign-out footer)
//! and right scrollable content pane with official section cards.

use fluxer_client_core::rest::{UserSettingsPatch, WebAuthnCredential};
use fluxer_client_core::state::Status;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, ScrollHandle, SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::backend::voice::VoiceAudioSettings;
use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::prefs::{DEFAULT_VOICE_VOLUME, MAX_VOICE_VOLUME, MIN_VOICE_VOLUME, ReducedMotionPref};
use crate::settings_model::{FONT_SIZE_OPTIONS, LOCALES, SettingsEffect, THEME_OPTIONS, theme_patch_value};
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
    #[default]
    Appearance,
    Themes,
    Notifications,
    MessagesMedia,
    VoiceVideo,
    Accessibility,
    Language,
    Keybinds,
    Advanced,
}

impl SettingsTab {
    const ACCOUNT_TABS: &[SettingsTab] = &[SettingsTab::Account, SettingsTab::PrivacySafety];
    const APP_TABS: &[SettingsTab] = &[
        SettingsTab::Appearance,
        SettingsTab::Themes,
        SettingsTab::Notifications,
        SettingsTab::MessagesMedia,
        SettingsTab::VoiceVideo,
        SettingsTab::Accessibility,
        SettingsTab::Language,
        SettingsTab::Keybinds,
        SettingsTab::Advanced,
    ];
    const ALL: &[SettingsTab] = &[
        SettingsTab::Profile,
        SettingsTab::Account,
        SettingsTab::PrivacySafety,
        SettingsTab::Appearance,
        SettingsTab::Themes,
        SettingsTab::Notifications,
        SettingsTab::MessagesMedia,
        SettingsTab::VoiceVideo,
        SettingsTab::Accessibility,
        SettingsTab::Language,
        SettingsTab::Keybinds,
        SettingsTab::Advanced,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Profile => l10n::tr("Profile"),
            Self::Account => l10n::tr("Account"),
            Self::PrivacySafety => l10n::tr("Privacy & safety"),
            Self::Appearance => l10n::tr("Appearance"),
            Self::Themes => l10n::tr("Themes"),
            Self::Notifications => l10n::tr("Notifications"),
            Self::MessagesMedia => l10n::tr("Messages & media"),
            Self::VoiceVideo => l10n::tr("Voice & video"),
            Self::Accessibility => l10n::tr("Accessibility"),
            Self::Language => l10n::tr("Language"),
            Self::Keybinds => l10n::tr("Keybinds"),
            Self::Advanced => l10n::tr("Advanced"),
        }
    }

    pub const fn debug_selector(self) -> &'static str {
        match self {
            Self::Profile => "settings-tab-profile",
            Self::Account => "settings-tab-account",
            Self::PrivacySafety => "settings-tab-privacy",
            Self::Appearance => "settings-tab-appearance",
            Self::Themes => "settings-tab-themes",
            Self::Notifications => "settings-tab-notifications",
            Self::MessagesMedia => "settings-tab-chat",
            Self::VoiceVideo => "settings-tab-voice",
            Self::Accessibility => "settings-tab-accessibility",
            Self::Language => "settings-tab-language",
            Self::Keybinds => "settings-tab-keybinds",
            Self::Advanced => "settings-tab-advanced",
        }
    }

    const fn icon(self) -> AppIcon {
        match self {
            Self::Profile => AppIcon::UserCircle,
            Self::Account => AppIcon::UserCircle,
            Self::PrivacySafety => AppIcon::EyeSlash,
            Self::Appearance => AppIcon::Palette,
            Self::Themes => AppIcon::Palette,
            Self::Notifications => AppIcon::Bell,
            Self::MessagesMedia => AppIcon::ChatsCircle,
            Self::VoiceVideo => AppIcon::SpeakerHigh,
            Self::Accessibility => AppIcon::PersonArmsSpread,
            Self::Language => AppIcon::Globe,
            Self::Keybinds => AppIcon::SlidersHorizontal,
            Self::Advanced => AppIcon::Wrench,
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
                ("friends-dms", "Friends & direct messages"),
                ("sensitive-content", "Sensitive content"),
                ("link-safety", "Link protection"),
            ],
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
            Self::Advanced => &[
                ("desktop-window", "Windows app"),
                ("updates", "About & updates"),
            ],
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
                "direct message",
                "friend request",
                "sensitive",
                "mature",
                "filter",
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
            Self::Advanced => &["advanced", "startup", "autostart", "tray", "update", "version", "about", "windows app"],
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
    pub sidebar_scroll: ScrollHandle,
    pub locale_scroll: ScrollHandle,
    pub _subscription: Subscription,
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
            sidebar_scroll: ScrollHandle::new(),
            locale_scroll: ScrollHandle::new(),
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(crate) fn select_settings_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.active_tab = tab;
            modal.active_section = tab.sections().first().map(|(id, _)| *id);
            if tab == SettingsTab::Account && modal.passkeys.is_empty() && !modal.passkey_loading {
                modal.passkey_loading = true;
                self.backend.load_webauthn_credentials();
            }
            cx.notify();
        }
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
            if tab == SettingsTab::Account && modal.passkeys.is_empty() && !modal.passkey_loading {
                modal.passkey_loading = true;
                self.backend.load_webauthn_credentials();
            }
            cx.notify();
        }
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
            SettingsEffect::AllowGuildDmsChanged(enabled) => {
                self.prefs.set_allow_guild_dms(enabled);
                cx.notify();
            }
            SettingsEffect::InlineMediaPreviewsChanged(enabled) => {
                self.prefs.set_inline_media_previews(enabled);
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

fn section_heading(title: String, theme: &Theme) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(theme.muted_foreground)
        .child(title)
}

fn info_card_row(title: String, subtitle: String, badge: Option<String>, theme: &Theme) -> AnyElement {
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
                    .border_color(theme.border)
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(b),
            )
        })
        .into_any_element()
}

fn render_profile_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (avatar_el, display_name, user_id_str) = match &view.user {
        Some(user) => (
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
        ),
        None => (
            div().size(px(64.)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("Fluxer User"),
            String::from("—"),
        ),
    };

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
                        .p_4()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .child(div().flex_none().child(avatar_el))
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
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
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

fn render_privacy_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let trusted_count = view.effective_trusted_domains().len();
    div()
        .debug_selector(|| "settings-privacy-dm".to_owned())
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Friends & direct messages"), theme))
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
                                        .child(l10n::tr("Direct messages from community members")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Allow members of shared communities to start direct messages with you.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-allow-guild-dms".to_owned()).child(
                                Switch::new("settings-allow-guild-dms")
                                    .checked(view.prefs.data.allow_guild_dms)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::AllowGuildDmsChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                )
                .child(info_card_row(
                    l10n::tr("Friend requests"),
                    l10n::tr("Control who can send you friend requests."),
                    Some(l10n::tr("Everyone")),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Sensitive content"), theme))
                .child(info_card_row(
                    l10n::tr("Spoilers & sensitive media"),
                    l10n::tr("Blur spoiler-tagged text and media until clicked."),
                    Some(l10n::tr("On click")),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Link protection"), theme))
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
                        }),
                ),
        )
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
        .border_color(theme.border)
        .bg(theme.background_secondary)
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
        .border_color(if selected { theme.brand } else { theme.border })
        .bg(if selected { theme.accent } else { theme.background_secondary })
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

    let theme_swatches = THEME_OPTIONS.iter().enumerate().map(|(ix, (kind, label))| {
        let kind = *kind;
        render_theme_swatch(ix, kind, label, active_theme == kind, theme, cx)
    });

    let density_cards = [
        (false, "Cozy", "Show avatars and generous spacing between messages.", "settings-density-cozy"),
        (true, "Compact", "Fit more messages on screen with inline timestamps.", "settings-density-compact"),
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
            .flex_1()
            .min_w(px(180.))
            .p_3()
            .rounded(px(8.))
            .border_2()
            .border_color(if selected { theme.brand } else { theme.border })
            .bg(if selected { theme.accent } else { theme.background_secondary })
            .cursor_pointer()
            .hover(|s| s.border_color(theme.brand))
            .flex()
            .flex_col()
            .gap_1()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.apply_settings_effect(SettingsEffect::CompactChanged(compact), window, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l10n::tr(label)))
                    .when(selected, |d| {
                        d.child(Icon::new(AppIcon::Check).xsmall().text_color(theme.brand))
                    }),
            )
            .child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr(desc)))
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
                    .size(px(if selected { 16. } else { 10. }))
                    .rounded_full()
                    .border_2()
                    .border_color(if selected { theme.brand } else { theme.border })
                    .bg(if selected { theme.on_fill } else { theme.background_secondary }),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(if selected { FontWeight::BOLD } else { FontWeight::NORMAL })
                    .text_color(if selected { theme.foreground } else { theme.muted_foreground })
                    .child(format!("{size}px")),
            )
    });

    let font_slider = div()
        .debug_selector(|| "settings-font-slider".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Font size")))
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded(px(4.))
                        .bg(theme.background)
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(format!("{}px", display.font_size)),
                ),
        )
        .child(
            div()
                .relative()
                .w_full()
                .pt_2()
                .child(
                    div()
                        .w_full()
                        .h(px(4.))
                        .rounded_full()
                        .bg(theme.border)
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
    let zoom_controls = div()
        .debug_selector(|| "settings-zoom-slider".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Zoom")))
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
                                    .small()
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
                                .py_0p5()
                                .rounded(px(4.))
                                .bg(theme.background)
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{}%", display.ui_zoom)),
                        )
                        .child(
                            div().debug_selector(|| "settings-zoom-in".to_owned()).child(
                                Button::new("settings-zoom-in")
                                    .ghost()
                                    .small()
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
                                    .small()
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
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(theme.border)
                .overflow_hidden()
                .child(div().h_full().bg(theme.brand).w(gpui_kit::relative(zoom_fraction))),
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
                .child(div().flex().flex_row().flex_wrap().gap_3().children(theme_swatches)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(section_heading(l10n::tr("Display"), theme))
                .child(div().flex().flex_row().flex_wrap().gap_2().children(density_cards))
                .child(font_slider)
                .child(zoom_controls),
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
                .child(info_card_row(
                    l10n::tr("Unread channel & mention indicators"),
                    l10n::tr("Highlight unread channels in the sidebar and show red mention count pills."),
                    Some(l10n::tr("Enabled")),
                    theme,
                )),
        )
        .into_any_element()
}

fn render_messages_media_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
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
                .child(info_card_row(
                    l10n::tr("Website link previews & embeds"),
                    l10n::tr("Show rich embed cards and YouTube thumbnails for links posted in chat."),
                    Some(l10n::tr("Enabled")),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Spoilers & reactions"), theme))
                .child(info_card_row(
                    l10n::tr("Spoiler tags"),
                    l10n::tr("Click hidden spoiler blocks to reveal text and images."),
                    Some(l10n::tr("On click")),
                    theme,
                ))
                .child(info_card_row(
                    l10n::tr("Emoji reactions"),
                    l10n::tr("Display reaction pills under messages and allow quick reaction toggling."),
                    Some(l10n::tr("Enabled")),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Text input & autocomplete"), theme))
                .child(info_card_row(
                    l10n::tr("Autocomplete suggestions"),
                    l10n::tr("Show popup suggestions when typing @mentions, #channels, and :emoji: shortcodes."),
                    Some(l10n::tr("Enabled")),
                    theme,
                )),
        )
        .into_any_element()
}

fn render_voice_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_in_dev = view
        .prefs
        .data
        .voice_input_device
        .clone()
        .unwrap_or_else(|| l10n::tr("System default"));
    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
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
                )),
        )
        .child(
            div()
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
                                        let vad = !view.prefs.data.voice_push_to_talk;
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
                                        let ptt = view.prefs.data.voice_push_to_talk;
                                        let mut btn = Button::new("settings-input-mode-ptt")
                                            .small()
                                            .label(l10n::tr("Push to Talk"));
                                        btn = if ptt { btn.primary() } else { btn.ghost() };
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
                ),
        )
        .child(
            div()
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
                                        .child(l10n::tr("Echo cancellation & noise suppression")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "WebRTC hardware-assisted acoustic echo cancellation, noise suppression, and auto gain.",
                                        )),
                                ),
                        )
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
                .debug_selector(|| "settings-voice-mic-test".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Microphone test"), theme))
                .child(info_card_row(
                    l10n::tr("Input monitor"),
                    format!("{}: {active_in_dev} ({}%)", l10n::tr("Active input"), view.prefs.data.voice_input_volume),
                    Some(if view.voice.effective_muted() {
                        l10n::tr("Muted")
                    } else {
                        l10n::tr("Ready")
                    }),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Video & screen share"), theme))
                .child(info_card_row(
                    l10n::tr("Screen share codec & quality"),
                    l10n::tr("Automatic H.265 / H.264 / VP8 capability negotiation with system audio capture."),
                    Some(String::from("1080p · 60fps")),
                    theme,
                )),
        )
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

fn render_language_tab(modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_locale = l10n::current().locale();
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
                .child(info_card_row(
                    l10n::tr("Regional timestamp formatting"),
                    l10n::tr("Automatically formats message timestamps and relative dates according to your selected locale."),
                    Some(active_locale.to_owned()),
                    theme,
                )),
        )
        .into_any_element()
}

fn render_keybinds_tab(theme: &Theme) -> AnyElement {
    let shortcuts: &[(&str, &str)] = &[
        ("Quick switcher", "Ctrl + K"),
        ("Open user settings", "Ctrl + ,"),
        ("Search messages in channel", "Ctrl + F"),
        ("Send message", "Enter"),
        ("Insert newline", "Shift + Enter"),
        ("Edit last own message", "Up Arrow"),
        ("Cancel reply / edit / close modal", "Escape"),
        ("Zoom in interface", "Ctrl + ="),
        ("Zoom out interface", "Ctrl + -"),
        ("Reset interface zoom", "Ctrl + 0"),
    ];

    div()
        .debug_selector(|| "settings-keybinds-list".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .child(section_heading(l10n::tr("Keyboard shortcuts"), theme))
        .children(shortcuts.iter().map(|(action, combo)| {
            info_card_row(
                l10n::tr(action),
                l10n::tr("Built-in desktop keyboard shortcut"),
                Some((*combo).to_owned()),
                theme,
            )
        }))
        .into_any_element()
}

fn render_advanced_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let autostart_row = div()
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

    let updates_section = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_heading(l10n::tr("About & Updates"), theme))
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
        });

    div()
        .flex()
        .flex_col()
        .gap_5()
        .when(cfg!(any(target_os = "windows", test)), |d| {
            d.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_heading(l10n::tr("Windows app"), theme))
                    .child(autostart_row)
                    .child(close_to_tray_row),
            )
        })
        .child(updates_section)
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
        SettingsTab::PrivacySafety => render_privacy_tab(view, theme, cx),
        SettingsTab::Appearance => render_appearance_tab(view, theme, cx),
        SettingsTab::Themes => render_themes_tab(view, theme, cx),
        SettingsTab::Notifications => render_notifications_tab(view, theme, cx),
        SettingsTab::MessagesMedia => render_messages_media_tab(view, theme, cx),
        SettingsTab::VoiceVideo => render_voice_tab(view, theme, cx),
        SettingsTab::Accessibility => render_accessibility_tab(view, theme, cx),
        SettingsTab::Language => render_language_tab(modal, theme, cx),
        SettingsTab::Keybinds => render_keybinds_tab(theme),
        SettingsTab::Advanced => render_advanced_tab(view, theme, cx),
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
    let mut item = div().flex().flex_col().gap_0p5().child(
        div()
            .id(sel_id)
            .debug_selector(move || sel_id.to_owned())
            .px_2()
            .py(px(6.))
            .rounded(px(6.))
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .text_color(if selected { theme.foreground } else { theme.muted_foreground })
            .when(selected, |d| d.bg(theme.accent).font_weight(FontWeight::SEMIBOLD))
            .when(!selected, |d| d.hover(|s| s.bg(theme.muted).text_color(theme.foreground)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_settings_tab(tab, cx);
            }))
            .child(Icon::new(tab.icon()).small())
            .child(div().text_sm().truncate().child(tab.label())),
    );

    if selected && show_sections && sections.len() > 1 {
        let mut sub_list = div()
            .ml_4()
            .pl_2()
            .border_l_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap(px(2.));
        for &(sec_id, sec_label) in sections {
            let is_active_sec = active_section == Some(sec_id);
            let sub_sel = format!("settings-subitem-{sec_id}");
            let sub_id = SharedString::from(sub_sel.clone());
            sub_list = sub_list.child(
                div()
                    .id(sub_id)
                    .debug_selector(move || sub_sel.clone())
                    .px_2()
                    .py_1()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_xs()
                    .text_color(if is_active_sec {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(is_active_sec, |d| d.font_weight(FontWeight::SEMIBOLD).bg(theme.muted))
                    .hover(|s| s.text_color(theme.foreground))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_settings_section(tab, sec_id, cx);
                    }))
                    .child(l10n::tr(sec_label)),
            );
        }
        item = item.child(sub_list);
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
                        status_border_bg: Some(theme.background),
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
    let show_sections = query_lower.is_empty();
    let profile_selected = modal.active_tab == SettingsTab::Profile;

    let footer = if modal.confirm_logout {
        div()
            .flex_none()
            .px_3()
            .py_3()
            .border_t_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Sign out?")))
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
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("Fluxer Rust v{CURRENT_VERSION}")),
            )
            .into_any_element()
    } else {
        div()
            .flex_none()
            .px_3()
            .py_3()
            .border_t_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div().debug_selector(|| "settings-logout".to_owned()).child(
                            Button::new("settings-logout")
                                .danger()
                                .small()
                                .icon(AppIcon::SignOut)
                                .label(l10n::tr("Sign out"))
                                .on_click(cx.listener(|this, _, _, cx| this.settings_prompt_logout(true, cx))),
                        ),
                    )
                    .when(view.accounts_enabled, |d| {
                        let switch_label = if view.saved_accounts.len() > 1 {
                            format!("{} ({})", l10n::tr("Switch accounts"), view.saved_accounts.len())
                        } else {
                            l10n::tr("Switch accounts")
                        };
                        d.child(
                            div().debug_selector(|| "settings-switch-accounts".to_owned()).child(
                                Button::new("settings-switch-accounts")
                                    .ghost()
                                    .small()
                                    .icon(AppIcon::Users)
                                    .tooltip(switch_label.clone())
                                    .accessibility_label(switch_label)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_settings_effect(SettingsEffect::OpenAccountSwitcher, window, cx);
                                    })),
                            ),
                        )
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("Fluxer Rust v{CURRENT_VERSION}")),
            )
            .into_any_element()
    };

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_none()
                .px_3()
                .pt_3()
                .pb_2()
                .debug_selector(|| "settings-search".to_owned())
                .child(Input::new(&modal.search).small()),
        )
        .child(
            div()
                .id("settings-profile-card")
                .debug_selector(|| "settings-profile-card".to_owned())
                .flex_none()
                .mx_3()
                .mb_2()
                .px_2()
                .py_2()
                .rounded(px(8.))
                .bg(if profile_selected { theme.accent } else { theme.background })
                .border_1()
                .border_color(if profile_selected { theme.brand } else { theme.border })
                .cursor_pointer()
                .hover(|s| s.border_color(theme.brand))
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
                                .child(Icon::new(AppIcon::PencilSimple).xsmall())
                                .child(div().truncate().child(l10n::tr("Edit your profile"))),
                        ),
                ),
        )
        .child(
            div()
                .id("settings-sidebar-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&modal.sidebar_scroll)
                .px_2()
                .py_1()
                .flex()
                .flex_col()
                .gap_3()
                .when(!account_tabs.is_empty(), |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().px_2().child(section_heading(l10n::tr("Your account"), theme)))
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
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().px_2().child(section_heading(l10n::tr("App settings"), theme)))
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
                .when(account_tabs.is_empty() && app_tabs.is_empty(), |d| {
                    d.child(
                        div()
                            .px_2()
                            .py_2()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("No results")),
                    )
                }),
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
            this.close_overlay(window, cx);
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
