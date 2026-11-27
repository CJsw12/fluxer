//! Headless UI tests for the minimal settings modal.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::TestAppContext;

use crate::client_view::ClientEvent;
use crate::tests::{backend_with_fixture, draw, focus_composer, open_client, runtime};
use crate::theme::{self, Theme, ThemeKind};
use crate::ui::l10n;
use crate::ui::overlay::Overlay;
use crate::ui::settings::SettingsTab;
use crate::backend::lock_sent;

struct LocaleGuard;

impl Drop for LocaleGuard {
    fn drop(&mut self) {
        l10n::set_locale("en-US");
    }
}

#[gpui_kit::test]
fn settings_modal_updates_theme_switches_locale_and_logs_out(cx: &mut TestAppContext) {
    let _guard = LocaleGuard;
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend.clone(), events);
    let logged_out = Arc::new(AtomicBool::new(false));
    let logged_out_flag = logged_out.clone();
    let _sub = cx.update(|_window, cx| {
        cx.subscribe(&view, move |_, ev: &ClientEvent, _| {
            if *ev == ClientEvent::LoggedOut {
                logged_out_flag.store(true, Ordering::SeqCst);
            }
        })
    });

    focus_composer(&view, cx);
    cx.simulate_keystrokes("ctrl-,");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::Settings(_))));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Reopen and switch theme to Light -> Dark
    cx.simulate_keystrokes("ctrl-,");
    cx.run_until_parked();
    view.update_in(cx, |v, window, cx| v.settings_set_theme(ThemeKind::Light, window, cx));
    cx.run_until_parked();
    draw(cx);
    let (kind, bg, expected_light) = cx.update(|_window, cx| {
        (theme::kind(cx), Theme::current(cx).background, Theme::light(cx).background)
    });
    assert_eq!(kind, ThemeKind::Light);
    assert_eq!(bg, expected_light);

    view.update_in(cx, |v, window, cx| v.settings_set_theme(ThemeKind::Dark, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(cx.update(|_window, cx| theme::kind(cx)), ThemeKind::Dark);
    assert_eq!(
        cx.update(|_window, cx| Theme::current(cx).background),
        cx.update(|_window, cx| Theme::dark(cx).background)
    );

    view.update_in(cx, |v, window, cx| v.settings_set_theme(ThemeKind::Darker, window, cx));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_window, cx| Theme::current(cx).background),
        cx.update(|_window, cx| Theme::darker(cx).background)
    );
    view.update_in(cx, |v, window, cx| v.settings_set_theme(ThemeKind::Black, window, cx));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_window, cx| Theme::current(cx).background),
        cx.update(|_window, cx| Theme::black(cx).background)
    );

    // Switches mirror notifier.enabled, notifier.sounds_enabled, autostart, and close_to_tray
    view.update(cx, |v, cx| {
        v.settings_set_notifications(false, cx);
        v.settings_set_sounds(false, cx);
        v.settings_set_autostart(true, cx);
        v.settings_set_close_to_tray(false, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(!v.notifier.enabled);
        assert!(!v.notifier.sounds_enabled);
        assert!(!v.prefs.data.notifications);
        assert!(!v.prefs.data.sounds);
        assert!(v.prefs.data.autostart);
        assert!(!v.prefs.data.close_to_tray);
    });

    // Switch locale to Korean
    view.update(cx, |v, cx| v.settings_set_locale("ko", cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(l10n::current().locale(), "ko");
    assert_eq!(l10n::tr("Log out"), "로그아웃");
    assert_eq!(
        lock_sent(&backend.sent_settings)
            .iter()
            .filter_map(|p| p.theme.as_deref())
            .collect::<Vec<_>>(),
        vec!["light", "dark", "dark_legacy", "coal"]
    );
    assert_eq!(
        lock_sent(&backend.sent_settings)
            .iter()
            .filter_map(|p| p.locale.as_deref())
            .collect::<Vec<_>>(),
        vec!["ko"]
    );

    // Logout confirmation emits ClientEvent::LoggedOut
    view.update(cx, |v, cx| v.settings_prompt_logout(true, cx));
    cx.run_until_parked();
    draw(cx);
    let confirm_bounds = cx.debug_bounds("settings-logout-confirm").expect("confirm button drawn");
    cx.simulate_click(confirm_bounds.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    assert!(logged_out.load(Ordering::SeqCst));
    drop(rt);
}

#[gpui_kit::test]
fn settings_modal_updates_voice_devices_and_volumes(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);

    focus_composer(&view, cx);
    cx.simulate_keystrokes("ctrl-,");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::Settings(_))));

    let voice_tab = cx.debug_bounds("settings-tab-voice").expect("voice tab drawn");
    cx.simulate_click(voice_tab.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    // Default voice prefs
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_device, None);
        assert_eq!(v.prefs.data.voice_output_device, None);
        assert_eq!(v.prefs.data.voice_input_volume, 100);
        assert_eq!(v.prefs.data.voice_output_volume, 100);
    });

    // Verify voice volume controls are drawn (reset buttons hidden at default 100)
    for selector in [
        "voice-input-vol-dec",
        "voice-input-vol-inc",
        "voice-output-vol-dec",
        "voice-output-vol-inc",
    ] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector} should be drawn");
    }
    for selector in ["voice-input-vol-reset", "voice-output-vol-reset"] {
        assert!(cx.debug_bounds(selector).is_none(), "{selector} should be hidden at 100");
    }

    // Step input/output volumes and verify clamping (0..=200) and reset (100)
    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceInputVolumeChanged(90), window, cx);
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceOutputVolumeChanged(110), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    for selector in ["voice-input-vol-reset", "voice-output-vol-reset"] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector} should be drawn when not 100");
    }
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_volume, 90);
        assert_eq!(v.prefs.data.voice_output_volume, 110);
    });

    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceInputVolumeChanged(0), window, cx);
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceOutputVolumeChanged(250), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_volume, 0);
        assert_eq!(v.prefs.data.voice_output_volume, 200);
    });

    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceInputVolumeChanged(100), window, cx);
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceOutputVolumeChanged(100), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_volume, 100);
        assert_eq!(v.prefs.data.voice_output_volume, 100);
    });

    // Set devices, verify reset buttons render, and reset back to None
    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(
            crate::settings_model::SettingsEffect::VoiceInputDeviceChanged(Some("USB Mic".to_owned())),
            window,
            cx,
        );
        v.apply_settings_effect(
            crate::settings_model::SettingsEffect::VoiceOutputDeviceChanged(Some("Headphones".to_owned())),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_device.as_deref(), Some("USB Mic"));
        assert_eq!(v.prefs.data.voice_output_device.as_deref(), Some("Headphones"));
    });
    assert!(cx.debug_bounds("voice-input-device-reset").is_some());
    assert!(cx.debug_bounds("voice-output-device-reset").is_some());

    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceInputDeviceChanged(None), window, cx);
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceOutputDeviceChanged(None), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_device, None);
        assert_eq!(v.prefs.data.voice_output_device, None);
    });
    assert!(cx.debug_bounds("voice-input-device-reset").is_none());
    assert!(cx.debug_bounds("voice-output-device-reset").is_none());

    drop(rt);
}

