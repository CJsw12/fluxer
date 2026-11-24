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
    MouseMoveEvent, MouseUpEvent, OperatingSystem, ParentElement, Pixels, PlatformInput, Styled, Window,
    AbsoluteLength, StyleRefinement, canvas, point, px, size,
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
    /// `file:line` of the `.flx` call (repo-relative when possible).
    pub src: Option<String>,
    /// Declared (non-hover) style, only the fields that are set.
    pub style: Option<Value>,
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

fn record(flx: &str, bounds: Bounds<Pixels>, text: Option<&str>, src: &str, style: &Option<Value>) {
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.building.push(Entry {
            flx: flx.to_owned(),
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            w: f32::from(bounds.size.width),
            h: f32::from(bounds.size.height),
            text: text.map(str::to_owned),
            src: Some(src.to_owned()),
            style: style.clone(),
        });
    }
}

/// Entries of the latest completed frame.
pub fn snapshot() -> Vec<Entry> {
    REGISTRY
        .lock()
        .map(|r| if !r.building.is_empty() { r.building.clone() } else { r.last.clone() })
        .unwrap_or_default()
}

/// First child of the root element: its prepaint (which runs before every
/// later sibling's) starts a new frame, independent of which views re-render.
pub fn frame_marker() -> impl IntoElement {
    canvas(|_, _, _| begin_frame(), |_, (), _, _| {}).absolute().size_0()
}

/// Repo-relative forward-slash path of a `Location::caller()` file plus line.
fn format_src(file: &str, line: u32) -> String {
    let f = file.replace('\\', "/");
    let f = match f.find("fluxer_client/") {
        Some(i) => f[i..].to_owned(),
        None => f,
    };
    format!("{f}:{line}")
}

/// Serializes only the set fields of a declared style. Hover/active/focus
/// styles are `pub(crate)` in gpui and cannot be read, so they are skipped.
fn style_json(style: &impl serde::Serialize) -> Option<Value> {
    let v = serde_json::to_value(style).ok()?;
    prune(v)
}

/// Drops nulls and empty objects/arrays recursively.
fn prune(v: Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::Object(m) => {
            let m: serde_json::Map<String, Value> =
                m.into_iter().filter_map(|(k, v)| prune(v).map(|v| (k, v))).collect();
            if m.is_empty() { None } else { Some(Value::Object(m)) }
        }
        Value::Array(a) if a.is_empty() => None,
        other => Some(other),
    }
}

/// Declared border widths `[top, right, bottom, left]` in px (rem = 16px).
/// An absolutely positioned child is laid out against the padding box, so the
/// recording canvas is pushed out by these amounts to measure the border box
/// like the official DOM rects. Borders set after `.flx(..)` in the builder
/// chain are not seen (same limit as the serialized style).
fn border_px(style: &StyleRefinement) -> [f32; 4] {
    let len = |l: &Option<AbsoluteLength>| match l {
        Some(AbsoluteLength::Pixels(p)) => f32::from(*p),
        Some(AbsoluteLength::Rems(r)) => r.0 * 16.0,
        None => 0.0,
    };
    let b = &style.border_widths;
    [len(&b.top), len(&b.right), len(&b.bottom), len(&b.left)]
}

/// Tags an element with an official `data-flx` id. Every method does nothing
/// unless the probe is enabled, so Location capture (free, compile-time
/// constant), serialization and allocation only happen with the probe on.
pub trait FlxExt: ParentElement + Styled + Sized {
    #[track_caller]
    fn flx(mut self, id: &'static str) -> Self {
        if !enabled() {
            return self;
        }
        let loc = std::panic::Location::caller();
        let src = format_src(loc.file(), loc.line());
        let [bt, br, bb, bl] = border_px(self.style());
        let style = style_json(self.style());
        self.child(
            canvas(move |bounds, _, _| record(id, bounds, None, &src, &style), |_, (), _, _| {})
                .absolute()
                .top(px(-bt))
                .right(px(-br))
                .bottom(px(-bb))
                .left(px(-bl)),
        )
    }

    #[track_caller]
    fn flx_text(mut self, id: &'static str, text: impl Into<String>) -> Self {
        if !enabled() {
            return self;
        }
        let loc = std::panic::Location::caller();
        let src = format_src(loc.file(), loc.line());
        let [bt, br, bb, bl] = border_px(self.style());
        let style = style_json(self.style());
        let text: String = text.into();
        self.child(
            canvas(move |bounds, _, _| record(id, bounds, Some(&text), &src, &style), |_, (), _, _| {})
                .absolute()
                .top(px(-bt))
                .right(px(-br))
                .bottom(px(-bb))
                .left(px(-bl)),
        )
    }
}

impl<T: ParentElement + Styled + Sized> FlxExt for T {}

