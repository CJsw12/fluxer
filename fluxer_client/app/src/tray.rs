//! System tray icon, context menu, and close-to-tray policy (Windows).

use std::sync::atomic::{AtomicBool, Ordering};

static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(true);
static TRAY_ACTIVE: AtomicBool = AtomicBool::new(false);
static QUITTING: AtomicBool = AtomicBool::new(false);

pub fn set_close_to_tray(enabled: bool) {
    CLOSE_TO_TRAY.store(enabled, Ordering::SeqCst);
}

pub fn close_to_tray() -> bool {
    CLOSE_TO_TRAY.load(Ordering::SeqCst)
}

pub fn is_active() -> bool {
    TRAY_ACTIVE.load(Ordering::SeqCst)
}

pub fn request_quit() {
    QUITTING.store(true, Ordering::SeqCst);
}

pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Whether `on_window_should_close` should allow the window to close (`true`)
/// or minimize to tray and keep the process alive (`false`).
pub fn should_allow_window_close(close_to_tray: bool, tray_active: bool, quitting: bool) -> bool {
    quitting || !close_to_tray || !tray_active
}

pub fn should_intercept_close() -> bool {
    !should_allow_window_close(close_to_tray(), is_active(), is_quitting())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ShowWindow,
    OpenSettings,
    ToggleNotifications,
    Quit,
}

