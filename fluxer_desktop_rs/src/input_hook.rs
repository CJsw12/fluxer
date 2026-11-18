// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;
use serde_json::{Value, json};
use crate::contracts::{GlobalKeyHookRegisterOptions, GlobalShortcutBinding, GlobalShortcutsStatus, GlobalShortcutsSyncPayload};
#[cfg(windows)]
use fluxer_windows_input_hook::{EventKind, InputHook, QueuedEvent};

// Callback MUST only enqueue (main uses EventLoopProxy), never re-enter this
// manager. Emitting under state serializes IPC workers and native transitions.
pub type KeybindCallback = Arc<dyn Fn(&str, Value) + Send + Sync + 'static>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Trigger {
    Key { code: Option<String>, layout: Option<String> },
    Mouse(u8),
    Modifiers { kinds: Vec<usize>, both: bool },
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding { source: String, action: String, trigger: Trigger, modifiers: [bool; 4] }
#[derive(Clone, Debug, PartialEq, Eq)]
enum Press { Key(u32, u32, bool), Mouse(u8), Modifiers }
struct HookEvent { down: bool, input: Press, keycode: u32, code: Option<String>, key: Option<String>, modifiers: [bool; 4] }
const MODIFIERS: [[&str; 2]; 4] = [
    ["ControlLeft", "ControlRight"], ["AltLeft", "AltRight"],
    ["ShiftLeft", "ShiftRight"], ["MetaLeft", "MetaRight"],
];
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> { mutex.lock().unwrap_or_else(|p| p.into_inner()) }
fn modifier_kind(code: &str) -> Option<usize> { MODIFIERS.iter().position(|s| s.contains(&code)) }
fn binding_from_combo(binding: GlobalShortcutBinding) -> Result<Binding, String> {
    let combo = binding.combo;
    let modifiers = [combo.ctrl, combo.alt, combo.shift, combo.meta];
    if binding.source_id.is_empty() || binding.action.is_empty() { return Err("Shortcut sourceId and action must not be empty".into()); }
    let trigger = if let Some(button) = combo.mouse_button {
        if button > 4 { return Err("Unsupported mouse button".into()); }
        Trigger::Mouse(button)
    } else {
        let code = combo.code.or_else(|| dom_code(&combo.key));
        if let Some(ref code) = code && !known_code(code) { return Err(format!("Unsupported shortcut code: {code}")); }
        if combo.modifier_only {
            let own = code.as_deref().and_then(modifier_kind);
            let kinds: Vec<_> = (0..4).filter(|i| modifiers[*i] || own == Some(*i)).collect();
            if kinds.is_empty() { return Err("Modifier-only binding requires a modifier".into()); }
            let both = combo.both_sides && kinds.len() == 1;
            Trigger::Modifiers { kinds, both }
        } else {
            let upper = combo.key.to_ascii_uppercase();
            let layout = (upper.len() == 1 && upper.as_bytes()[0].is_ascii_alphanumeric()
                && !code.as_deref().is_some_and(|c| c.starts_with("Numpad"))).then_some(upper);
            if code.is_none() && layout.is_none() { return Err(format!("Unsupported shortcut key: {}", combo.key)); }
            Trigger::Key { code, layout }
        }
    };
    Ok(Binding { source: binding.source_id, action: binding.action, trigger, modifiers })
}
#[derive(Default)]
struct InputHookState {
    active: bool, hook_error: Option<String>, paused: bool, legacy_started: bool,
    bindings: Vec<Binding>, pressed: HashMap<String, Press>, held_modifiers: HashSet<String>,
    registrations: HashMap<String, GlobalKeyHookRegisterOptions>, legacy_pressed: HashMap<String, Press>,
    capture: Option<u64>, capture_seq: u64,
}
impl InputHookState {
    fn status(&self) -> GlobalShortcutsStatus {
        GlobalShortcutsStatus {
            backend: if cfg!(windows) { "windows" } else { "none" }.into(),
            platform: if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" }.into(),
            linux: None, hooks_active: self.active, hook_error: self.hook_error.clone(),
            supports_mouse_buttons: cfg!(windows), supports_modifier_only: cfg!(windows),
        }
    }
    fn transition(binding: &Binding, phase: &str, emit: &KeybindCallback) {
        emit("event", json!({"sourceId": binding.source, "action": binding.action, "phase": phase}));
    }
    fn set_bindings(&mut self, payload: GlobalShortcutsSyncPayload, emit: &KeybindCallback) -> Result<(), String> {
        let mut seen = HashSet::new();
        let mut bindings = Vec::new();
        for entry in payload.bindings {
            // Electron keeps the first sourceId; distinct sources may share a combo.
            if seen.insert(entry.source_id.clone()) { bindings.push(binding_from_combo(entry)?); }
        }
        for previous in &self.bindings {
            if self.pressed.contains_key(&previous.source) && !bindings.contains(previous) {
                self.pressed.remove(&previous.source);
                Self::transition(previous, "release", emit);
            }
        }
        self.bindings = bindings;
        Ok(())
    }
    fn modifier_satisfied(&self, trigger: &Trigger) -> bool {
        match trigger {
            Trigger::Modifiers { kinds, both } => kinds.iter().all(|i| {
                let [left, right] = MODIFIERS[*i].map(|c| self.held_modifiers.contains(c));
                if *both { left && right } else { left || right }
            }),
            _ => false,
        }
    }
    fn release_all(&mut self, emit: &KeybindCallback) {
        for binding in &self.bindings {
            if self.pressed.remove(&binding.source).is_some() { Self::transition(binding, "release", emit); }
        }
        for (id, _) in self.legacy_pressed.drain() { emit("legacy", json!({"id": id, "type": "keyup"})); }
        self.held_modifiers.clear();
    }
    fn handle_event(&mut self, event: HookEvent, emit: &KeybindCallback) {
        if !self.active { return; }
        let own = event.code.as_deref().filter(|c| modifier_kind(c).is_some());
        for (i, sides) in MODIFIERS.iter().enumerate() {
            if !event.modifiers[i] {
                for code in sides {
                    if !(event.down && own == Some(code)) { self.held_modifiers.remove(*code); }
                }
            }
        }
        if !event.down && let Some(code) = own { self.held_modifiers.remove(code); }
        for binding in &self.bindings {
            if let Some(press) = self.pressed.get(&binding.source) {
                // Release uses physical identity, independent of current modifiers/layout.
                if (!event.down && *press == event.input)
                    || (*press == Press::Modifiers && !self.modifier_satisfied(&binding.trigger)) {
                    self.pressed.remove(&binding.source);
                    Self::transition(binding, "release", emit);
                }
            }
        }
        let satisfied_before: HashSet<_> = self.bindings.iter().filter(|b| self.modifier_satisfied(&b.trigger))
            .map(|b| b.source.clone()).collect();
        if event.down && let Some(code) = own { self.held_modifiers.insert(code.into()); }
        if event.down && !self.paused {
            for binding in &self.bindings {
                if self.pressed.contains_key(&binding.source) { continue; }
                let matched = match &binding.trigger {
                    Trigger::Modifiers { .. } => !satisfied_before.contains(&binding.source) && self.modifier_satisfied(&binding.trigger),
                    Trigger::Mouse(button) => event.input == Press::Mouse(*button) && modifiers_match(binding.modifiers, event.modifiers),
                    Trigger::Key { code, layout } => {
                        let key_matches = match (layout, &event.key) {
                            (Some(expected), Some(key)) if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => key == expected,
                            (Some(_), Some(key)) if dom_code(key).is_some() => false,
                            _ => code.is_some() && *code == event.code,
                        };
                        matches!(event.input, Press::Key(..)) && key_matches && modifiers_match(binding.modifiers, event.modifiers)
                    }
                };
                if matched {
                    self.pressed.insert(binding.source.clone(), if matches!(binding.trigger, Trigger::Modifiers { .. }) { Press::Modifiers } else { event.input.clone() });
                    Self::transition(binding, "press", emit);
                }
            }
        }
        if self.legacy_started {
        let mut raw = json!({"ctrlKey": event.modifiers[0], "altKey": event.modifiers[1],
            "shiftKey": event.modifiers[2], "metaKey": event.modifiers[3]});
        if let Press::Mouse(button) = event.input {
            raw["type"] = json!(if event.down { "mousedown" } else { "mouseup" });
            raw["button"] = json!(button);
            emit("mouse", raw);
        } else {
            raw["type"] = json!(if event.down { "keydown" } else { "keyup" });
            raw["keycode"] = json!(event.keycode);
            raw["keyName"] = json!(event.key.as_deref().unwrap_or(""));
            raw["backend"] = json!("native");
            emit("key", raw);
        }
        for (id, opts) in &self.registrations {
            if !event.down {
                if self.legacy_pressed.get(id) == Some(&event.input) {
                    self.legacy_pressed.remove(id);
                    emit("legacy", json!({"id": id, "type": "keyup"}));
                }
                continue;
            }
            if self.legacy_pressed.contains_key(id) { continue; }
            let matched = match event.input {
                Press::Mouse(button) => opts.mouse_button == Some(button),
                Press::Key(..) => opts.keycode == Some(event.keycode)
                    || opts.key_name.as_ref().is_some_and(|k| event.key.as_ref().is_some_and(|n| n.eq_ignore_ascii_case(k)))
                    || opts.physical_key_name.as_ref().is_some_and(|k| event.code.as_ref().is_some_and(|n| n.eq_ignore_ascii_case(k))),
                Press::Modifiers => false,
            };
            if matched && event.modifiers == [opts.ctrl, opts.alt, opts.shift, opts.meta] {
                self.legacy_pressed.insert(id.clone(), event.input.clone());
                emit("legacy", json!({"id": id, "type": "keydown"}));
            }
        }
        }
        if self.capture.is_some() {
            let mouse = matches!(event.input, Press::Mouse(_));
            emit("capture", json!({
                "type": match (mouse, event.down) { (false, true) => "keydown", (false, false) => "keyup", (true, true) => "mousedown", (true, false) => "mouseup" },
                "code": event.code, "key": event.key,
                "button": if let Press::Mouse(button) = event.input { Some(button) } else { None },
                "ctrl": event.modifiers[0], "alt": event.modifiers[1], "shift": event.modifiers[2], "meta": event.modifiers[3]
            }));
        }
    }
}
fn modifiers_match(expected: [bool; 4], actual: [bool; 4]) -> bool {
    // Electron permits extra modifiers on otherwise unmodified bindings.
    expected == [false; 4] || expected == actual
}
struct CaptureTimer { cancel: mpsc::Sender<()>, worker: JoinHandle<()> }
impl CaptureTimer {
    fn stop(self) { let _ = self.cancel.send(()); let _ = self.worker.join(); }
}
#[derive(Default)]
struct Lifecycle {
    #[cfg(windows)]
    hook: Option<InputHook>,
    timer: Option<CaptureTimer>,
}
pub struct InputHookManager {
    state: Arc<Mutex<InputHookState>>,
    lifecycle: Mutex<Lifecycle>,
    callback: KeybindCallback,
}
impl InputHookManager {
    pub fn new(callback: KeybindCallback) -> Self {
        Self { state: Arc::new(Mutex::new(InputHookState::default())), lifecycle: Mutex::new(Lifecycle::default()), callback }
    }
    pub fn get_status(&self) -> GlobalShortcutsStatus { lock(&self.state).status() }
    pub fn handle_request(&self, command: &str, payload: &Value) -> Result<Value, String> {
        match command {
            "globalShortcutsSync" | "global-shortcuts:sync" => {
                let payload = serde_json::from_value(payload.clone()).map_err(|e| format!("Invalid shortcut sync payload: {e}"))?;
                self.sync(payload)?;
            }
            "globalShortcutsSetPaused" | "global-shortcuts:set-paused" => {
                let paused = payload.get("paused").and_then(Value::as_bool).ok_or("paused must be a boolean")?;
                self.set_paused(paused);
            }
            "globalShortcutsGetStatus" | "global-shortcuts:get-status"
            | "globalShortcutsSetUp" | "globalShortcutsRecheck" => return Ok(json!(self.get_status())),
            "globalShortcutsConfigure" | "globalShortcutsSetDirectInputEnabled" => {
                // Portal configuration and evdev opt-in do not exist on Windows.
                return Err("Linux portal/direct-input configuration is unavailable on this backend".into());
            }
            "globalShortcutsStartCapture" => return self.start_capture().map(|id| json!(id)),
            "globalShortcutsStopCapture" => {
                let id = payload.get("captureId").and_then(Value::as_u64).ok_or("captureId must be an integer")?;
                self.stop_capture(id);
            }
            "globalShortcutsReset" => self.reset(),
            "globalKeyHookStart" | "global-key-hook-start" => return Ok(json!(self.start())),
            "globalKeyHookStop" | "global-key-hook-stop" => self.stop(),
            "globalKeyHookRegister" | "global-key-hook-register" => {
                let options = serde_json::from_value(payload.clone()).map_err(|e| format!("Invalid legacy binding: {e}"))?;
                self.register(options)?;
            }
            "globalKeyHookUnregister" | "global-key-hook-unregister" => {
                let id = payload.get("id").and_then(Value::as_str).or_else(|| payload.as_str()).ok_or("Binding id must be a string")?;
                self.unregister(id);
            }
            "globalKeyHookUnregisterAll" | "global-key-hook-unregister-all" => self.unregister_all(),
            _ => return Err(format!("Unsupported global input command: {command}")),
        }
        Ok(Value::Null)
    }
    fn notify_status(&self) { (self.callback)("status", json!(lock(&self.state).status())); }
    fn refresh(&self, lifecycle: &mut Lifecycle) -> Result<(), String> {
        let wanted = {
            let state = lock(&self.state);
            state.legacy_started || !state.bindings.is_empty() || state.capture.is_some()
        };
        if wanted && !lock(&self.state).active {
            #[cfg(windows)]
            {
                let state = Arc::clone(&self.state);
                let callback = Arc::clone(&self.callback);
                let hook = InputHook::new_with_sink(Arc::new(move |event| {
                    lock(&state).handle_event(event_from_native(event), &callback);
                }));
                // Never hold state across start/stop: start can deliver events;
                // stop joins the event-producing worker.
                if let Err(error) = hook.start() {
                    lock(&self.state).hook_error = Some("start-failed".into());
                    self.notify_status();
                    return Err(error);
                }
                lifecycle.hook = Some(hook);
                let mut state = lock(&self.state);
                state.active = true;
                state.hook_error = None;
            }
            #[cfg(not(windows))]
            { return Err("Global shortcuts are currently supported only on Windows".into()); }
        } else if !wanted { self.stop_native(lifecycle); }
        self.notify_status();
        Ok(())
    }
    fn stop_native(&self, lifecycle: &mut Lifecycle) {
        {
            let mut state = lock(&self.state);
            state.active = false;
            state.release_all(&self.callback);
        }
        #[cfg(windows)]
        if let Some(hook) = lifecycle.hook.take() { hook.stop(); }
        #[cfg(not(windows))]
        let _ = lifecycle;
    }
    pub fn sync(&self, payload: GlobalShortcutsSyncPayload) -> Result<(), String> {
        let mut lifecycle = lock(&self.lifecycle);
        lock(&self.state).set_bindings(payload, &self.callback)?;
        self.refresh(&mut lifecycle)
    }
    pub fn set_paused(&self, paused: bool) { lock(&self.state).paused = paused; }
    pub fn start(&self) -> bool {
        let mut lifecycle = lock(&self.lifecycle);
        lock(&self.state).legacy_started = true;
        if self.refresh(&mut lifecycle).is_err() { lock(&self.state).legacy_started = false; return false; }
        true
    }
    pub fn stop(&self) {
        let mut lifecycle = lock(&self.lifecycle);
        {
            let mut state = lock(&self.state);
            state.legacy_started = false;
            for (id, _) in state.legacy_pressed.drain() { (self.callback)("legacy", json!({"id": id, "type": "keyup"})); }
        }
        let _ = self.refresh(&mut lifecycle);
    }
    pub fn register(&self, opts: GlobalKeyHookRegisterOptions) -> Result<(), String> {
        if opts.id.is_empty() || (opts.keycode.is_none() && opts.key_name.is_none() && opts.physical_key_name.is_none() && opts.mouse_button.is_none()) {
            return Err("A legacy binding requires an id and a key or mouse button".into());
        }
        if opts.mouse_button.is_some_and(|b| b > 4) { return Err("Unsupported mouse button".into()); }
        let mut state = lock(&self.state);
        if state.legacy_pressed.remove(&opts.id).is_some() { (self.callback)("legacy", json!({"id": opts.id, "type": "keyup"})); }
        state.registrations.insert(opts.id.clone(), opts);
        Ok(())
    }
    pub fn unregister(&self, id: &str) {
        let mut state = lock(&self.state);
        state.registrations.remove(id);
        if state.legacy_pressed.remove(id).is_some() { (self.callback)("legacy", json!({"id": id, "type": "keyup"})); }
    }
    pub fn unregister_all(&self) {
        let mut state = lock(&self.state);
        state.registrations.clear();
        for (id, _) in state.legacy_pressed.drain() { (self.callback)("legacy", json!({"id": id, "type": "keyup"})); }
    }
    pub fn start_capture(&self) -> Result<u64, String> {
        let mut lifecycle = lock(&self.lifecycle);
        if let Some(timer) = lifecycle.timer.take() { timer.stop(); }
        let id = {
            let mut state = lock(&self.state);
            state.capture_seq += 1;
            let id = state.capture_seq;
            state.capture = Some(id);
            id
        };
        if let Err(error) = self.refresh(&mut lifecycle) { lock(&self.state).capture = None; return Err(error); }
        let (cancel, receive) = mpsc::channel();
        let callback = Arc::clone(&self.callback);
        let worker = match std::thread::Builder::new().name("fluxer-shortcut-capture-timer".into()).spawn(move || {
            if receive.recv_timeout(Duration::from_secs(60)) == Err(mpsc::RecvTimeoutError::Timeout) {
                // Main handles expiry; timer never takes lifecycle (owner joins it).
                callback("capture-timeout", json!(id));
            }
        }) {
            Ok(worker) => worker,
            Err(error) => {
                lock(&self.state).capture = None;
                let _ = self.refresh(&mut lifecycle);
                return Err(format!("Could not start capture timer: {error}"));
            }
        };
        lifecycle.timer = Some(CaptureTimer { cancel, worker });
        Ok(id)
    }
    pub fn stop_capture(&self, id: u64) {
        let mut lifecycle = lock(&self.lifecycle);
        {
            let mut state = lock(&self.state);
            if state.capture != Some(id) { return; }
            state.capture = None;
        }
        if let Some(timer) = lifecycle.timer.take() { timer.stop(); }
        let _ = self.refresh(&mut lifecycle);
    }
    pub fn reset(&self) {
        let mut lifecycle = lock(&self.lifecycle);
        { let mut state = lock(&self.state); state.capture = None; state.paused = false; state.legacy_started = false; }
        if let Some(timer) = lifecycle.timer.take() { timer.stop(); }
        self.stop_native(&mut lifecycle);
        let mut state = lock(&self.state);
        state.bindings.clear();
        state.registrations.clear();
    }
}
impl Drop for InputHookManager { fn drop(&mut self) { self.reset(); } }
#[cfg(windows)]
fn event_from_native(event: QueuedEvent) -> HookEvent {
    let mouse = matches!(event.kind, EventKind::MouseDown | EventKind::MouseUp);
    let code = if mouse { None } else if event.keycode == 0xe7 {
        (event.scan_code == 0x20).then(|| "Space".into())
    } else { windows_code(event.scan_code, event.extended, event.keycode).map(str::to_owned) };
    let key = if mouse { None } else if event.keycode == 0xe7 {
        char::from_u32(event.scan_code).filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase().to_string())
    } else {
        let name = event.key_name();
        (!name.is_empty() && !name.starts_with("VK_")).then(|| name.to_owned())
    };
    let mut modifiers = [event.ctrl, event.alt, event.shift, event.meta];
    let down = matches!(event.kind, EventKind::KeyDown | EventKind::MouseDown);
    if down && let Some(i) = code.as_deref().and_then(modifier_kind) { modifiers[i] = true; }
    HookEvent { down, keycode: event.keycode, input: if mouse { Press::Mouse(event.button) } else {
        Press::Key(if event.scan_code == 0 { event.keycode } else { 0 }, event.scan_code, event.extended)
    }, code, key, modifiers }
}
// Physical identity tables match Electron GlobalShortcutKeys.ts.
include!("input_hook_key_codes.rs");

#[cfg(all(test, windows))]
#[path = "input_hook_tests.rs"]
pub(crate) mod tests;