#[gpui_kit::test]
fn settings_modal_updates_reduced_motion(cx: &mut TestAppContext) {
    use crate::prefs::ReducedMotionPref;
    use crate::settings_model::SettingsEffect;

    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);

    focus_composer(&view, cx);
    cx.simulate_keystrokes("ctrl-,");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::Settings(_))));

    let a11y_tab = cx.debug_bounds("settings-tab-accessibility").expect("accessibility tab drawn");
    cx.simulate_click(a11y_tab.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    for selector in ["reduced-motion-system", "reduced-motion-on", "reduced-motion-off"] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector} should be drawn");
    }

    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(SettingsEffect::ReducedMotionChanged(ReducedMotionPref::On), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.update(|_window, cx| cx.reduce_motion()));
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.reduced_motion, ReducedMotionPref::On);
    });

    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(SettingsEffect::ReducedMotionChanged(ReducedMotionPref::Off), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(!cx.update(|_window, cx| cx.reduce_motion()));
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.reduced_motion, ReducedMotionPref::Off);
    });

    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(SettingsEffect::ReducedMotionChanged(ReducedMotionPref::System), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(!cx.update(|_window, cx| cx.reduce_motion()));
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.reduced_motion, ReducedMotionPref::System);
    });

    drop(rt);
}

#[gpui_kit::test]
fn settings_modal_selects_enumerated_voice_devices(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);

    focus_composer(&view, cx);
    cx.simulate_keystrokes("ctrl-,");
    cx.run_until_parked();
    view.update(cx, |v, cx| {
        v.select_settings_tab(SettingsTab::VoiceVideo, cx);
        v.audio_devices = crate::audio_devices::AudioDeviceLists {
            input_devices: vec!["Built-in Mic".to_owned(), "USB Mic".to_owned()],
            output_devices: vec!["Headphones".to_owned(), "Speakers".to_owned()],
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);

    for selector in [
        "voice-refresh-devices",
        "voice-input-device-default",
        "voice-input-device-0",
        "voice-input-device-1",
        "voice-output-device-default",
        "voice-output-device-0",
        "voice-output-device-1",
    ] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector} should be drawn");
    }

    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(
            crate::settings_model::SettingsEffect::VoiceInputDeviceChanged(Some("USB Mic".to_owned())),
            window,
            cx,
        );
        v.apply_settings_effect(
            crate::settings_model::SettingsEffect::VoiceOutputDeviceChanged(Some("Headphones".to_owned())),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);

    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_device.as_deref(), Some("USB Mic"));
        assert_eq!(v.prefs.data.voice_output_device.as_deref(), Some("Headphones"));
    });
    assert!(cx.debug_bounds("voice-input-device-reset").is_some());
    assert!(cx.debug_bounds("voice-output-device-reset").is_some());

    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(
            crate::settings_model::SettingsEffect::VoiceInputDeviceChanged(None),
            window,
            cx,
        );
        v.apply_settings_effect(
            crate::settings_model::SettingsEffect::VoiceOutputDeviceChanged(None),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);

    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_device, None);
        assert_eq!(v.prefs.data.voice_output_device, None);
    });
    assert!(cx.debug_bounds("voice-input-device-reset").is_none());
    assert!(cx.debug_bounds("voice-output-device-reset").is_none());

    drop(rt);
}

