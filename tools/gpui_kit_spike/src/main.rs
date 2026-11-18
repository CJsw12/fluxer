//! gpui-kit 0.10.0 (gpui-pre =0.3.12) UI 가능성 검증 시제품 2호.
//! 5만 행 가변 높이 메시지 목록(MessageScroller) + Markdown(TextView) + Input.
//! 실행: `gpui_kit_spike [--bench | --bench-plain] [--step=<items>]`

mod mem;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::text::TextView;
use gpui_kit::{
    App, Bounds, Context, Entity, EntityInputHandler as _, Focusable as _, ListOffset, SharedString, Subscription,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};

const MESSAGE_COUNT: usize = 50_000;
const BENCH_TIMEOUT: Duration = Duration::from_secs(150);
static BENCH_FAILED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq)]
enum BenchKind {
    Markdown,
    Plain,
}

struct Message {
    author: SharedString,
    time: SharedString,
    body: SharedString,
}

/// (본문 조각, 줄 수). 조각은 마크다운 블록 하나다.
const PIECES: &[(&str, usize)] = &[
    ("안녕하세요, **오늘 회의 자료** 공유드립니다.", 1),
    (
        "Hello everyone, the build is *green* again 🎉 see [release notes](https://example.com/notes)",
        1,
    ),
    ("점심 뭐 먹을까요? 🍜🍣 `cargo build --release` 돌려놓고 갈게요", 1),
    ("- 첫째 항목 **굵게**\n- 둘째 항목 `inline_code`\n- 세 번째 항목 한글 😀", 3),
    ("```rust\nfn main() {\n    println!(\"안녕, Fluxer 🚀\");\n}\n```", 5),
    (
        "음성 채널에서 **화면 공유**가 끊기는 현상 로그: `ERR_STREAM_RESET` 👀 [이슈](https://example.com/issues/42)",
        1,
    ),
    ("1. 빌드\n2. 테스트\n3. 배포 🚢", 3),
    ("ㅋㅋㅋㅋ 그거 완전 맞는 말이네요 😂😂", 1),
    (
        "Mixed 한글 and English 문장이 줄바꿈될 때 line height가 어떻게 계산되는지 보려고 길게 씁니다. **bold**, `code`, [link](https://example.com) 모두 섞었어요 🧪",
        2,
    ),
    ("ok", 1),
    ("👍", 1),
    ("```\nplain block\nsecond line\n```", 4),
];
const AUTHORS: &[&str] = &["민수", "Alice", "지은", "Bob", "하늘", "Fluxer Bot"];

fn synth_messages(n: usize) -> Vec<Message> {
    // 재현 가능한 LCG (gpui_spike와 같은 방식).
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    (0..n)
        .map(|i| {
            let target = 1 + next() % 10; // 1~10줄
            let mut lines = 0;
            let mut parts: Vec<&str> = Vec::new();
            while lines < target {
                let (text, l) = PIECES[next() % PIECES.len()];
                parts.push(text);
                lines += l;
            }
            Message {
                author: AUTHORS[next() % AUTHORS.len()].into(),
                time: format!("{:02}:{:02}", (i / 60) % 24, i % 60).into(),
                body: parts.join("\n\n").into(),
            }
        })
        .collect()
}

struct Bench {
    kind: BenchKind,
    step: usize,
    started: Instant,
    pos: usize,
    scrolled_to_top: bool,
    start_mem: mem::MemInfo,
    last_render: Option<Instant>,
    intervals_us: Vec<u32>,
}

