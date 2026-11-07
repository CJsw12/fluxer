// SPDX-License-Identifier: AGPL-3.0-or-later

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;

pub struct ShortcutManager {
    _manager: GlobalHotKeyManager,
    pub focus_hotkey: HotKey,
    pub mute_hotkey: HotKey,
    pub overlay_hotkey: HotKey,
}

impl ShortcutManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let manager = GlobalHotKeyManager::new()?;

        // Ctrl + Alt + F : Fluxer 창 활성화/포커스
        let focus_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyF);

        // Ctrl + Shift + M : 전역 음소거/액션 토글
        let mute_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);

        // Ctrl + Shift + O : GPUI 네이티브 음성 오버레이 토글
        let overlay_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);

        let _ = manager.register(focus_hotkey);
        let _ = manager.register(mute_hotkey);
        let _ = manager.register(overlay_hotkey);

        Ok(Self {
            _manager: manager,
            focus_hotkey,
            mute_hotkey,
            overlay_hotkey,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_creation() {
        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyF);
        assert_eq!(hotkey.id(), HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyF).id());

        let overlay_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
        assert_eq!(overlay_hotkey.id(), HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO).id());
    }
}
