//! Release-safe parity probe. Active only when `FLUXER_PARITY_PROBE=1`; an
//! external harness joins Rust element geometry with the official app's DOM
//! `data-flx` ids (via the authenticated single-instance IPC) and drives the
//! window without OS input. When disabled every hook is a no-op and adds no
//! elements.
//!
//! Geometry is recorded from `canvas` prepaint callbacks (no app/view state is
//! mutated and `cx.notify()` is never called while rendering).

use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use gpui_kit::{
    App, Bounds, IntoElement, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, OperatingSystem, ParentElement, Pixels, PlatformInput, Styled as _, Window,
    canvas, point, px, size,
};
use serde_json::{Value, json};

static ENABLED: OnceLock<bool> = OnceLock::new();

/// True when `FLUXER_PARITY_PROBE=1` (read once).
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var("FLUXER_PARITY_PROBE").is_ok_and(|v| v == "1"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub flx: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub text: Option<String>,
}

#[derive(Default)]
struct Registry {
    building: Vec<Entry>,
    last: Vec<Entry>,
}

impl Registry {
    fn begin_frame(&mut self) {
        self.last = std::mem::take(&mut self.building);
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry { building: Vec::new(), last: Vec::new() });

/// Finishes the previous frame (its entries become the inspectable snapshot).
pub fn begin_frame() {
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.begin_frame();
    }
}

fn record(flx: &str, bounds: Bounds<Pixels>, text: Option<&str>) {
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.building.push(Entry {
            flx: flx.to_owned(),
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            w: f32::from(bounds.size.width),
            h: f32::from(bounds.size.height),
            text: text.map(str::to_owned),
        });
    }
}

/// Entries of the last completed frame.
pub fn snapshot() -> Vec<Entry> {
    REGISTRY.lock().map(|r| r.last.clone()).unwrap_or_default()
}

/// First child of the root element: its prepaint (which runs before every
/// later sibling's) starts a new frame, independent of which views re-render.
pub fn frame_marker() -> impl IntoElement {
    canvas(|_, _, _| begin_frame(), |_, (), _, _| {}).absolute().size_0()
}

/// Window control buttons are drawn inside `gpui_component::TitleBar` (not ours
/// to tag), so their boxes are derived from the titlebar bounds: three square
/// buttons (`TITLE_BAR_HEIGHT` wide) right-aligned as minimize, maximize, close.
pub fn titlebar_controls_marker() -> impl IntoElement {
    canvas(
        |bounds, _, _| {
            let side = f32::from(gpui_kit::component::TITLE_BAR_HEIGHT);
            let right = f32::from(bounds.origin.x) + f32::from(bounds.size.width);
            let ids = [
                "app.native-titlebar.control-button.minimize",
                "app.native-titlebar.control-button.toggle-maximize",
                "app.native-titlebar.control-button.close",
            ];
            for (i, id) in ids.iter().enumerate() {
                let x = right - side * (3 - i) as f32;
                let rect = Bounds { origin: point(px(x), bounds.origin.y), size: size(px(side), bounds.size.height) };
                record(id, rect, Some("derived"));
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .inset_0()
}

/// Tags an element with an official `data-flx` id.
pub trait FlxExt: ParentElement + Sized {
    fn flx(self, id: &'static str) -> Self {
        if !enabled() {
            return self;
        }
        self.child(canvas(move |bounds, _, _| record(id, bounds, None), |_, (), _, _| {}).absolute().inset_0())
    }

    fn flx_text(self, id: &'static str, text: impl Into<String>) -> Self {
        if !enabled() {
            return self;
        }
        let text: String = text.into();
        self.child(
            canvas(move |bounds, _, _| record(id, bounds, Some(&text)), |_, (), _, _| {}).absolute().inset_0(),
        )
    }
}

impl<T: ParentElement + Sized> FlxExt for T {}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Flx { flx: String, index: usize },
    Point { x: f32, y: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeRequest {
    Inspect,
    Resize { w: f32, h: f32 },
    Click(Target),
    Hover(Target),
    Key(String),
}

fn parse_target(val: &Value) -> Option<Target> {
    if let Some(flx) = val.get("flx").and_then(Value::as_str) {
        let index = val.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).unwrap_or(0);
        return Some(Target::Flx { flx: flx.to_owned(), index });
    }
    let x = val.get("x").and_then(Value::as_f64)? as f32;
    let y = val.get("y").and_then(Value::as_f64)? as f32;
    (x.is_finite() && y.is_finite()).then_some(Target::Point { x, y })
}

/// Parses a `probe_*` IPC action. `None` means unknown or malformed.
pub fn parse_request(action: &str, val: &Value) -> Option<ProbeRequest> {
    match action {
        "probe_inspect" => Some(ProbeRequest::Inspect),
        "probe_resize" => {
            let w = val.get("w").and_then(Value::as_f64)? as f32;
            let h = val.get("h").and_then(Value::as_f64)? as f32;
            ((200.0..=8192.0).contains(&w) && (200.0..=8192.0).contains(&h)).then_some(ProbeRequest::Resize { w, h })
        }
        "probe_click" => parse_target(val).map(ProbeRequest::Click),
        "probe_hover" => parse_target(val).map(ProbeRequest::Hover),
        "probe_key" => {
            let k = val.get("keystroke").and_then(Value::as_str)?;
            (!k.is_empty() && k.len() <= 64).then(|| ProbeRequest::Key(k.to_owned()))
        }
        _ => None,
    }
}

fn resolve(target: &Target) -> Option<(f32, f32)> {
    match target {
        Target::Point { x, y } => Some((*x, *y)),
        Target::Flx { flx, index } => snapshot()
            .into_iter()
            .filter(|e| &e.flx == flx)
            .nth(*index)
            .map(|e| (e.x + e.w / 2.0, e.y + e.h / 2.0)),
    }
}

fn viewport_json(window: &Window) -> Value {
    let vp = window.viewport_size();
    json!([f32::from(vp.width), f32::from(vp.height)])
}

fn error_reply(msg: &str) -> String {
    json!({ "ok": false, "error": msg }).to_string()
}

/// Runs a probe request on the UI thread. The reply is sent to `reply` (after
/// the next frame for input/resize requests, immediately for inspect).
pub fn execute(req: ProbeRequest, window: &mut Window, cx: &mut App, reply: Sender<String>) {
    match req {
        ProbeRequest::Inspect => {
            let elements: Vec<Value> = snapshot()
                .into_iter()
                .map(|e| json!({ "flx": e.flx, "rect": [e.x, e.y, e.w, e.h], "text": e.text }))
                .collect();
            let a11y = window
                .debug_a11y_tree_json()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .unwrap_or(Value::Null);
            let out = json!({
                "ok": true,
                "viewport": viewport_json(window),
                "scale": window.scale_factor(),
                "elements": elements,
                "a11y": a11y,
            });
            let _ = reply.send(out.to_string());
        }
        ProbeRequest::Resize { w, h } => {
            window.resize(size(px(w), px(h)));
            window.on_next_frame(move |window, _| {
                let out = json!({ "ok": true, "viewport": viewport_json(window) });
                let _ = reply.send(out.to_string());
            });
        }
        ProbeRequest::Hover(target) => {
            let Some((x, y)) = resolve(&target) else {
                let _ = reply.send(error_reply("element not found"));
                return;
            };
            dispatch_move(window, cx, x, y);
            ack_next_frame(window, reply, x, y);
        }
        ProbeRequest::Click(target) => {
            let Some((x, y)) = resolve(&target) else {
                let _ = reply.send(error_reply("element not found"));
                return;
            };
            dispatch_move(window, cx, x, y);
            let position = point(px(x), px(y));
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Modifiers::none(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Modifiers::none(),
                    click_count: 1,
                }),
                cx,
            );
            ack_next_frame(window, reply, x, y);
        }
        ProbeRequest::Key(spec) => {
            let Ok(keystroke) = Keystroke::parse_with_os(&spec, OperatingSystem::Windows) else {
                let _ = reply.send(error_reply("invalid keystroke"));
                return;
            };
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
            window.on_next_frame(move |_, _| {
                let _ = reply.send(json!({ "ok": true }).to_string());
            });
        }
    }
}

fn dispatch_move(window: &mut Window, cx: &mut App, x: f32, y: f32) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: None,
            modifiers: Modifiers::none(),
        }),
        cx,
    );
}

