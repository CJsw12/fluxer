//! Headless UI tests for the minimal settings modal.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::TestAppContext;

use crate::client_view::ClientEvent;
use crate::tests::{backend_with_fixture, draw, focus_composer, open_client, runtime};
use crate::theme::{self, Theme, ThemeKind};
use crate::ui::l10n;
use crate::ui::overlay::Overlay;
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

    // Default voice prefs
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.voice_input_device, None);
        assert_eq!(v.prefs.data.voice_output_device, None);
        assert_eq!(v.prefs.data.voice_input_volume, 100);
        assert_eq!(v.prefs.data.voice_output_volume, 100);
    });

    // Verify all voice volume controls are drawn
    for selector in [
        "voice-input-vol-dec",
        "voice-input-vol-inc",
        "voice-input-vol-reset",
        "voice-output-vol-dec",
        "voice-output-vol-inc",
        "voice-output-vol-reset",
    ] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector} should be drawn");
    }

    // Step input/output volumes and verify clamping (0..=200) and reset (100)
    view.update_in(cx, |v, window, cx| {
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceInputVolumeChanged(90), window, cx);
        v.apply_settings_effect(crate::settings_model::SettingsEffect::VoiceOutputVolumeChanged(110), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
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

    // Available + Open release page click
    let release_url = "https://github.com/CJsw12/fluxer/releases/tag/v0.2.0".to_owned();
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Available {
            current: "0.1.0".to_owned(),
            latest: "0.2.0".to_owned(),
            release_url: release_url.clone(),
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("settings-update-available").is_some());
    assert!(cx.debug_bounds("settings-open-release").is_some());

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