#[gpui_kit::test]
fn settings_modal_renders_all_update_statuses(cx: &mut TestAppContext) {
    use crate::updater::UpdateStatus;

    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);

    focus_composer(&view, cx);
    cx.simulate_keystrokes("ctrl-,");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::Settings(_))));

    let adv_tab = cx.debug_bounds("settings-tab-advanced").expect("advanced tab drawn");
    cx.simulate_click(adv_tab.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-check-updates").is_some());

    // Checking
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Checking;
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-checking").is_some());

    // UpToDate
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::UpToDate {
            current: "0.1.0".to_owned(),
            latest: "0.1.0".to_owned(),
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-uptodate").is_some());

    // Available without asset + Open release page only
    let release_url = "https://github.com/CJsw12/fluxer/releases/tag/v0.2.0".to_owned();
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Available {
            current: "0.1.0".to_owned(),
            latest: "0.2.0".to_owned(),
            release_url: release_url.clone(),
            asset_name: None,
            asset_url: None,
            checksum_url: None,
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-available").is_some());
    assert!(cx.debug_bounds("settings-download-update").is_none());
    assert!(cx.debug_bounds("settings-open-release").is_some());

    // Available with Windows asset + Download update button
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Available {
            current: "0.1.0".to_owned(),
            latest: "0.2.0".to_owned(),
            release_url,
            asset_name: Some("fluxer_client-windows-x86_64.zip".to_owned()),
            asset_url: Some(
                "https://github.com/CJsw12/fluxer/releases/download/v0.2.0/fluxer_client-windows-x86_64.zip"
                    .to_owned(),
            ),
            checksum_url: Some(
                "https://github.com/CJsw12/fluxer/releases/download/v0.2.0/SHA256SUMS.txt".to_owned(),
            ),
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-available").is_some());
    assert!(cx.debug_bounds("settings-download-update").is_some());
    assert!(cx.debug_bounds("settings-open-release").is_some());

    // Downloading
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Downloading {
            latest: "0.2.0".to_owned(),
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-downloading").is_some());

    // Downloaded (verified & unverified)
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Downloaded {
            latest: "0.2.0".to_owned(),
            path: "C:\\Users\\test\\AppData\\Local\\FluxerClient\\updates\\fluxer_client-windows-x86_64.zip"
                .to_owned(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
            verified: true,
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-downloaded").is_some());

    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Downloaded {
            latest: "0.2.0".to_owned(),
            path: "C:\\Users\\test\\AppData\\Local\\FluxerClient\\updates\\fluxer_client-windows-x86_64.zip"
                .to_owned(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
            verified: false,
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-downloaded").is_some());

    // Error
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Error("Network error".to_owned());
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-error").is_some());

    drop(rt);
}

#[gpui_kit::test]
fn settings_modal_switches_tabs_filters_search_and_closes(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);

    focus_composer(&view, cx);
    cx.simulate_keystrokes("ctrl-,");
    cx.run_until_parked();
    draw(cx);

    // Default tab is Appearance and renders the live preview card.
    view.read_with(cx, |v, _| match &v.overlay {
        Overlay::Settings(modal) => assert_eq!(modal.active_tab, SettingsTab::Appearance),
        _ => panic!("expected settings overlay"),
    });
    assert!(cx.debug_bounds("settings-appearance-preview").is_some());
    assert!(cx.debug_bounds("settings-density-compact").is_some());

    // Switch through tabs via sidebar clicks (including official Profile card, Privacy, Themes, Messages & Media, Keybinds, and sub-section tree).
    assert!(cx.debug_bounds("settings-subitem-theme").is_some(), "active Appearance tab expands sub-section tree");
    let profile_card = cx.debug_bounds("settings-profile-card").expect("clickable profile card drawn");
    cx.simulate_click(profile_card.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| match &v.overlay {
        Overlay::Settings(modal) => assert_eq!(modal.active_tab, SettingsTab::Profile),
        _ => panic!("expected settings overlay"),
    });
    assert!(cx.debug_bounds("settings-profile-customization").is_some());

    for (selector, expected_tab, body_selector) in [
        ("settings-tab-account", SettingsTab::Account, "settings-account-passkeys"),
        ("settings-tab-privacy", SettingsTab::PrivacySafety, "settings-privacy-dm"),
        ("settings-tab-themes", SettingsTab::Themes, "settings-theme-swatch-dark"),
        ("settings-tab-notifications", SettingsTab::Notifications, "settings-notifications"),
        ("settings-tab-chat", SettingsTab::MessagesMedia, "settings-chat-display"),
        ("settings-tab-voice", SettingsTab::VoiceVideo, "voice-refresh-devices"),
        ("settings-tab-accessibility", SettingsTab::Accessibility, "reduced-motion-system"),
        ("settings-tab-language", SettingsTab::Language, "settings-locale-list"),
        ("settings-tab-keybinds", SettingsTab::Keybinds, "settings-keybinds-list"),
        ("settings-tab-advanced", SettingsTab::Advanced, "settings-check-updates"),
        ("settings-tab-appearance", SettingsTab::Appearance, "settings-appearance-preview"),
    ] {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} drawn"));
        cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        view.read_with(cx, |v, _| match &v.overlay {
            Overlay::Settings(modal) => assert_eq!(modal.active_tab, expected_tab),
            _ => panic!("expected settings overlay"),
        });
        assert!(cx.debug_bounds(body_selector).is_some(), "{body_selector} should be drawn for {selector}");
    }

    // Search filters sidebar tabs and displays matching sections in the content pane.
    view.update_in(cx, |v, window, cx| {
        if let Overlay::Settings(modal) = &v.overlay {
            modal.search.update(cx, |input, cx| input.set_value("voice", window, cx));
        }
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-tab-voice").is_some());
    assert!(cx.debug_bounds("settings-tab-appearance").is_none());
    assert!(cx.debug_bounds("settings-tab-language").is_none());
    assert!(cx.debug_bounds("voice-refresh-devices").is_some());

    // Clearing search restores all tabs and the active tab view.
    view.update_in(cx, |v, window, cx| {
        if let Overlay::Settings(modal) = &v.overlay {
            modal.search.update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-tab-appearance").is_some());
    assert!(cx.debug_bounds("settings-tab-language").is_some());
    assert!(cx.debug_bounds("settings-appearance-preview").is_some());

    // Close button dismisses the modal.
    let close_bounds = cx.debug_bounds("settings-close").expect("settings close button drawn");
    cx.simulate_click(close_bounds.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    drop(rt);
}

#[gpui_kit::test]
fn appearance_theme_swatches_sliders_and_custom_titlebar_render(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, vcx) = open_client(cx, backend, events);

    view.update_in(vcx, |v, window, cx| v.open_settings(window, cx));
    vcx.run_until_parked();
    draw(vcx);

    assert!(vcx.debug_bounds("settings-open-themes-tab").is_some());
    assert!(vcx.debug_bounds("settings-font-slider").is_some());
    assert!(vcx.debug_bounds("settings-zoom-slider").is_some());

    let themes_link = vcx.debug_bounds("settings-open-themes-tab").expect("open themes link").center();
    vcx.simulate_click(themes_link, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);

    assert!(vcx.debug_bounds("settings-theme-swatch-dark").is_some());
    assert!(vcx.debug_bounds("settings-theme-swatch-light").is_some());
    assert!(vcx.debug_bounds("settings-theme-swatch-dark_legacy").is_some());
    assert!(vcx.debug_bounds("settings-theme-swatch-coal").is_some());
    assert!(vcx.debug_bounds("settings-theme-swatch-system").is_some());

    let coal = vcx.debug_bounds("settings-theme-swatch-coal").expect("coal swatch").center();
    vcx.simulate_click(coal, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert_eq!(vcx.update(|_, cx| crate::theme::kind(cx)), crate::theme::ThemeKind::Black);

    drop(rt);
}

#[gpui_kit::test]
fn account_tab_renders_passkeys_and_handles_credential_events(cx: &mut TestAppContext) {
    use fluxer_client_core::rest::WebAuthnCredential;

    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, vcx) = open_client(cx, backend, events);

    view.update_in(vcx, |v, window, cx| {
        v.open_settings(window, cx);
        v.select_settings_tab(crate::ui::settings::SettingsTab::Account, cx);
    });
    vcx.run_until_parked();
    draw(vcx);

    assert!(vcx.debug_bounds("settings-account-passkeys").is_some());
    assert!(vcx.debug_bounds("settings-passkey-add-btn").is_some());

    view.update(vcx, |v, cx| {
        v.handle_events(
            vec![crate::backend::UiEvent::WebAuthnCredentialsLoaded {
                result: Ok(vec![WebAuthnCredential {
                    id: "cred-test-1".to_owned(),
                    name: "YubiKey 5C".to_owned(),
                    created_at: Some("2026-11-23".to_owned()),
                    last_used_at: None,
                }]),
            }],
            cx,
        );
    });
    vcx.run_until_parked();
    draw(vcx);

    view.read_with(vcx, |v, _| {
        let Overlay::Settings(modal) = &v.overlay else {
            panic!("expected settings overlay");
        };
        assert!(!modal.passkey_loading);
        assert_eq!(modal.passkeys.len(), 1);
        assert_eq!(modal.passkeys[0].name, "YubiKey 5C");
    });

    drop(rt);
}

#[gpui_kit::test]
fn m5_user_area_popout_and_interactive_settings_prefs_work(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, vcx) = open_client(cx, backend, events);

    // Home button renders brand icon.
    assert!(vcx.debug_bounds("rail-home-icon").is_some());

    // Clicking user-panel-profile toggles the dedicated bottom-left UserAreaPopout.
    let profile_row = vcx.debug_bounds("user-panel-profile").expect("user-panel-profile").center();
    vcx.simulate_click(profile_row, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("user-area-popout").is_some());
    assert!(vcx.debug_bounds("user-area-status-row").is_some());
    assert!(vcx.debug_bounds("user-area-switch-accounts").is_some());
    assert!(vcx.debug_bounds("user-area-copy-user-id").is_some());
    // Hovering the status row opens the status submenu.
    view.update(vcx, |v, cx| {
        v.user_area_submenu = Some(crate::ui::user_panel::UserAreaSubmenu::Status);
        cx.notify();
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("user-area-status-online").is_some());
    assert!(vcx.debug_bounds("user-area-status-dnd").is_some());
    assert!(vcx.debug_bounds("user-area-edit-profile").is_some());

    // Clicking Edit Profile in UserAreaPopout closes the popout and opens Settings on Profile tab.
    let edit_prof = vcx.debug_bounds("user-area-edit-profile").unwrap().center();
    vcx.simulate_click(edit_prof, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("user-area-popout").is_none());
    view.read_with(vcx, |v, _| {
        let Overlay::Settings(modal) = &v.overlay else {
            panic!("expected settings overlay");
        };
        assert_eq!(modal.active_tab, crate::ui::settings::SettingsTab::Profile);
    });

    // Switch to Voice & Video tab and toggle Push to Talk + Noise Suppression.
    view.update(vcx, |v, cx| {
        v.select_settings_tab(crate::ui::settings::SettingsTab::VoiceVideo, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-input-mode-vad").is_some());
    assert!(vcx.debug_bounds("settings-input-mode-ptt").is_some());

    let custom_btn = vcx.debug_bounds("settings-voice-profile-custom").unwrap().center();
    vcx.simulate_click(custom_btn, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-noise-suppression").is_some());

    let ptt_btn = vcx.debug_bounds("settings-input-mode-ptt").unwrap().center();
    vcx.simulate_click(ptt_btn, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.prefs.data.voice_push_to_talk);
    });

    drop(rt);
}

#[gpui_kit::test]
fn m7_guild_rail_hover_indicator_and_full_video_voice_settings_work(cx: &mut TestAppContext) {
    use crate::client_view::RailHoverTarget;

    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, vcx) = open_client(cx, backend, events);

    // Home is selected by default -> 40px indicator pill on home, none on favorites.
    assert!(vcx.debug_bounds("rail-pill-home").is_some());
    assert!(vcx.debug_bounds("rail-pill-favorites").is_none());

    // Hovering Favorites shows its 20px indicator pill!
    view.update(vcx, |v, cx| {
        v.set_rail_hover(RailHoverTarget::Favorites, true, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    let fav_pill_hover = vcx.debug_bounds("rail-pill-favorites").expect("hovered favorites pill");
    assert!((f32::from(fav_pill_hover.size.height) - 20.0).abs() < 0.5);

    // Clicking Favorites selects it -> 40px indicator pill on favorites!
    let fav_btn = vcx.debug_bounds("rail-favorites-btn").expect("favorites button").center();
    vcx.simulate_click(fav_btn, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    let fav_pill_sel = vcx.debug_bounds("rail-pill-favorites").expect("selected favorites pill");
    assert!((f32::from(fav_pill_sel.size.height) - 40.0).abs() < 0.5);

    // Hovering Home while Favorites is selected shows 20px indicator pill on Home!
    view.update(vcx, |v, cx| {
        v.set_rail_hover(RailHoverTarget::Home, true, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    let home_pill_hover = vcx.debug_bounds("rail-pill-home").expect("hovered home pill");
    assert!((f32::from(home_pill_hover.size.height) - 20.0).abs() < 0.5);

    // Open Settings -> Voice & Video and exercise video codec, resolution, fps, bitrate, voice profile, mic test.
    view.update_in(vcx, |v, window, cx| {
        v.open_settings(window, cx);
        v.select_settings_tab(crate::ui::settings::SettingsTab::VoiceVideo, cx);
    });
    vcx.run_until_parked();
    draw(vcx);

    for sel in [
        "settings-voice-profile-custom",
        "settings-auto-gain-control",
        "settings-mic-test-btn",
        "settings-mic-test-meter",
        "settings-camera-res-1080p",
        "settings-mirror-camera",
        "settings-video-res-1440p",
        "settings-video-res-source",
        "settings-video-fps-30",
        "settings-video-codec-h265",
        "settings-video-codec-av1",
        "settings-video-bitrate-8000",
        "settings-video-hw-accel",
    ] {
        assert!(vcx.debug_bounds(sel).is_some(), "{sel} must be rendered in Voice & Video tab");
    }

    for pre_sel in ["settings-voice-profile-custom", "settings-input-mode-ptt"] {
        let pt = vcx.debug_bounds(pre_sel).expect(pre_sel).center();
        vcx.simulate_click(pt, gpui_kit::Modifiers::default());
        vcx.run_until_parked();
        draw(vcx);
    }
    for sel in [
        "settings-ns-backend-deepfilternet",
        "settings-echo-cancellation",
        "settings-ptt-delay-500",
    ] {
        assert!(vcx.debug_bounds(sel).is_some(), "{sel} must be rendered in custom PTT mode");
    }

    for (section_sel, click_sels) in [
        (
            "settings-subitem-audio",
            &[
                "settings-ptt-delay-500",
                "settings-voice-profile-custom",
                "settings-ns-backend-deepfilternet",
                "settings-mic-test-btn",
            ][..],
        ),
        (
            "settings-subitem-video",
            &[
                "settings-camera-res-1080p",
                "settings-video-res-1440p",
                "settings-video-fps-30",
                "settings-video-codec-h265",
                "settings-video-bitrate-8000",
            ][..],
        ),
    ] {
        let sec_pt = vcx.debug_bounds(section_sel).expect(section_sel).center();
        vcx.simulate_click(sec_pt, gpui_kit::Modifiers::default());
        vcx.run_until_parked();
        draw(vcx);
        for &click_sel in click_sels {
            if click_sel == "settings-ptt-delay-500" || click_sel == "settings-mic-test-btn" {
                let scroll_pos = vcx.debug_bounds("settings-voice-profile-custom").expect("scroll anchor").center();
                vcx.simulate_event(gpui_kit::ScrollEvent {
                    position: scroll_pos,
                    delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-400.))),
                    ..Default::default()
                });
                vcx.run_until_parked();
                draw(vcx);
            }
            let pt = vcx.debug_bounds(click_sel).expect(click_sel).center();
            vcx.simulate_click(pt, gpui_kit::Modifiers::default());
            vcx.run_until_parked();
            draw(vcx);
            if click_sel == "settings-mic-test-btn" {
                vcx.simulate_event(gpui_kit::ScrollEvent {
                    position: pt,
                    delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(800.))),
                    ..Default::default()
                });
                vcx.run_until_parked();
                draw(vcx);
            }
        }
    }

    view.read_with(vcx, |v, _| {
        assert_eq!(v.prefs.data.voice_processing_mode, "custom");
        assert_eq!(v.prefs.data.voice_noise_suppression_backend, "deepfilternet");
        assert_eq!(v.prefs.data.voice_ptt_release_delay_ms, 500);
        assert!(v.mic_test_active);
        assert_eq!(v.prefs.data.camera_resolution, "1080p");
        assert_eq!(v.prefs.data.video_resolution, "1440p");
        assert_eq!(v.prefs.data.video_fps, 30);
        assert_eq!(v.prefs.data.video_codec, "h265");
        assert_eq!(v.prefs.data.video_bitrate_kbps, 8000);
    });

    drop(rt);
}

#[gpui_kit::test]
fn settings_modal_new_tabs_deeplinks_search_and_privacy_work(cx: &mut TestAppContext) {
    use fluxer_client_core::rest::{DeveloperApplication, DeveloperApplicationBot, UserConnection};

    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, vcx) = open_client(cx, backend.clone(), events);

    // Deep link: fluxer://scene/settings/windows -> WindowsApp tab
    view.update_in(vcx, |v, window, cx| {
        assert!(v.apply_scene_deep_link("settings/windows", window, cx));
    });
    vcx.run_until_parked();
    draw(vcx);
    view.read_with(vcx, |v, _| match &v.overlay {
        Overlay::Settings(modal) => assert_eq!(modal.active_tab, SettingsTab::WindowsApp),
        _ => panic!("expected settings overlay"),
    });
    assert!(vcx.debug_bounds("settings-windows-app-tab").is_some());
    assert!(vcx.debug_bounds("settings-windows-autostart").is_some());
    assert!(vcx.debug_bounds("settings-windows-close-to-tray").is_some());

    // Deep link: fluxer://scene/settings/connections -> Connections tab + loaded event rendering
    view.update_in(vcx, |v, window, cx| {
        assert!(v.apply_scene_deep_link("settings/connections", window, cx));
    });
    vcx.run_until_parked();
    draw(vcx);
    view.read_with(vcx, |v, _| match &v.overlay {
        Overlay::Settings(modal) => assert_eq!(modal.active_tab, SettingsTab::Connections),
        _ => panic!("expected settings overlay"),
    });
    assert!(vcx.debug_bounds("settings-connections-tab").is_some());

    view.update(vcx, |v, cx| {
        v.handle_events(
            vec![crate::backend::UiEvent::ConnectionsLoaded {
                result: Ok(vec![UserConnection {
                    id: "conn-1".to_owned(),
                    kind: "domain".to_owned(),
                    name: "fluxer.dev".to_owned(),
                    verified: true,
                    visibility_flags: 1,
                    sort_order: 0,
                    provider_id: None,
                    url: Some("https://fluxer.dev".to_owned()),
                }]),
            }],
            cx,
        );
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-connection-card-conn-1").is_some());

    // Deep link: fluxer://scene/settings/applications -> Applications tab + detail view
    view.update_in(vcx, |v, window, cx| {
        assert!(v.apply_scene_deep_link("settings/applications", window, cx));
    });
    vcx.run_until_parked();
    draw(vcx);
    view.read_with(vcx, |v, _| match &v.overlay {
        Overlay::Settings(modal) => assert_eq!(modal.active_tab, SettingsTab::Applications),
        _ => panic!("expected settings overlay"),
    });
    assert!(vcx.debug_bounds("settings-applications-tab").is_some());
    assert!(vcx.debug_bounds("settings-applications-create").is_some());
    assert!(vcx.debug_bounds("settings-applications-docs").is_some());

    view.update(vcx, |v, cx| {
        v.handle_events(
            vec![crate::backend::UiEvent::ApplicationsLoaded {
                result: Ok(vec![DeveloperApplication {
                    id: "700000000000000001".to_owned(),
                    name: "My Test Bot".to_owned(),
                    redirect_uris: vec!["https://example.org/cb".to_owned()],
                    bot_public: true,
                    bot_require_code_grant: false,
                    bot: Some(DeveloperApplicationBot {
                        id: "700000000000000002".to_owned(),
                        username: "testbot".to_owned(),
                        discriminator: "0001".to_owned(),
                        avatar: None,
                        bio: None,
                        banner: None,
                        flags: None,
                    }),
                }]),
            }],
            cx,
        );
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-application-item-700000000000000001").is_some());

    view.update(vcx, |v, cx| {
        v.settings_select_application("700000000000000001".to_owned(), cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-application-detail").is_some());
    assert!(vcx.debug_bounds("settings-application-back").is_some());

    // Privacy tab renders enhanced sections and sends non-destructive privacy patches
    view.update(vcx, |v, cx| {
        v.select_settings_tab(SettingsTab::PrivacySafety, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    for sel in [
        "settings-profile-privacy-0",
        "settings-profile-privacy-2",
        "settings-allow-guild-dms",
        "settings-allow-bot-dms",
        "settings-incoming-call-8",
        "settings-group-dm-add-16",
        "settings-sensitive-friend-1",
        "settings-sensitive-other-2",
        "settings-sensitive-guild-1",
        "settings-privacy-data-export",
        "settings-privacy-data-deletion",
    ] {
        assert!(vcx.debug_bounds(sel).is_some(), "{sel} should be drawn in Privacy tab");
    }

    // Search filter finds Applications, Connections, and Windows app tabs
    for (query, expected_sel, hidden_sel) in [
        ("applications", "settings-tab-applications", "settings-tab-appearance"),
        ("connections", "settings-tab-connections", "settings-tab-language"),
        ("windows", "settings-tab-windows", "settings-tab-keybinds"),
    ] {
        view.update_in(vcx, |v, window, cx| {
            if let Overlay::Settings(modal) = &v.overlay {
                modal.search.update(cx, |input, cx| input.set_value(query, window, cx));
            }
            cx.notify();
        });
        vcx.run_until_parked();
        draw(vcx);
        assert!(vcx.debug_bounds(expected_sel).is_some(), "{expected_sel} should match query '{query}'");
        assert!(vcx.debug_bounds(hidden_sel).is_none(), "{hidden_sel} should be filtered out for '{query}'");
    }

    drop(rt);
}

#[gpui_kit::test]
fn keybinds_tab_editing_recording_conflicts_search_reset_and_runtime_dispatch(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, vcx) = open_client(cx, backend, events);
    vcx.update(|_, cx| crate::shortcuts::bind_keys(cx));

    // Open settings and switch to Keybinds tab.
    view.update_in(vcx, |v, window, cx| {
        v.open_settings(window, cx);
        v.select_settings_tab(SettingsTab::Keybinds, cx);
    });
    vcx.run_until_parked();
    draw(vcx);

    assert!(vcx.debug_bounds("settings-keybinds-list").is_some());
    assert!(vcx.debug_bounds("settings-keybinds-search").is_some());
    assert!(vcx.debug_bounds("settings-keybinds-add").is_some());
    assert!(vcx.debug_bounds("settings-keybinds-disable-builtins").is_some());
    assert!(vcx.debug_bounds("settings-keybinds-custom-empty").is_some());
    assert!(vcx.debug_bounds("settings-keybinds-reset-all").is_none());

    // Add a custom keybind via the Add button.
    let add_btn = vcx.debug_bounds("settings-keybinds-add").expect("add button drawn");
    vcx.simulate_click(add_btn.center(), gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);

    assert!(vcx.debug_bounds("settings-keybind-custom-row-kb-1").is_some());
    assert!(vcx.debug_bounds("settings-keybinds-reset-all").is_some());

    // Select action "nav_quick_switcher" from the dropdown.
    let action_btn = vcx
        .debug_bounds("settings-keybind-action-kb-1")
        .expect("action dropdown drawn");
    vcx.simulate_click(action_btn.center(), gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-keybind-action-menu-kb-1").is_some());
    view.update(vcx, |v, cx| {
        v.settings_set_custom_keybind_action("kb-1", Some("nav_quick_switcher".to_owned()), cx);
    });
    vcx.run_until_parked();
    draw(vcx);

    // Start recording and press Escape -> cancels recording without closing settings.
    view.update_in(vcx, |v, window, cx| {
        v.settings_toggle_record_keybind("kb-1".to_owned(), window, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(view.read_with(vcx, |v, _| v.is_recording_keybind()));

    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    draw(vcx);
    assert!(!view.read_with(vcx, |v, _| v.is_recording_keybind()));
    assert!(view.read_with(vcx, |v, _| matches!(v.overlay, Overlay::Settings(_))));

    // Start recording and press Ctrl+K -> pauses built-in QuickSwitcher and records "ctrl-k".
    view.update_in(vcx, |v, window, cx| {
        v.settings_toggle_record_keybind("kb-1".to_owned(), window, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    vcx.simulate_keystrokes("ctrl-k");
    vcx.run_until_parked();
    draw(vcx);
    assert!(!view.read_with(vcx, |v, _| v.is_recording_keybind()));
    assert!(view.read_with(vcx, |v, _| matches!(v.overlay, Overlay::Settings(_))));
    view.read_with(vcx, |v, _| {
        assert_eq!(v.prefs.data.custom_keybinds[0].combo, "ctrl-k");
    });

    // Re-record to Alt+J for runtime dispatch test.
    view.update_in(vcx, |v, window, cx| {
        v.settings_toggle_record_keybind("kb-1".to_owned(), window, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    vcx.simulate_keystrokes("alt-j");
    vcx.run_until_parked();
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.prefs.data.custom_keybinds[0].combo, "alt-j");
    });

    // Add a second custom keybind with the same combo -> conflict badge appears.
    view.update(vcx, |v, cx| {
        let id2 = v.settings_add_custom_keybind(Some("misc_search".to_owned()), cx);
        v.settings_set_custom_keybind_combo(&id2, "alt-j".to_owned(), cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-keybind-conflict-kb-1").is_some());
    assert!(vcx.debug_bounds("settings-keybind-conflict-kb-2").is_some());

    // Disabling kb-2 clears the conflict badge.
    view.update(vcx, |v, cx| {
        v.settings_set_custom_keybind_enabled("kb-2", false, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-keybind-conflict-kb-1").is_none());

    // Real-time search filters custom and default keybinds.
    view.update_in(vcx, |v, window, cx| {
        if let Overlay::Settings(modal) = &v.overlay {
            modal.keybinds_search.update(cx, |input, cx| input.set_value("zzzz-no-match", window, cx));
        }
        cx.notify();
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-keybinds-custom-no-match").is_some());
    assert!(vcx.debug_bounds("settings-keybinds-defaults-no-match").is_some());

    view.update_in(vcx, |v, window, cx| {
        if let Overlay::Settings(modal) = &v.overlay {
            modal.keybinds_search.update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-keybind-custom-row-kb-1").is_some());

    // Close settings and verify runtime shortcut execution:
    // 1. Built-in ToggleQuickSwitcher is overridden by kb-1, so dispatching it does nothing.
    view.update_in(vcx, |v, window, cx| {
        v.close_overlay(window, cx);
        v.focus_root(window, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(view.read_with(vcx, |v, _| matches!(v.overlay, Overlay::None)));
    vcx.dispatch_action(crate::shortcuts::ToggleQuickSwitcher);
    vcx.run_until_parked();
    draw(vcx);
    assert!(
        view.read_with(vcx, |v, _| matches!(v.overlay, Overlay::None)),
        "overridden built-in ToggleQuickSwitcher should be suppressed"
    );

    // 2. Custom combo Alt+J opens QuickSwitcher.
    vcx.simulate_keystrokes("alt-j");
    vcx.run_until_parked();
    draw(vcx);
    assert!(
        view.read_with(vcx, |v, _| matches!(v.overlay, Overlay::QuickSwitcher(_))),
        "custom keybind alt-j should trigger nav_quick_switcher"
    );

    // 3. Disable built-ins suppresses built-in OpenSearch.
    view.update_in(vcx, |v, window, cx| {
        v.close_overlay(window, cx);
        v.settings_set_disable_builtin_keybinds(true, cx);
        v.focus_root(window, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    vcx.dispatch_action(crate::shortcuts::OpenSearch);
    vcx.run_until_parked();
    draw(vcx);
    assert!(view.read_with(vcx, |v, _| v.search.is_none()));

    // 4. Reset to defaults confirmation modal resets custom keybinds and disable_builtin_keybinds.
    view.update_in(vcx, |v, window, cx| {
        v.open_settings(window, cx);
        v.select_settings_tab(SettingsTab::Keybinds, cx);
        v.settings_set_keybinds_confirm_reset(true, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("settings-keybinds-reset-confirm").is_some());
    let confirm_btn = vcx
        .debug_bounds("settings-keybinds-reset-confirm-btn")
        .expect("confirm reset button drawn");
    vcx.simulate_click(confirm_btn.center(), gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.prefs.data.custom_keybinds.is_empty());
        assert!(!v.prefs.data.disable_builtin_keybinds);
        assert!(v.prefs.data.disabled_builtin_actions.is_empty());
    });
    assert!(vcx.debug_bounds("settings-keybinds-custom-empty").is_some());

    drop(rt);
}

#[gpui_kit::test]
fn settings_modal_theme_tokens_and_rendering_across_dark_and_light(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, vcx) = open_client(cx, backend, events);

    let (dark, light) = vcx.update(|_window, cx| (Theme::dark(cx), Theme::light(cx)));

    // Dark/light theme tokens used by settings modal and overlay must be distinct and ordered.
    assert_ne!(dark.background_tertiary, light.background_tertiary);
    assert_ne!(dark.background_header_secondary, light.background_header_secondary);
    assert_ne!(dark.background_secondary, light.background_secondary);
    assert_ne!(dark.background_secondary_lighter, light.background_secondary_lighter);
    assert_ne!(dark.background_modifier_accent, light.background_modifier_accent);
    assert_ne!(dark.foreground, light.foreground);
    assert_ne!(dark.muted_foreground, light.muted_foreground);
    assert_ne!(dark.text_tertiary, light.text_tertiary);
    assert_ne!(dark.hover_bg, light.hover_bg);
    assert_ne!(dark.divider, light.divider);

    for theme in [&dark, &light] {
        assert_ne!(theme.background_tertiary, theme.background_secondary);
        assert_ne!(theme.background_secondary_lighter, theme.background_tertiary);
        assert!(theme.background_modifier_accent.a > 0.0);
        assert_ne!(theme.foreground, theme.muted_foreground);
        assert_ne!(theme.muted_foreground, theme.text_tertiary);
        assert!(theme.on_fill.l > 0.9);
    }
    // Dark sidebar (background_tertiary) must remain lighter than content (background_secondary).
    assert!(dark.background_tertiary.l > dark.background_secondary.l);
    assert!(dark.background_header_secondary.l < light.background_header_secondary.l);

    // Render settings modal in Dark mode and verify core settings surfaces draw cleanly.
    view.update_in(vcx, |v, window, cx| {
        v.open_settings(window, cx);
        v.select_settings_tab(SettingsTab::Appearance, cx);
        v.settings_set_theme(ThemeKind::Dark, window, cx);
    });
    vcx.run_until_parked();
    draw(vcx);
    assert_eq!(vcx.update(|_window, cx| theme::kind(cx)), ThemeKind::Dark);
    for selector in [
        "settings-close",
        "settings-search",
        "settings-profile-card",
        "settings-logout",
        "settings-density-cozy",
        "settings-font-16",
    ] {
        assert!(
            vcx.debug_bounds(selector).is_some(),
            "expected {selector} to render in Dark theme"
        );
    }

    // Switch to Light mode and verify settings modal re-renders with light tokens.
    view.update_in(vcx, |v, window, cx| v.settings_set_theme(ThemeKind::Light, window, cx));
    vcx.run_until_parked();
    draw(vcx);
    assert_eq!(vcx.update(|_window, cx| theme::kind(cx)), ThemeKind::Light);
    assert_eq!(
        vcx.update(|_window, cx| Theme::current(cx).background_tertiary),
        light.background_tertiary
    );
    for selector in [
        "settings-close",
        "settings-search",
        "settings-profile-card",
        "settings-logout",
        "settings-density-cozy",
        "settings-font-16",
    ] {
        assert!(
            vcx.debug_bounds(selector).is_some(),
            "expected {selector} to render in Light theme"
        );
    }

    drop(rt);
}