fn ack_next_frame(window: &mut Window, reply: Sender<String>, x: f32, y: f32) {
    window.on_next_frame(move |_, _| {
        let _ = reply.send(json!({ "ok": true, "at": [x, y] }).to_string());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(flx: &str) -> Entry {
        Entry { flx: flx.to_owned(), x: 1.0, y: 2.0, w: 3.0, h: 4.0, text: None }
    }

    #[test]
    fn frame_swap_replaces_previous_entries() {
        let mut reg = Registry::default();
        reg.building.push(entry("a"));
        reg.begin_frame();
        assert_eq!(reg.last, vec![entry("a")]);
        assert!(reg.building.is_empty());
        reg.building.push(entry("b"));
        reg.building.push(entry("b"));
        reg.begin_frame();
        assert_eq!(reg.last.len(), 2);
        assert!(reg.last.iter().all(|e| e.flx == "b"));
    }

    #[test]
    fn disabled_is_noop() {
        // The test environment never sets FLUXER_PARITY_PROBE.
        if enabled() {
            return;
        }
        let div = gpui_kit::div().flx("x.y").flx_text("x.z", "t");
        drop(div);
        assert!(snapshot().is_empty());
    }

    #[test]
    fn parses_requests() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap_or(Value::Null);
        assert_eq!(parse_request("probe_inspect", &v("{}")), Some(ProbeRequest::Inspect));
        assert_eq!(
            parse_request("probe_resize", &v(r#"{"w":1280,"h":800}"#)),
            Some(ProbeRequest::Resize { w: 1280.0, h: 800.0 })
        );
        assert_eq!(parse_request("probe_resize", &v(r#"{"w":10,"h":800}"#)), None);
        assert_eq!(
            parse_request("probe_click", &v(r#"{"flx":"a.b","index":2}"#)),
            Some(ProbeRequest::Click(Target::Flx { flx: "a.b".into(), index: 2 }))
        );
        assert_eq!(
            parse_request("probe_hover", &v(r#"{"x":5,"y":6}"#)),
            Some(ProbeRequest::Hover(Target::Point { x: 5.0, y: 6.0 }))
        );
        assert_eq!(parse_request("probe_click", &v("{}")), None);
        assert_eq!(
            parse_request("probe_key", &v(r#"{"keystroke":"escape"}"#)),
            Some(ProbeRequest::Key("escape".into()))
        );
        assert_eq!(parse_request("probe_nope", &v("{}")), None);
    }
}
