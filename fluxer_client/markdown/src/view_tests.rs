//! Headless GPUI tests: the element in a real (virtual) window with the
//! window-scoped text selection, driven by simulated mouse input.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{
    AppContext as _, Context, Entity, IntoElement, Modifiers, MouseButton, ParentElement, Pixels, Point, Render,
    Styled, TestAppContext, VisualTestContext, Window, div, point, px,
};
use gpui_kit::base::TextSelection;
use gpui_kit::base::text::TextViewDefaults;

use crate::model::{Document, MentionTarget};
use crate::model_tests::doc;
use crate::view::{MessageMarkdown, SpoilerReveals};

static MENTION_CLICKS: AtomicUsize = AtomicUsize::new(0);

struct Host {
    doc: Arc<Document>,
    spoilers: Entity<SpoilerReveals>,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(700.)).flex().flex_col().child(
            MessageMarkdown::new("msg", self.doc.clone()).spoiler_state(self.spoilers.clone()).on_mention_click(
                |target, _, _| {
                    if *target == MentionTarget::User("1".to_owned()) {
                        MENTION_CLICKS.fetch_add(1, Ordering::SeqCst);
                    }
                },
            ),
        )
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn open<'a>(cx: &'a mut TestAppContext, content: &str) -> (Entity<SpoilerReveals>, &'a mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let doc = Arc::new(doc(content));
    let mut spoilers = None;
    let (_, vcx) = cx.add_window_view(|window, cx| {
        let state = cx.new(|_| SpoilerReveals::default());
        spoilers = Some(state.clone());
        let host = cx.new(|_| Host { doc, spoilers: state });
        gpui_kit::base::Root::new(host, window, cx)
    });
    vcx.run_until_parked();
    draw(vcx);
    (spoilers.expect("window built"), vcx)
}

/// Topmost and bottommost rows (at `x`) with selectable text.
fn selectable_rows(cx: &mut VisualTestContext, x: Pixels) -> (Pixels, Pixels) {
    let mut first = None;
    let mut last = None;
    let mut y = px(1.);
    while y < px(900.) {
        let p = point(x, y);
        cx.simulate_mouse_move(p, None, Modifiers::default());
        draw(cx);
        if cx.update(|window, cx| TextSelection::is_selectable_at(p, window, cx)) {
            first.get_or_insert(y);
            last = Some(y);
        }
        y += px(2.);
    }
    (first.expect("some selectable text"), last.expect("some selectable text"))
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) -> String {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    draw(cx);
    let steps = 8;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let p = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        cx.simulate_mouse_move(p, Some(MouseButton::Left), Modifiers::default());
        draw(cx);
    }
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    draw(cx);
    cx.update(|window, cx| TextSelection::selected_text(window, cx))
}

const EVERYTHING: &str = "\
**Bold** *it* __under__ ~~strike~~ `code` and [link](https://x.y)
hey <@1> in <#10> for <@&20> <:blob:42> at <t:1795017909:d>
secret: ||hidden words|| end
# Heading
> quoted
- one
- two
```rs
fn main() {}
```
-# small print";

#[gpui::test]
fn drag_select_across_every_construct_copies_plain_text(cx: &mut TestAppContext) {
    let (_, cx) = open(cx, EVERYTHING);
    let x = px(3.);
    let (top, bottom) = selectable_rows(cx, x);
    let selected = drag(cx, point(px(1.), top), point(px(690.), bottom));
    eprintln!("selected: {selected:?}");
    let expected_in_order = [
        "Bold it under strike code and link",
        "hey @Alice in general for @Mods <:blob:42> at 11/18/2026",
        "secret: hidden words end",
        "Heading",
        "quoted",
        "one",
        "two",
        "fn main() {}",
        "small print",
    ];
    let mut rest = selected.as_str();
    for piece in expected_in_order {
        let at = rest.find(piece).unwrap_or_else(|| panic!("{piece:?} not in order in {selected:?}"));
        rest = &rest[at + piece.len()..];
    }
    for markup in ["**", "__", "~~", "`", "fluxer-md", "<strong>", "\\", "&#"] {
        assert!(!selected.contains(markup), "{markup:?} leaked into {selected:?}");
    }
}

#[gpui::test]
fn line_break_after_an_emoji_survives_copy(cx: &mut TestAppContext) {
    let (_, cx) = open(cx, "<:blob:42>\nnext <@1>\nlast");
    let (top, bottom) = selectable_rows(cx, px(3.));
    let selected = drag(cx, point(px(1.), top), point(px(690.), bottom));
    // The space before each newline is the `hard_break` workaround.
    assert_eq!(selected.trim_end(), "<:blob:42> \nnext @Alice \nlast");
}

#[gpui::test]
fn partial_drag_inside_one_line(cx: &mut TestAppContext) {
    let (_, cx) = open(cx, "alpha **beta** gamma");
    let (top, _) = selectable_rows(cx, px(3.));
    let selected = drag(cx, point(px(1.), top + px(2.)), point(px(690.), top + px(2.)));
    assert_eq!(selected.trim_end(), "alpha beta gamma");
}

#[gpui::test]
fn clicking_a_hidden_spoiler_reveals_it(cx: &mut TestAppContext) {
    let (spoilers, cx) = open(cx, "||secret|| and ||other||");
    let (top, _) = selectable_rows(cx, px(3.));
    // The first spoiler starts the line.
    cx.simulate_click(point(px(5.), top + px(4.)), Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    spoilers.read_with(cx, |state, _| {
        assert!(state.is_revealed(0), "first spoiler revealed");
        assert!(!state.is_revealed(1), "second spoiler still hidden");
    });
    // Revealed text is plain selectable text now; hidden words copy as text
    // too. The re-parsed view registers its text again on the next frames.
    let (top, _) = selectable_rows(cx, px(3.));
    let selected = drag(cx, point(px(1.), top + px(4.)), point(px(690.), top + px(4.)));
    assert_eq!(selected.trim_end(), "secret and other");
}

#[gpui::test]
fn clicking_a_known_mention_calls_the_handler(cx: &mut TestAppContext) {
    let (_, cx) = open(cx, "<@1> hi");
    let (top, _) = selectable_rows(cx, px(3.));
    let before = MENTION_CLICKS.load(Ordering::SeqCst);
    cx.simulate_click(point(px(6.), top + px(4.)), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(MENTION_CLICKS.load(Ordering::SeqCst), before + 1);
}

#[gpui::test]
fn code_blocks_get_the_syntect_highlighter(cx: &mut TestAppContext) {
    let (_, cx) = open(cx, "```rs\nfn main() {}\n```");
    assert!(cx.update(|_, cx| TextViewDefaults::global(cx).has_code_block_highlighter()));
}