/// Generates a 32x32 RGBA fallback icon (`#4641D9` Fluxer brand circle) when
/// no embedded Win32 icon resource is available (e.g., dev runs or non-res exe).
pub fn fallback_rgba_icon() -> (Vec<u8>, u32, u32) {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as i32 - 16;
            let dy = y as i32 - 16;
            if dx * dx + dy * dy <= 14 * 14 {
                rgba.extend_from_slice(&[0x46, 0x41, 0xd9, 0xff]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    (rgba, SIZE, SIZE)
}

#[cfg(target_os = "windows")]
mod imp {
    use std::sync::atomic::Ordering;

    use gpui_kit::{AnyWindowHandle, App, AsyncApp, Global, WeakEntity};
    use tokio::sync::mpsc;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::{TRAY_ACTIVE, TrayAction, fallback_rgba_icon, request_quit};
    use crate::platform;
    use crate::root::{AppRoot, Screen};
    use crate::ui::l10n;
    use crate::ui::overlay::Overlay;

    struct HeldTray {
        _tray: TrayIcon,
    }
    impl Global for HeldTray {}

    impl Drop for HeldTray {
        fn drop(&mut self) {
            TRAY_ACTIVE.store(false, Ordering::SeqCst);
        }
    }

    fn load_icon() -> Option<Icon> {
        if let Ok(icon) = Icon::from_resource(1, Some((32, 32))) {
            return Some(icon);
        }
        let (rgba, width, height) = fallback_rgba_icon();
        Icon::from_rgba(rgba, width, height).ok()
    }

    pub fn install(window_handle: AnyWindowHandle, root: WeakEntity<AppRoot>, cx: &mut App) {
        if super::is_active() {
            return;
        }
        let menu = Menu::new();
        let open_item = MenuItem::new(
            l10n::tr_args("Open {productName}", &[("productName", platform::APP_DISPLAY_NAME)]),
            true,
            None,
        );
        let settings_item = MenuItem::new(l10n::tr("Settings"), true, None);
        let mute_item = MenuItem::new(l10n::tr("Mute notifications"), true, None);
        let quit_item = MenuItem::new(
            l10n::tr_args("Quit {appName}", &[("appName", platform::APP_DISPLAY_NAME)]),
            true,
            None,
        );

        let open_id = open_item.id().clone();
        let settings_id = settings_item.id().clone();
        let mute_id = mute_item.id().clone();
        let quit_id = quit_item.id().clone();

        if let Err(err) = menu
            .append(&open_item)
            .and_then(|()| menu.append(&settings_item))
            .and_then(|()| menu.append(&mute_item))
            .and_then(|()| menu.append(&PredefinedMenuItem::separator()))
            .and_then(|()| menu.append(&quit_item))
        {
            tracing::warn!(%err, "could not build the tray context menu");
            return;
        }

        let Some(icon) = load_icon() else {
            tracing::warn!("could not create the tray icon image");
            return;
        };

        let tray = match TrayIconBuilder::new()
            .with_tooltip(platform::APP_DISPLAY_NAME)
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
        {
            Ok(tray) => tray,
            Err(err) => {
                tracing::warn!(%err, "could not install the system tray icon");
                return;
            }
        };

        TRAY_ACTIVE.store(true, Ordering::SeqCst);
        cx.set_global(HeldTray { _tray: tray });

        let (tx, mut rx) = mpsc::unbounded_channel::<TrayAction>();
        let click_tx = tx.clone();
        TrayIconEvent::set_event_handler(Some(move |event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                let _ = click_tx.send(TrayAction::ShowWindow);
            }
            _ => {}
        }));

        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = if event.id == open_id {
                Some(TrayAction::ShowWindow)
            } else if event.id == settings_id {
                Some(TrayAction::OpenSettings)
            } else if event.id == mute_id {
                Some(TrayAction::ToggleNotifications)
            } else if event.id == quit_id {
                Some(TrayAction::Quit)
            } else {
                None
            };
            if let Some(action) = action {
                let _ = tx.send(action);
            }
        }));

        cx.spawn(async move |cx: &mut AsyncApp| {
            while let Some(action) = rx.recv().await {
                handle_action(action, window_handle, &root, cx);
            }
        })
        .detach();
    }

    fn handle_action(
        action: TrayAction,
        window_handle: AnyWindowHandle,
        root: &WeakEntity<AppRoot>,
        cx: &mut AsyncApp,
    ) {
        match action {
            TrayAction::ShowWindow => {
                let _ = window_handle.update(cx, |_, window, _| window.activate_window());
            }
            TrayAction::OpenSettings => {
                let _ = window_handle.update(cx, |_, window, cx| {
                    window.activate_window();
                    let _ = root.update(cx, |app_root, cx| {
                        if let Screen::Client(client) = &app_root.screen {
                            client.update(cx, |view, cx| {
                                if !matches!(view.overlay, Overlay::Settings(_)) {
                                    view.open_settings(window, cx);
                                }
                            });
                        }
                    });
                });
            }
            TrayAction::ToggleNotifications => {
                let _ = cx.update(|cx| {
                    let _ = root.update(cx, |app_root, cx| {
                        if let Screen::Client(client) = &app_root.screen {
                            client.update(cx, |view, cx| {
                                let next = !view.notifier.enabled;
                                view.settings_set_notifications(next, cx);
                            });
                        }
                    });
                });
            }
            TrayAction::Quit => {
                request_quit();
                let _ = cx.update(|cx| cx.quit());
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::install;

#[cfg(not(target_os = "windows"))]
pub fn install(
    _window_handle: gpui_kit::AnyWindowHandle,
    _root: gpui_kit::WeakEntity<crate::root::AppRoot>,
    _cx: &mut gpui_kit::App,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_policy_respects_quit_flag_and_tray_state() {
        for (close_to_tray, tray_active, quitting, expected) in [
            (false, false, false, true),
            (false, true, false, true),
            (true, false, false, true),
            (true, true, false, false),
            (false, false, true, true),
            (false, true, true, true),
            (true, false, true, true),
            (true, true, true, true),
        ] {
            assert_eq!(
                should_allow_window_close(close_to_tray, tray_active, quitting),
                expected,
                "close_to_tray={close_to_tray}, tray_active={tray_active}, quitting={quitting}"
            );
        }
    }

    #[test]
    fn fallback_rgba_icon_has_expected_dimensions_and_circle() {
        let (rgba, width, height) = fallback_rgba_icon();
        assert_eq!((width, height), (32, 32));
        assert_eq!(rgba.len(), 32 * 32 * 4);
        // Center pixel (16, 16) is opaque Fluxer brand indigo (#4641D9).
        let center = (16 * 32 + 16) * 4;
        assert_eq!(&rgba[center..center + 4], &[0x46, 0x41, 0xd9, 0xff]);
        // Corner pixel (0, 0) is transparent.
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
    }
}