struct Root {
    scroller: Entity<MessageScrollerState>,
    messages: Rc<RefCell<Vec<Message>>>,
    input: Entity<InputState>,
    bench: Option<Bench>,
    plain: bool,
    frames: u64,
    _sub: Subscription,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>, bench: Option<(BenchKind, usize)>) -> Self {
        let messages = Rc::new(RefCell::new(synth_messages(MESSAGE_COUNT)));
        let scroller = cx.new(|cx| MessageScrollerState::new(MESSAGE_COUNT, cx));
        // 맨 아래부터 시작.
        scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("메시지 보내기 (Enter)"));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.submit(input, window, cx);
                }
            },
        );
        let plain = matches!(bench, Some((BenchKind::Plain, _)));
        let bench = bench.map(|(kind, step)| Bench {
            kind,
            step,
            started: Instant::now(),
            pos: 0,
            scrolled_to_top: false,
            start_mem: mem::process_memory().unwrap_or_default(),
            last_render: None,
            intervals_us: Vec::with_capacity(20_000),
        });
        Self { scroller, messages, input, bench, plain, frames: 0, _sub: sub }
    }

    fn submit(&mut self, input: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
        // IME 조합 중 Enter는 확정 용도이므로 전송하지 않는다 (가능한 범위).
        let composing = input
            .update(cx, |s, cx| s.marked_text_range(window, cx))
            .is_some();
        if composing {
            return;
        }
        let text = input.read(cx).value();
        if text.trim().is_empty() {
            return;
        }
        self.messages.borrow_mut().push(Message {
            author: "me".into(),
            time: "now".into(),
            body: text,
        });
        input.update(cx, |s, cx| s.set_value("", window, cx));
        self.scroller.update(cx, |s, cx| {
            s.append(1, cx);
            s.scroll_to_end(cx);
        });
        cx.notify();
    }

    /// 프레임마다 목록을 `step` 행 스크롤하고, 끝에 닿으면 결과를 출력하고 종료한다.
    fn drive_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.bench.as_mut() else { return };
        let now = Instant::now();
        if let Some(prev) = b.last_render.replace(now) {
            if b.scrolled_to_top {
                let us = now.duration_since(prev).as_micros().min(u32::MAX as u128) as u32;
                b.intervals_us.push(us);
            }
        }
        let count = MESSAGE_COUNT;
        if !b.scrolled_to_top {
            self.scroller.update(cx, |s, cx| {
                s.scroll_to_offset(ListOffset::default(), cx);
            });
            b.scrolled_to_top = true;
            b.started = Instant::now();
            b.pos = 0;
            b.intervals_us.clear();
            self.frames = 0;
        } else {
            b.pos += b.step;
            let pos = b.pos.min(count - 1);
            self.scroller.update(cx, |s, cx| {
                s.scroll_to_offset(ListOffset { item_ix: pos, offset_in_item: px(0.) }, cx);
            });
        }
        let top = self.scroller.read(cx).logical_scroll_top().item_ix;
        let reached_end = b.scrolled_to_top && self.frames > 1 && b.pos + 40 >= count;
        let timed_out = b.started.elapsed() > BENCH_TIMEOUT;
        if reached_end || timed_out {
            let elapsed = b.started.elapsed();
            let end_mem = mem::process_memory().unwrap_or_default();
            let mb = |v: u64| v as f64 / (1024.0 * 1024.0);
            let mut iv = std::mem::take(&mut b.intervals_us);
            iv.sort_unstable();
            let pct = |p: f64| -> f64 {
                if iv.is_empty() {
                    return 0.0;
                }
                let ix = ((iv.len() as f64 - 1.0) * p).round() as usize;
                iv[ix] as f64 / 1000.0
            };
            println!(
                "bench_result: {} ({})",
                if reached_end { "ok" } else { "timeout" },
                if b.kind == BenchKind::Markdown { "markdown" } else { "plain" }
            );
            println!("items: {count}  step_items: {}", b.step);
            println!("elapsed_ms: {}", elapsed.as_millis());
            println!(
                "render_frames: {}  avg_fps: {:.1}",
                self.frames,
                self.frames as f64 / elapsed.as_secs_f64().max(1e-9)
            );
            println!(
                "frame_interval_ms: p50={:.2} p99={:.2} max={:.2}",
                pct(0.50),
                pct(0.99),
                pct(1.0)
            );
            println!("final_top_item: {top}");
            println!(
                "mem_start: private={:.1}MB peak_ws={:.1}MB",
                mb(b.start_mem.private_usage),
                mb(b.start_mem.peak_working_set)
            );
            println!(
                "mem_end: private={:.1}MB peak_ws={:.1}MB",
                mb(end_mem.private_usage),
                mb(end_mem.peak_working_set)
            );
            if timed_out && !reached_end {
                BENCH_FAILED.store(true, Ordering::SeqCst);
            }
            cx.quit();
        } else {
            window.request_animation_frame();
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        self.drive_bench(window, cx);
        let messages = self.messages.clone();
        let plain = self.plain;
        let tokens = cx.theme().semantic_tokens();
        let (fg, muted, bg) = (
            tokens.colors.foreground,
            tokens.colors.muted_foreground,
            tokens.colors.background,
        );
        let rows = MessageScroller::new("messages", self.scroller.clone(), move |ix, _, _| {
            let msgs = messages.borrow();
            let Some(m) = msgs.get(ix) else { return div().into_any_element() };
            let body = if plain {
                div().child(m.body.clone()).into_any_element()
            } else {
                TextView::markdown(("msg", ix), m.body.clone())
                    .selectable(true)
                    .into_any_element()
            };
            div()
                .flex()
                .flex_col()
                .w_full()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(div().text_color(fg).child(m.author.clone()))
                        .child(div().text_color(muted).text_xs().child(m.time.clone())),
                )
                .child(body)
                .into_any_element()
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .text_size(px(14.))
            .child(div().flex_1().min_h_0().child(rows))
            .child(div().w_full().p_3().child(Input::new(&self.input)))
    }
}

