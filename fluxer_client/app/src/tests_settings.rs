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

    // Switches mirror notifier.enabled and notifier.sounds_enabled
    view.update(cx, |v, cx| {
        v.settings_set_notifications(false, cx);
        v.settings_set_sounds(false, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(!v.notifier.enabled);
        assert!(!v.notifier.sounds_enabled);
        assert!(!v.prefs.data.notifications);
        assert!(!v.prefs.data.sounds);
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