fn entry_json(e: Entry) -> Value {
    json!({ "flx": e.flx, "rect": [e.x, e.y, e.w, e.h], "text": e.text, "src": e.src, "style": e.style })
}

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
    DoubleClick(Target),
    Drag { from: Target, to: (f32, f32), steps: usize },
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
        "probe_dblclick" => parse_target(val).map(ProbeRequest::DoubleClick),
        "probe_drag" => {
            let from = val.get("from").and_then(parse_target).or_else(|| parse_target(val))?;
            let to_x = val.get("to_x").and_then(Value::as_f64)? as f32;
            let to_y = val.get("to_y").and_then(Value::as_f64)? as f32;
            let steps = val
                .get("steps")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(8)
                .clamp(1, 30);
            (to_x.is_finite() && to_y.is_finite()).then_some(ProbeRequest::Drag {
                from,
                to: (to_x, to_y),
                steps,
            })
        }
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
                .map(entry_json)
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
        ProbeRequest::DoubleClick(target) => {
            let Some((x, y)) = resolve(&target) else {
                let _ = reply.send(error_reply("element not found"));
                return;
            };
            dispatch_move(window, cx, x, y);
            let position = point(px(x), px(y));
            for click_count in [1, 2] {
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Modifiers::none(),
                        click_count,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Modifiers::none(),
                        click_count,
                    }),
                    cx,
                );
            }
            ack_next_frame(window, reply, x, y);
        }
        ProbeRequest::Drag {
            from,
            to: (to_x, to_y),
            steps,
        } => {
            let Some((x1, y1)) = resolve(&from) else {
                let _ = reply.send(error_reply("element not found"));
                return;
            };
            dispatch_move(window, cx, x1, y1);
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(px(x1), px(y1)),
                    modifiers: Modifiers::none(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.on_next_frame(move |window, cx| {
                for step in 1..=steps {
                    let t = step as f32 / steps as f32;
                    let x = x1 + (to_x - x1) * t;
                    let y = y1 + (to_y - y1) * t;
                    window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: point(px(x), px(y)),
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Modifiers::none(),
                        }),
                        cx,
                    );
                }
                window.on_next_frame(move |window, cx| {
                    window.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position: point(px(to_x), px(to_y)),
                            modifiers: Modifiers::none(),
                            click_count: 1,
                        }),
                        cx,
                    );
                    window.on_next_frame(move |_, _| {
                        let _ = reply.send(json!({ "ok": true, "from": [x1, y1], "to": [to_x, to_y] }).to_string());
                    });
                });
            });
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
        Entry { flx: flx.to_owned(), x: 1.0, y: 2.0, w: 3.0, h: 4.0, text: None, src: None, style: None }
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
    fn style_serializes_only_set_fields() {
        let mut d = gpui_kit::div().p(px(4.0)).gap(px(2.0)).bg(gpui_kit::rgb(0x112233));
        let v = style_json(d.style()).unwrap_or(Value::Null);
        let keys: Vec<&String> = v.as_object().map(|m| m.keys().collect()).unwrap_or_default();
        eprintln!("style = {v}");
        assert!(keys.iter().all(|k| ["padding", "gap", "background"].contains(&k.as_str())), "{keys:?}");
        assert!(keys.contains(&&"padding".to_owned()) && keys.contains(&&"gap".to_owned()));
        assert!(style_json(gpui_kit::div().style()).is_none());
    }

    #[test]
    fn entry_json_shape_with_src_and_style() {
        let e = Entry {
            src: Some(format_src("C:\\x\\fluxer_client\\app\\src\\ui\\x.rs", 123)),
            style: Some(json!({ "gap": { "width": { "Definite": 2.0 } } })),
            ..entry("a.b")
        };
        let v = entry_json(e);
        eprintln!("entry = {v}");
        assert_eq!(v["flx"], "a.b");
        assert_eq!(v["rect"], json!([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(v["src"], "fluxer_client/app/src/ui/x.rs:123");
        assert!(v["style"]["gap"].is_object());
        assert!(v["text"].is_null());
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
            parse_request("probe_dblclick", &v(r#"{"flx":"a.b"}"#)),
            Some(ProbeRequest::DoubleClick(Target::Flx { flx: "a.b".into(), index: 0 }))
        );
        assert_eq!(
            parse_request("probe_drag", &v(r#"{"flx":"h.b","to_x":250,"to_y":400,"steps":12}"#)),
            Some(ProbeRequest::Drag {
                from: Target::Flx { flx: "h.b".into(), index: 0 },
                to: (250.0, 400.0),
                steps: 12,
            })
        );
        assert_eq!(parse_request("probe_drag", &v(r#"{"flx":"h.b"}"#)), None);
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
