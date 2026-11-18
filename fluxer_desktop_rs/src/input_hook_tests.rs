// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

type Messages = Arc<Mutex<Vec<(String, Value)>>>;

fn fixture() -> (InputHookManager, Messages) {
    let messages = Arc::new(Mutex::new(Vec::new()));
    let target = Arc::clone(&messages);
    let manager = InputHookManager::new(Arc::new(move |kind, value| lock(&target).push((kind.into(), value))));
    // No OS hooks are installed and no input is injected. Native QueuedEvent
    // fixtures enter the exact adapter/sink used by an already installed hook.
    lock(&manager.state).active = true;
    (manager, messages)
}

fn key(kind: EventKind, vk: u32, scan: u32, name: &str, ctrl: bool) -> QueuedEvent {
    let mut event = QueuedEvent { kind, keycode: vk, scan_code: scan, ctrl, ..QueuedEvent::default() };
    event.key_name_buf[..name.len()].copy_from_slice(name.as_bytes());
    event.key_name_len = name.len() as u8;
    event
}

fn feed(manager: &InputHookManager, event: QueuedEvent) {
    lock(&manager.state).handle_event(event_from_native(event), &manager.callback);
}

fn combo(key: &str, code: &str, ctrl: bool) -> Value {
    json!({"key": key, "code": code, "ctrl": ctrl, "alt": false, "shift": false, "meta": false})
}

fn binding(source: &str, combo: Value) -> Value { json!({"sourceId": source, "action": "push_to_talk", "combo": combo}) }

fn sync(manager: &InputHookManager, bindings: Vec<Value>) {
    manager.handle_request("globalShortcutsSync", &json!({"bindings": bindings, "actions": []})).unwrap();
}

fn events(messages: &Messages) -> Vec<Value> {
    lock(messages).iter().filter(|(kind, _)| kind == "event").map(|(_, v)| v.clone()).collect()
}

// Also consumed by the Node bridge execution test: these are serialized output
// from sync + the native event adapter/matcher, not handwritten bridge payloads.
pub(crate) fn shortcut_fixture_events() -> Vec<Value> {
    let (manager, messages) = fixture();
    sync(&manager, vec![binding("ptt", combo("M", "KeyM", true))]);
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", true));
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", true));
    feed(&manager, key(EventKind::KeyUp, 0xa2, 0x1d, "ControlLeft", false));
    feed(&manager, key(EventKind::KeyUp, 0x4d, 0x32, "M", false));
    events(&messages)
}

#[test]
fn sync_native_repeat_and_modifier_release_produce_one_balanced_pair() {
    assert_eq!(shortcut_fixture_events(), vec![
        json!({"action": "push_to_talk", "sourceId": "ptt", "phase": "press"}),
        json!({"action": "push_to_talk", "sourceId": "ptt", "phase": "release"}),
    ]);
}

#[test]
fn resync_duplicate_sources_and_shared_combos_match_electron() {
    let (manager, messages) = fixture();
    let first = binding("one", combo("M", "KeyM", false));
    let second = binding("two", combo("M", "KeyM", false));
    sync(&manager, vec![first.clone(), binding("one", combo("A", "KeyA", false)), second.clone()]);
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", false));
    assert_eq!(events(&messages).len(), 2);
    sync(&manager, vec![first, second]);
    assert_eq!(events(&messages).len(), 2, "identical sync must preserve held presses");
    sync(&manager, vec![binding("one", combo("A", "KeyA", false))]);
    let result = events(&messages);
    assert_eq!(result.iter().map(|v| v["phase"].as_str().unwrap()).collect::<Vec<_>>(), ["press", "press", "release", "release"]);
    feed(&manager, key(EventKind::KeyUp, 0x4d, 0x32, "M", false));
    assert_eq!(events(&messages), result, "old keyup must not release a replacement");
    sync(&manager, vec![]);
    assert!(!manager.get_status().hooks_active);
}

#[test]
fn invalid_sync_is_atomic_and_does_not_report_success() {
    let (manager, messages) = fixture();
    sync(&manager, vec![binding("ptt", combo("M", "KeyM", false))]);
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", false));
    let invalid = json!({"bindings": [binding("new", combo("Unknown", "NotAKey", false))], "actions": []});
    assert!(manager.handle_request("globalShortcutsSync", &invalid).is_err());
    assert_eq!(events(&messages).len(), 1);
    feed(&manager, key(EventKind::KeyUp, 0x4d, 0x32, "M", false));
    assert_eq!(events(&messages).last().unwrap()["sourceId"], "ptt");
    assert!(manager.handle_request("globalShortcutsSetPaused", &json!({"paused": "yes"})).is_err());
    assert!(manager.handle_request("globalShortcutsConfigure", &Value::Null).is_err());
    assert!(manager.handle_request("globalShortcutsTypo", &Value::Null).is_err());
}

#[test]
fn pause_blocks_presses_but_allows_pending_releases_and_resume() {
    let (manager, messages) = fixture();
    sync(&manager, vec![binding("ptt", combo("M", "KeyM", true))]);
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", true));
    manager.handle_request("globalShortcutsSetPaused", &json!({"paused": true})).unwrap();
    feed(&manager, key(EventKind::KeyUp, 0x4d, 0x32, "M", false));
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", true));
    assert_eq!(events(&messages).len(), 2);
    manager.set_paused(false);
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", true));
    assert_eq!(events(&messages).len(), 3);
    manager.reset();
    assert_eq!(events(&messages).last().unwrap()["phase"], "release");
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", true));
    assert_eq!(events(&messages).len(), 4, "late callback after reset must be ignored");
}

