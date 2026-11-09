// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::contracts::{GlobalKeyHookRegisterOptions, GlobalShortcutsStatus};

#[cfg(windows)]
use fluxer_windows_input_hook::{EventKind, InputHook, QueuedEvent};

pub type KeybindCallback = Arc<dyn Fn(String, String) + Send + Sync + 'static>;

struct InputHookState {
    active: bool,
    #[cfg(windows)]
    hook: Option<InputHook>,
    registrations: HashMap<String, GlobalKeyHookRegisterOptions>,
}

pub struct InputHookManager {
    state: Arc<Mutex<InputHookState>>,
    callback: KeybindCallback,
}

impl InputHookManager {
    pub fn new(callback: KeybindCallback) -> Self {
        Self {
            state: Arc::new(Mutex::new(InputHookState {
                active: false,
                #[cfg(windows)]
                hook: None,
                registrations: HashMap::new(),
            })),
            callback,
        }
    }

    /// 전역 단축키 및 PTT 훅 상태를 반환합니다.
    pub fn get_status(&self) -> GlobalShortcutsStatus {
        GlobalShortcutsStatus {
            supported: cfg!(windows),
            has_access: true,
            direct_input_enabled: true,
            direct_input_available: cfg!(windows),
            is_capturing: false,
            mode: "native".to_string(),
        }
    }

    /// 전역 저수준 키보드 및 마우스 훅을 시작합니다.
    #[cfg(windows)]
    pub fn start(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.active {
            return true;
        }

        let state_clone = Arc::clone(&self.state);
        let cb_clone = Arc::clone(&self.callback);

        let event_sink = Arc::new(move |event: QueuedEvent| {
            Self::handle_event(&state_clone, &cb_clone, event);
        });

        let hook = InputHook::new_with_sink(event_sink);
        if let Err(err) = hook.start() {
            eprintln!("[InputHook] Failed to start windows-input-hook: {err}");
            return false;
        }

        state.hook = Some(hook);
        state.active = true;
        true
    }

    #[cfg(not(windows))]
    pub fn start(&self) -> bool {
        false
    }

    /// 전역 훅을 중지합니다.
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        if !state.active {
            return;
        }
        #[cfg(windows)]
        {
            if let Some(hook) = state.hook.take() {
                hook.stop();
            }
        }
        state.active = false;
    }

    /// 단축키 또는 PTT 바인딩을 등록합니다.
    pub fn register(&self, opts: GlobalKeyHookRegisterOptions) {
        let mut state = self.state.lock().unwrap();
        state.registrations.insert(opts.id.clone(), opts);
    }

    /// 지정된 바인딩을 해제합니다.
    pub fn unregister(&self, id: &str) {
        let mut state = self.state.lock().unwrap();
        state.registrations.remove(id);
    }

    /// 등록된 모든 바인딩을 해제합니다.
    pub fn unregister_all(&self) {
        let mut state = self.state.lock().unwrap();
        state.registrations.clear();
    }

    #[cfg(windows)]
    fn handle_event(
        state_lock: &Arc<Mutex<InputHookState>>,
        callback: &KeybindCallback,
        event: QueuedEvent,
    ) {
        let (is_down, is_mouse) = match event.kind {
            EventKind::KeyDown => (true, false),
            EventKind::KeyUp => (false, false),
            EventKind::MouseDown => (true, true),
            EventKind::MouseUp => (false, true),
        };

        let event_type = if is_down { "keydown" } else { "keyup" };
        let event_key_name = event.key_name();

        let state = state_lock.lock().unwrap();
        for (id, opts) in &state.registrations {
            // 보조키(Modifiers) 일치 여부 검사
            let mod_match = event.ctrl == opts.ctrl
                && event.alt == opts.alt
                && event.shift == opts.shift
                && event.meta == opts.meta;

            if !mod_match {
                continue;
            }

            // 마우스 버튼 바인딩 매칭
            if is_mouse {
                if let Some(btn) = opts.mouse_button
                    && event.button == btn
                {
                    callback(id.clone(), event_type.to_string());
                }
                continue;
            }

            // 키보드 키 이름 매칭 (대소문자 무시)
            if !is_mouse {
                let mut matched = false;
                if let Some(ref target) = opts.key_name
                    && target.eq_ignore_ascii_case(event_key_name)
                {
                    matched = true;
                }
                if !matched
                    && let Some(ref target) = opts.physical_key_name
                    && target.eq_ignore_ascii_case(event_key_name)
                {
                    matched = true;
                }

                if matched {
                    callback(id.clone(), event_type.to_string());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_hook_status_query() {
        let triggered = Arc::new(Mutex::new(Vec::new()));
        let t_clone = Arc::clone(&triggered);
        let mgr = InputHookManager::new(Arc::new(move |id, evt| {
            t_clone.lock().unwrap().push((id, evt));
        }));

        let status = mgr.get_status();
        assert_eq!(status.has_access, true);
        assert_eq!(status.mode, "native");

        // 등록 및 해제 검증
        let opts = GlobalKeyHookRegisterOptions {
            id: "ptt-1".to_string(),
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            key_name: Some("Space".to_string()),
            physical_key_name: None,
            mouse_button: None,
        };

        mgr.register(opts);
        {
            let st = mgr.state.lock().unwrap();
            assert!(st.registrations.contains_key("ptt-1"));
        }

        mgr.unregister("ptt-1");
        {
            let st = mgr.state.lock().unwrap();
            assert!(!st.registrations.contains_key("ptt-1"));
        }
    }
}