fn run() -> Result<()> {
    let mut kind: Option<BenchKind> = None;
    let mut step = 12usize;
    for arg in std::env::args().skip(1) {
        if arg == "--bench" {
            kind = Some(BenchKind::Markdown);
        } else if arg == "--bench-plain" {
            kind = Some(BenchKind::Plain);
        } else if let Some(v) = arg.strip_prefix("--step=") {
            step = v.parse::<usize>().map_err(|e| anyhow!("bad --step: {e}"))?;
        } else {
            return Err(anyhow!("unknown argument: {arg}"));
        }
    }
    let bench = kind.map(|k| (k, step));

    gpui_kit::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(900.), px(700.)), cx);
        let opened = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| {
                let root = cx.new(|cx| Root::new(window, cx, bench));
                let input = root.read(cx).input.clone();
                let focus = input.focus_handle(cx);
                window.focus(&focus, cx);
                root
            },
        );
        match opened {
            Ok(_) => cx.activate(true),
            Err(e) => {
                eprintln!("open_window failed: {e:#}");
                BENCH_FAILED.store(true, Ordering::SeqCst);
                cx.quit();
            }
        }
    });

    if BENCH_FAILED.load(Ordering::SeqCst) {
        return Err(anyhow!("bench failed (timeout or window error)"));
    }
    Ok(())
}

fn main() -> Result<()> {
    run()
}

#[cfg(test)]
mod tests {
    //! 헤드리스 창에서 목록 메시지 본문을 마우스로 드래그해 창 단위 텍스트 선택이 되는지 본다.
    use super::*;
    use gpui_kit::base::TextSelection;
    use gpui_kit::{Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, point};

    fn open(cx: &mut TestAppContext) -> (Entity<Root>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        let mut content = None;
        let (_, vcx) = cx.add_window_view(|window, cx| {
            let root = cx.new(|cx| Root::new(window, cx, None));
            let focus = root.read(cx).input.focus_handle(cx);
            window.focus(&focus, cx);
            content = Some(root.clone());
            gpui_kit::base::Root::new(root, window, cx)
        });
        let content = content.expect("window built its content");
        vcx.run_until_parked();
        draw(vcx);
        (content, vcx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    /// 목록 아래쪽에서 위로 훑어 선택 가능한 본문 텍스트가 있는 첫 지점을 찾는다.
    /// `is_selectable_at`은 hitbox hover 상태를 쓰므로 지점마다 포인터를 먼저 옮기고 프레임을 그린다.
    fn find_selectable_from_bottom(cx: &mut VisualTestContext, x: Pixels) -> Option<Point<Pixels>> {
        let height = cx.update(|window, _| window.viewport_size().height);
        let mut y = height - px(1.);
        while y > px(0.) {
            let p = point(x, y);
            cx.simulate_mouse_move(p, None, Modifiers::default());
            draw(cx);
            if cx.update(|window, cx| TextSelection::is_selectable_at(p, window, cx)) {
                return Some(p);
            }
            y -= px(3.);
        }
        eprintln!("viewport height {height:?}: no selectable point at x={x:?}");
        None
    }

    /// 왼쪽 여백에서 누르고 본문을 지나 줄 끝 너머까지 끈다. 실제 포인터처럼 중간 지점을 거친다.
    /// (누른 지점과 놓은 지점이 모두 텍스트 밖이고 중간 이동이 없으면 라이브러리는 빈 공간 드래그로
    /// 보고 아무것도 선택하지 않는다. 실제 마우스에서는 중간 MouseMove가 항상 온다.)
    fn drag_across_line(cx: &mut VisualTestContext, y: Pixels) -> String {
        cx.simulate_mouse_down(point(px(1.), y), MouseButton::Left, Modifiers::default());
        draw(cx);
        for x in [50., 100., 200., 400., 600.] {
            cx.simulate_mouse_move(point(px(x), y), Some(MouseButton::Left), Modifiers::default());
            draw(cx);
        }
        cx.simulate_mouse_up(point(px(600.), y), MouseButton::Left, Modifiers::default());
        draw(cx);
        cx.update(|window, cx| TextSelection::selected_text(window, cx))
    }

    #[gpui_kit::test]
    fn synthetic_message_body_is_drag_selectable(cx: &mut TestAppContext) {
        let (_, cx) = open(cx);
        let start = find_selectable_from_bottom(cx, px(30.))
            .expect("no selectable message text found in the list");
        let text = drag_across_line(cx, start.y);
        assert!(!text.is_empty(), "dragging over a synthetic message selected nothing");
    }

    #[gpui_kit::test]
    fn submitted_message_body_is_drag_selectable(cx: &mut TestAppContext) {
        let (root, cx) = open(cx);
        cx.simulate_input("selectme hello world");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        draw(cx);
        draw(cx);
        let count = root.read_with(cx, |r, _| r.messages.borrow().len());
        assert_eq!(count, MESSAGE_COUNT + 1, "Enter did not append the message");
        let start = find_selectable_from_bottom(cx, px(30.))
            .expect("no selectable message text found in the list");
        let text = drag_across_line(cx, start.y);
        assert!(
            text.contains("selectme"),
            "dragging over the submitted message did not select it: {text:?}"
        );
    }
}