#[test]
fn mouse_buttons_use_dom_numbering_and_release_without_modifiers() {
    let (manager, messages) = fixture();
    let mut mouse = combo("", "", true);
    mouse["mouseButton"] = json!(3);
    sync(&manager, vec![binding("mouse", mouse)]);
    let down = QueuedEvent { kind: EventKind::MouseDown, button: 3, ctrl: true, ..QueuedEvent::default() };
    feed(&manager, down);
    feed(&manager, down);
    feed(&manager, QueuedEvent { kind: EventKind::MouseUp, ctrl: false, ..down });
    assert_eq!(events(&messages).iter().map(|v| v["phase"].as_str().unwrap()).collect::<Vec<_>>(), ["press", "release"]);
}

#[test]
fn modifier_only_both_sides_reconcile_lost_keyups_from_flags() {
    let (manager, messages) = fixture();
    let mut modifiers = combo("ControlLeft", "ControlLeft", true);
    modifiers["modifierOnly"] = json!(true);
    modifiers["bothSides"] = json!(true);
    sync(&manager, vec![binding("both", modifiers)]);
    feed(&manager, key(EventKind::KeyDown, 0xa2, 0x1d, "ControlLeft", false));
    assert!(events(&messages).is_empty());
    let mut right = key(EventKind::KeyDown, 0xa3, 0x1d, "ControlRight", true);
    right.extended = true;
    feed(&manager, right);
    feed(&manager, right);
    assert_eq!(events(&messages).len(), 1);
    // Missing modifier key-up is recovered when a later event has no Ctrl flag.
    feed(&manager, key(EventKind::KeyDown, 0x41, 0x1e, "A", false));
    assert_eq!(events(&messages).last().unwrap()["phase"], "release");
}

#[test]
fn layout_and_numpad_identity_and_changed_keycode_on_release() {
    let (manager, messages) = fixture();
    sync(&manager, vec![binding("layout", combo("Q", "KeyA", false)), binding("pad", combo("1", "Numpad1", false))]);
    feed(&manager, key(EventKind::KeyDown, 0x51, 0x1e, "Q", true));
    // Same physical key released after layout change; vk/name no longer match.
    feed(&manager, key(EventKind::KeyUp, 0x41, 0x1e, "A", false));
    feed(&manager, key(EventKind::KeyDown, 0x31, 0x02, "1", false));
    assert_eq!(events(&messages).len(), 2, "main-row digit must not match numpad");
    feed(&manager, key(EventKind::KeyDown, 0x61, 0x4f, "Numpad1", false));
    assert_eq!(events(&messages).last().unwrap()["sourceId"], "pad");
}

#[test]
fn capture_replacement_cancel_and_raw_legacy_channels_cleanup() {
    let (manager, messages) = fixture();
    let first = manager.start_capture().unwrap();
    let second = manager.start_capture().unwrap();
    assert_ne!(first, second);
    manager.stop_capture(first);
    assert_eq!(lock(&manager.state).capture, Some(second));
    lock(&manager.state).legacy_started = true;
    manager.register(serde_json::from_value(json!({"id": "legacy", "keycode": 0x4d})).unwrap()).unwrap();
    feed(&manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", false));
    let capture = lock(&messages).iter().find(|(kind, _)| kind == "capture").unwrap().1.clone();
    assert_eq!(capture, json!({"type": "keydown", "key": "M", "code": "KeyM", "button": null,
        "ctrl": false, "alt": false, "shift": false, "meta": false}));
    manager.unregister("legacy");
    let legacy: Vec<_> = lock(&messages).iter().filter(|(kind, _)| kind == "legacy").map(|(_, v)| v.clone()).collect();
    assert_eq!(legacy, [json!({"id":"legacy","type":"keydown"}), json!({"id":"legacy","type":"keyup"})]);
    manager.stop_capture(second);
    manager.stop();
    assert!(!manager.get_status().hooks_active);
    assert!(lock(&manager.lifecycle).timer.is_none(), "cancel must join timer and remove ownership");
    let count = lock(&messages).len();
    feed(&manager, key(EventKind::KeyUp, 0x4d, 0x32, "M", false));
    assert_eq!(lock(&messages).len(), count);
}

#[test]
fn concurrent_callbacks_and_reset_finish_with_balanced_events() {
    let (manager, messages) = fixture();
    sync(&manager, vec![binding("ptt", combo("M", "KeyM", false))]);
    let manager = Arc::new(manager);
    let worker_manager = Arc::clone(&manager);
    let (finished, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        for _ in 0..100 {
            feed(&worker_manager, key(EventKind::KeyDown, 0x4d, 0x32, "M", false));
            feed(&worker_manager, key(EventKind::KeyUp, 0x4d, 0x32, "M", false));
        }
        finished.send(()).unwrap();
    });
    manager.reset();
    receive.recv_timeout(Duration::from_secs(2)).expect("reset must not deadlock an in-flight sink");
    worker.join().unwrap();
    let result = events(&messages);
    assert_eq!(result.len() % 2, 0);
    for pair in result.chunks_exact(2) { assert_eq!(pair[0]["phase"], "press"); assert_eq!(pair[1]["phase"], "release"); }
    assert!(!manager.get_status().hooks_active);
}

#[test]
fn status_serializes_the_public_windows_contract() {
    let manager = InputHookManager::new(Arc::new(|_, _| {}));
    assert_eq!(manager.handle_request("globalShortcutsGetStatus", &Value::Null).unwrap(), json!({
        "backend": "windows", "platform": "windows", "linux": null, "hooksActive": false,
        "hookError": null, "supportsMouseButtons": true, "supportsModifierOnly": true
    }));
}
