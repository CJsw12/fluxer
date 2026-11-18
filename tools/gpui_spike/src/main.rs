//! GPUI 메인 UI 가능성 검증용 시제품: 가변 높이 2만 행 목록 + IME 대응 한 줄 입력.
//! 실행: `gpui_spike [--bench] [--step=<px>]`

mod input;
mod mem;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use gpui::{
    AnyElement, App, Application, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding,
    ListAlignment, ListOffset, ListState, SharedString, Window, WindowBounds, WindowOptions, div,
    list, prelude::*, px, rgb, size,
};

use input::{
    Backspace, Copy, Cut, Delete, End, Enter, Home, Left, Paste, Right, SelectAll, SelectLeft,
    SelectRight, Submit, TextInput,
};

const MESSAGE_COUNT: usize = 20_000;
const BENCH_TIMEOUT: Duration = Duration::from_secs(100);
static BENCH_FAILED: AtomicBool = AtomicBool::new(false);

struct Message {
    author: SharedString,
    time: SharedString,
    body: SharedString,
}

/// 한글, 영문, 이모지를 섞은 합성 문장 풀.
const SNIPPETS: &[&str] = &[
    "안녕하세요, 오늘 회의 자료 공유드립니다.",
    "Hello everyone, the build is green again 🎉",
    "점심 뭐 먹을까요? 🍜🍣 저는 아무거나 좋아요",
    "Can someone review my PR? It touches the gateway session handling and a few tests.",
    "음성 채널에서 화면 공유가 가끔 끊기는 현상이 있어서 로그를 첨부합니다 👀",
    "ㅋㅋㅋㅋ 그거 완전 맞는 말이네요 😂😂",
    "LGTM! ship it 🚀",
    "한글 입력 중에 후보창이 입력창 위치를 따라가는지 확인해 주세요. 조합 중 밑줄도 필요합니다.",
    "Mixed 한글 and English 문장이 줄바꿈될 때 line height가 어떻게 계산되는지 보려고 길게 씁니다 🧪",
    "ok",
    "👍",
];
const AUTHORS: &[&str] = &["민수", "Alice", "지은", "Bob", "하늘", "Fluxer Bot"];

fn synth_messages(n: usize) -> Vec<Message> {
    // 재현 가능한 LCG (외부 난수 의존 없음).
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    (0..n)
        .map(|i| {
            let lines = 1 + next() % 6;
            let body = (0..lines)
                .map(|_| SNIPPETS[next() % SNIPPETS.len()])
                .collect::<Vec<_>>()
                .join("\n");
            Message {
                author: AUTHORS[next() % AUTHORS.len()].into(),
                time: format!("{:02}:{:02}", (i / 60) % 24, i % 60).into(),
                body: body.into(),
            }
        })
        .collect()
}

fn render_message(m: &Message) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .w_full()
        .px_4()
        .py_2()
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(div().text_color(rgb(0xf2f3f5)).child(m.author.clone()))
                .child(div().text_color(rgb(0x949ba4)).text_xs().child(m.time.clone())),
        )
        .child(div().text_color(rgb(0xdbdee1)).child(m.body.clone()))
        .into_any_element()
}

struct Bench {
    started: Instant,
    step: f32,
    scrolled_to_top: bool,
    start_mem: mem::MemInfo,
}

struct Root {
    list_state: ListState,
    messages: Rc<RefCell<Vec<Message>>>,
    input: Entity<TextInput>,
    focus_handle: FocusHandle,
    bench: Option<Bench>,
    frames: u64,
}

