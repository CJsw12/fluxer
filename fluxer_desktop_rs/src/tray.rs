// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tao::window::Window;
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

static TRAY_INITIALIZED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayMenuAction {
    None,
    ToggleWindow,
    OpenSettings,
    ToggleOverlay,
    SetStatus(String),
    CheckForUpdates,
    Restart,
    Quit,
}

pub struct TrayManager {
    _tray_icon: TrayIcon,
    toggle_item: MenuItem,
    open_settings_id: MenuId,
    overlay_item_id: MenuId,
    status_online_item: CheckMenuItem,
    status_idle_item: CheckMenuItem,
    status_dnd_item: CheckMenuItem,
    status_invisible_item: CheckMenuItem,
    check_updates_id: MenuId,
    restart_id: MenuId,
    quit_id: MenuId,
}

impl TrayManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();

        // 1. Hide Fluxer / Show Fluxer & Overlay
        let toggle_item = MenuItem::new("Hide Fluxer", true, None);
        let open_settings_item = MenuItem::new("Open settings", true, None);
        let overlay_item = MenuItem::new("Voice Overlay (GPUI)", true, None);

        let open_settings_id = open_settings_item.id().clone();
        let overlay_item_id = overlay_item.id().clone();

        menu.append(&toggle_item)?;
        menu.append(&open_settings_item)?;
        menu.append(&overlay_item)?;
        menu.append(&PredefinedMenuItem::separator())?;

        // 2. Status 서브메뉴
        let status_submenu = Submenu::new("Status", true);
        let status_online_item = CheckMenuItem::new("Online", true, true, None);
        let status_idle_item = CheckMenuItem::new("Idle", true, false, None);
        let status_dnd_item = CheckMenuItem::new("Do not disturb", true, false, None);
        let status_invisible_item = CheckMenuItem::new("Invisible", true, false, None);

        status_submenu.append(&status_online_item)?;
        status_submenu.append(&status_idle_item)?;
        status_submenu.append(&status_dnd_item)?;
        status_submenu.append(&status_invisible_item)?;

        menu.append(&status_submenu)?;
        menu.append(&PredefinedMenuItem::separator())?;

        // 3. Check for updates
        let check_updates_item = MenuItem::new("Check for updates", true, None);
        let check_updates_id = check_updates_item.id().clone();
        menu.append(&check_updates_item)?;
        menu.append(&PredefinedMenuItem::separator())?;

        // 4. Restart Fluxer & Quit Fluxer
        let restart_item = MenuItem::new("Restart Fluxer", true, None);
        let quit_item = MenuItem::new("Quit Fluxer", true, None);

        let restart_id = restart_item.id().clone();
        let quit_id = quit_item.id().clone();

        menu.append(&restart_item)?;
        menu.append(&quit_item)?;

        let icon = create_tray_icon()?;

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("Fluxer")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()?;

        TRAY_INITIALIZED.store(true, Ordering::Relaxed);

        Ok(Self {
            _tray_icon: tray_icon,
            toggle_item,
            open_settings_id,
            overlay_item_id,
            status_online_item,
            status_idle_item,
            status_dnd_item,
            status_invisible_item,
            check_updates_id,
            restart_id,
            quit_id,
        })
    }

    pub fn handle_menu_event(&self, id: &MenuId, window: &Arc<Window>) -> TrayMenuAction {
        if id == self.toggle_item.id() {
            toggle_window_visibility(window);
            self.update_visibility_label(window.is_visible());
            TrayMenuAction::ToggleWindow
        } else if id == &self.open_settings_id {
            ensure_window_visible(window);
            self.update_visibility_label(true);
            TrayMenuAction::OpenSettings
        } else if id == &self.overlay_item_id {
            TrayMenuAction::ToggleOverlay
        } else if id == self.status_online_item.id() {
            self.set_active_status("online");
            TrayMenuAction::SetStatus("online".to_string())
        } else if id == self.status_idle_item.id() {
            self.set_active_status("idle");
            TrayMenuAction::SetStatus("idle".to_string())
        } else if id == self.status_dnd_item.id() {
            self.set_active_status("dnd");
            TrayMenuAction::SetStatus("dnd".to_string())
        } else if id == self.status_invisible_item.id() {
            self.set_active_status("invisible");
            TrayMenuAction::SetStatus("invisible".to_string())
        } else if id == &self.check_updates_id {
            TrayMenuAction::CheckForUpdates
        } else if id == &self.restart_id {
            TrayMenuAction::Restart
        } else if id == &self.quit_id {
            TrayMenuAction::Quit
        } else {
            TrayMenuAction::None
        }
    }

    pub fn toggle_window(&self, window: &Arc<Window>) {
        toggle_window_visibility(window);
        self.update_visibility_label(window.is_visible());
    }

    pub fn update_visibility_label(&self, is_visible: bool) {
        let label = if is_visible { "Hide Fluxer" } else { "Show Fluxer" };
        self.toggle_item.set_text(label);
    }

    fn set_active_status(&self, status: &str) {
        self.status_online_item.set_checked(status == "online");
        self.status_idle_item.set_checked(status == "idle");
        self.status_dnd_item.set_checked(status == "dnd");
        self.status_invisible_item.set_checked(status == "invisible");
    }
}

pub fn toggle_window_visibility(window: &Arc<Window>) {
    let is_visible = window.is_visible();
    if is_visible {
        window.set_visible(false);
    } else {
        ensure_window_visible(window);
    }
}

pub fn ensure_window_visible(window: &Arc<Window>) {
    window.set_visible(true);
    window.set_minimized(false);
    window.set_focus();
}

/// 32x32 Fluxer 공식 로고 트레이 아이콘을 로드합니다.
fn create_tray_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    const ICON_BYTES: &[u8] = include_bytes!("../resources/tray_32x32.png");
    if let Ok(img) = image::load_from_memory(ICON_BYTES) {
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        return Ok(Icon::from_rgba(rgba.into_raw(), width, height)?);
    }

    // Fallback: 32x32 Rust Red 로고 버퍼
    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 32;
    let mut rgba = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let dx = (x as i32) - 16;
            let dy = (y as i32) - 16;
            let dist_sq = dx * dx + dy * dy;

            if dist_sq < 144 {
                // 내부 로고: Rust Red (#ce412b)
                rgba.extend_from_slice(&[0xce, 0x41, 0x2b, 0xff]);
            } else if dist_sq < 196 {
                // 테두리: Rust Warm Orange (#e55a3c)
                rgba.extend_from_slice(&[0xe5, 0x5a, 0x3c, 0x88]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    Ok(Icon::from_rgba(rgba, WIDTH, HEIGHT)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_tray_icon() {
        let icon_res = create_tray_icon();
        assert!(icon_res.is_ok());
    }

    #[test]
    fn test_tray_menu_action_variants() {
        let action = TrayMenuAction::ToggleOverlay;
        assert_eq!(action, TrayMenuAction::ToggleOverlay);
    }
}