impl Root {
    fn new(cx: &mut Context<Self>, bench_step: Option<f32>) -> Self {
        let messages = Rc::new(RefCell::new(synth_messages(MESSAGE_COUNT)));
        let list_state = ListState::new(MESSAGE_COUNT, ListAlignment::Bottom, px(1200.));
        let input = cx.new(|cx| TextInput::new(cx, "메시지 보내기 (Enter)"));
        cx.subscribe(&input, |this: &mut Self, _, ev: &Submit, cx| {
            let count = {
                let mut msgs = this.messages.borrow_mut();
                msgs.push(Message {
                    author: "me".into(),
                    time: "now".into(),
                    body: ev.0.clone().into(),
                });
                msgs.len()
            };
            this.list_state.splice(count - 1..count - 1, 1);
            // 끝(item_count)으로 이동하면 Bottom 정렬에서 맨 아래로 붙는다.
            this.list_state.scroll_to(ListOffset { item_ix: count, offset_in_item: px(0.) });
            cx.notify();
        })
        .detach();
        let bench = bench_step.map(|step| Bench {
            started: Instant::now(),
            step,
            scrolled_to_top: false,
            start_mem: mem::process_memory().unwrap_or_default(),
        });
        Self { list_state, messages, input, focus_handle: cx.focus_handle(), bench, frames: 0 }
    }

    /// 프레임마다 목록을 한 단계 스크롤하고, 끝에 닿으면 결과를 출력하고 종료한다.
    fn drive_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.bench.as_mut() else { return };
        let count = self.list_state.item_count();
        if !b.scrolled_to_top {
            self.list_state.scroll_to(ListOffset::default());
            b.scrolled_to_top = true;
            b.started = Instant::now();
            self.frames = 0;
        } else {
            self.list_state.scroll_by(px(b.step));
        }
        let top = self.list_state.logical_scroll_top().item_ix;
        let reached_end = b.scrolled_to_top && self.frames > 1 && top + 40 >= count;
        let timed_out = b.started.elapsed() > BENCH_TIMEOUT;
        if reached_end || timed_out {
            let elapsed = b.started.elapsed();
            let end_mem = mem::process_memory().unwrap_or_default();
            let mb = |v: u64| v as f64 / (1024.0 * 1024.0);
            println!("bench_result: {}", if reached_end { "ok" } else { "timeout" });
            println!("items: {count}  step_px: {}", b.step);
            println!("elapsed_ms: {}", elapsed.as_millis());
            println!(
                "render_frames: {}  avg_fps: {:.1}",
                self.frames,
                self.frames as f64 / elapsed.as_secs_f64().max(1e-9)
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

impl Focusable for Root {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        self.drive_bench(window, cx);
        let messages = self.messages.clone();
        let rows = list(self.list_state.clone(), move |ix, _, _| match messages.borrow().get(ix) {
            Some(m) => render_message(m),
            None => div().into_any_element(),
        })
        .size_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x313338))
            .text_color(rgb(0xdbdee1))
            .text_size(px(14.))
            .child(div().flex_1().min_h_0().child(rows))
            .child(div().w_full().p_3().child(self.input.clone()))
    }
}

fn run() -> Result<()> {
    let mut bench_step = None;
    for arg in std::env::args().skip(1) {
        if arg == "--bench" {
            bench_step.get_or_insert(1500.0);
        } else if let Some(v) = arg.strip_prefix("--step=") {
            bench_step = Some(v.parse::<f32>().map_err(|e| anyhow!("bad --step: {e}"))?);
        } else {
            return Err(anyhow!("unknown argument: {arg}"));
        }
    }

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("backspace", Backspace, Some("TextInput")),
            KeyBinding::new("delete", Delete, Some("TextInput")),
            KeyBinding::new("left", Left, Some("TextInput")),
            KeyBinding::new("right", Right, Some("TextInput")),
            KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
            KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
            KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
            KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
            KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
            KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
            KeyBinding::new("home", Home, Some("TextInput")),
            KeyBinding::new("end", End, Some("TextInput")),
            KeyBinding::new("enter", Enter, Some("TextInput")),
        ]);
        let bounds = Bounds::centered(None, size(px(900.), px(700.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let root = cx.new(|cx| Root::new(cx, bench_step));
                let input_focus = root.read(cx).input.focus_handle(cx);
                window.focus(&input_focus);
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
