//! Headless tests for the sidebar, voice split, and emoji picker resize
//! handles: drag clamping, keyboard steps, double-click reset, and prefs.

use fluxer_client_core::state::Snowflake;
use gpui_kit::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext,
    point, px, size,
};

use crate::backend::voice::VoiceConnectionState;
use crate::client_view::ClientView;
use crate::prefs::{
    EMOJI_PICKER_HEIGHT_DEFAULT, EMOJI_PICKER_HEIGHT_MAX, EMOJI_PICKER_HEIGHT_MIN, EMOJI_PICKER_WIDTH_DEFAULT,
    EMOJI_PICKER_WIDTH_MIN, SIDEBAR_WIDTH_DEFAULT, SIDEBAR_WIDTH_MAX, SIDEBAR_WIDTH_MIN, VOICE_CALL_HEIGHT_DEFAULT,
    VOICE_CALL_HEIGHT_MIN,
};
use crate::tests::{backend_with_fixture, draw, open_client, runtime};
use crate::ui::emoji_picker::columns_for_width;
use crate::ui::emoji_picker::sections::PickerTarget;
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);
const VOICE: Snowflake = Snowflake(13);

fn open_resize_client(cx: &mut TestAppContext) -> (tokio::runtime::Runtime, Entity<ClientView>, &mut VisualTestContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, view, cx)
}

fn resize_window(cx: &mut VisualTestContext, width: f32, height: f32) {
    cx.simulate_resize(size(px(width), px(height)));
    cx.run_until_parked();
    draw(cx);
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    draw(cx);
}

fn double_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count,
        });
    }
    draw(cx);
}

fn emoji_metrics(view: &Entity<ClientView>, cx: &mut VisualTestContext) -> (f32, f32, usize) {
    view.read_with(cx, |v, _| match &v.overlay {
        Overlay::EmojiPicker(picker) => (v.emoji_picker_width, v.emoji_picker_height, picker.columns),
        Overlay::None => panic!("emoji picker closed"),
        _ => panic!("unexpected overlay"),
    })
}

#[gpui_kit::test]
fn sidebar_resize_drag_keyboard_reset_and_prefs(cx: &mut TestAppContext) {
    let (_rt, view, cx) = open_resize_client(cx);
    resize_window(cx, 1280., 800.);

    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, SIDEBAR_WIDTH_DEFAULT as f32);
        assert_eq!(v.effective_sidebar_width(), SIDEBAR_WIDTH_DEFAULT as f32);
        assert_eq!(v.prefs.data.sidebar_width, SIDEBAR_WIDTH_DEFAULT);
    });

    // Click the handle to focus it, then drive it with the keyboard.
    let handle = cx.debug_bounds("sidebar-resize-handle").expect("sidebar handle drawn").center();
    cx.simulate_click(handle, Modifiers::default());
    draw(cx);

    cx.simulate_keystrokes("right");
    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, 332.0);
        assert_eq!(v.prefs.data.sidebar_width, 332);
    });

    cx.simulate_keystrokes("shift-right");
    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, 380.0);
        assert_eq!(v.prefs.data.sidebar_width, 380);
    });

    cx.simulate_keystrokes("home");
    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, SIDEBAR_WIDTH_MIN as f32);
        assert_eq!(v.prefs.data.sidebar_width, SIDEBAR_WIDTH_MIN);
    });

    cx.simulate_keystrokes("end");
    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, SIDEBAR_WIDTH_MAX as f32);
        assert_eq!(v.prefs.data.sidebar_width, SIDEBAR_WIDTH_MAX);
    });

    cx.simulate_keystrokes("enter");
    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, SIDEBAR_WIDTH_DEFAULT as f32);
        assert_eq!(v.prefs.data.sidebar_width, SIDEBAR_WIDTH_DEFAULT);
    });

    // Drag to the minimum width.
    let handle = cx.debug_bounds("sidebar-resize-handle").expect("sidebar handle drawn").center();
    drag(cx, handle, point(px(8.), handle.y));
    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, SIDEBAR_WIDTH_MIN as f32);
        assert_eq!(v.prefs.data.sidebar_width, SIDEBAR_WIDTH_MIN);
        assert!(v.active_resize.is_none(), "mouse up finishes the resize");
    });

    // Drag to the maximum width.
    let handle = cx.debug_bounds("sidebar-resize-handle").expect("sidebar handle drawn").center();
    drag(cx, handle, point(px(900.), handle.y));
    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, SIDEBAR_WIDTH_MAX as f32);
        assert_eq!(v.prefs.data.sidebar_width, SIDEBAR_WIDTH_MAX);
    });

    // Double-click resets to the default width.
    let handle = cx.debug_bounds("sidebar-resize-handle").expect("sidebar handle drawn").center();
    double_click(cx, handle);
    view.read_with(cx, |v, _| {
        assert_eq!(v.sidebar_width, SIDEBAR_WIDTH_DEFAULT as f32);
        assert_eq!(v.prefs.data.sidebar_width, SIDEBAR_WIDTH_DEFAULT);
    });
}

#[gpui_kit::test]
fn voice_split_resize_clamp_keyboard_reset_and_prefs(cx: &mut TestAppContext) {
    let (_rt, view, cx) = open_resize_client(cx);
    resize_window(cx, 1280., 800.);

    view.update(cx, |v, cx| {
        v.voice.state = VoiceConnectionState::Connecting;
        v.voice.guild_id = Some(GUILD);
        v.voice.channel_id = Some(VOICE);
        v.voice_stage_open = true;
        v.voice_stage_expanded = false;
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);

    view.read_with(cx, |v, _| {
        assert_eq!(v.selected_channel, Some(GENERAL));
        assert_eq!(v.voice_call_height, VOICE_CALL_HEIGHT_MIN as f32);
        assert_eq!(v.prefs.data.voice_call_height, VOICE_CALL_HEIGHT_MIN);
    });
    let max_height = view.update_in(cx, |v, window, _| v.max_voice_call_height(window));
    assert_eq!(max_height, 590.0, "800px window leaves a 590px call cap");
    assert!(
        cx.debug_bounds("voice-split-resize-handle").is_some(),
        "split voice handle drawn"
    );

    // Drag down to the maximum split height.
    let handle = cx.debug_bounds("voice-split-resize-handle").expect("voice handle drawn").center();
    drag(cx, handle, point(handle.x, px(790.)));
    view.read_with(cx, |v, _| {
        assert_eq!(v.voice_call_height, 590.0);
        assert_eq!(v.prefs.data.voice_call_height, 590);
    });

    // Drag back up to the minimum split height.
    let handle = cx.debug_bounds("voice-split-resize-handle").expect("voice handle drawn").center();
    drag(cx, handle, point(handle.x, px(1.)));
    view.read_with(cx, |v, _| {
        assert_eq!(v.voice_call_height, VOICE_CALL_HEIGHT_MIN as f32);
        assert_eq!(v.prefs.data.voice_call_height, VOICE_CALL_HEIGHT_MIN);
    });

    // Keyboard resizes by 16px and honors home/end/enter.
    cx.simulate_keystrokes("down");
    view.read_with(cx, |v, _| {
        assert_eq!(v.voice_call_height, 336.0);
        assert_eq!(v.prefs.data.voice_call_height, 336);
    });

    cx.simulate_keystrokes("up");
    view.read_with(cx, |v, _| {
        assert_eq!(v.voice_call_height, VOICE_CALL_HEIGHT_MIN as f32);
        assert_eq!(v.prefs.data.voice_call_height, VOICE_CALL_HEIGHT_MIN);
    });

    cx.simulate_keystrokes("home");
    view.read_with(cx, |v, _| {
        assert_eq!(v.voice_call_height, VOICE_CALL_HEIGHT_MIN as f32);
        assert_eq!(v.prefs.data.voice_call_height, VOICE_CALL_HEIGHT_MIN);
    });

    cx.simulate_keystrokes("end");
    view.read_with(cx, |v, _| {
        assert_eq!(v.voice_call_height, 590.0);
        assert_eq!(v.prefs.data.voice_call_height, 590);
    });

    cx.simulate_keystrokes("enter");
    view.read_with(cx, |v, _| {
        assert_eq!(v.voice_call_height, VOICE_CALL_HEIGHT_DEFAULT as f32);
        assert_eq!(v.prefs.data.voice_call_height, VOICE_CALL_HEIGHT_DEFAULT);
    });

    // Double-click resets a maximized split.
    cx.simulate_keystrokes("end");
    draw(cx);
    let handle = cx.debug_bounds("voice-split-resize-handle").expect("voice handle drawn").center();
    double_click(cx, handle);
    view.read_with(cx, |v, _| {
        assert_eq!(v.voice_call_height, VOICE_CALL_HEIGHT_DEFAULT as f32);
        assert_eq!(v.prefs.data.voice_call_height, VOICE_CALL_HEIGHT_DEFAULT);
    });
}

#[gpui_kit::test]
fn emoji_picker_resize_snaps_columns_clamps_resets_and_prefs(cx: &mut TestAppContext) {
    let (_rt, view, cx) = open_resize_client(cx);
    resize_window(cx, 1280., 800.);

    view.update_in(cx, |v, window, cx| {
        v.open_emoji_picker(PickerTarget::Composer, point(px(1010.), px(730.)), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    assert_eq!(
        emoji_metrics(&view, cx),
        (
            EMOJI_PICKER_WIDTH_DEFAULT as f32,
            EMOJI_PICKER_HEIGHT_DEFAULT as f32,
            columns_for_width(EMOJI_PICKER_WIDTH_DEFAULT as f32)
        )
    );
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.emoji_picker_width, EMOJI_PICKER_WIDTH_DEFAULT);
        assert_eq!(v.prefs.data.emoji_picker_height, EMOJI_PICKER_HEIGHT_DEFAULT);
    });

    // 592 -> 400 by dragging the left edge right.
    let handle = cx.debug_bounds("emoji-resize-left").expect("left handle drawn").center();
    drag(cx, handle, point(handle.x + px(192.), handle.y));
    assert_eq!(emoji_metrics(&view, cx), (400.0, 686.0, 7));
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.emoji_picker_width, EMOJI_PICKER_WIDTH_MIN);
        assert_eq!(v.prefs.data.emoji_picker_height, EMOJI_PICKER_HEIGHT_DEFAULT);
    });

    // 400 -> 592 by dragging the left edge left.
    let handle = cx.debug_bounds("emoji-resize-left").expect("left handle drawn").center();
    drag(cx, handle, point(handle.x - px(192.), handle.y));
    assert_eq!(emoji_metrics(&view, cx), (592.0, 686.0, 11));
    view.read_with(cx, |v, _| assert_eq!(v.prefs.data.emoji_picker_width, 592));

    // 592 -> 640.
    let handle = cx.debug_bounds("emoji-resize-left").expect("left handle drawn").center();
    drag(cx, handle, point(handle.x - px(48.), handle.y));
    assert_eq!(emoji_metrics(&view, cx), (640.0, 686.0, 12));
    view.read_with(cx, |v, _| assert_eq!(v.prefs.data.emoji_picker_width, 640));

    // 640 -> 736.
    let handle = cx.debug_bounds("emoji-resize-left").expect("left handle drawn").center();
    drag(cx, handle, point(handle.x - px(96.), handle.y));
    assert_eq!(emoji_metrics(&view, cx), (736.0, 686.0, 14));
    view.read_with(cx, |v, _| assert_eq!(v.prefs.data.emoji_picker_width, 736));

    // Drag the top edge down to clamp at the 360px minimum height.
    let handle = cx.debug_bounds("emoji-resize-top").expect("top handle drawn").center();
    drag(cx, handle, point(handle.x, handle.y + px(400.)));
    assert_eq!(emoji_metrics(&view, cx), (736.0, 360.0, 14));
    view.read_with(cx, |v, _| assert_eq!(v.prefs.data.emoji_picker_height, EMOJI_PICKER_HEIGHT_MIN));

    // Drag the top edge up to clamp at the 686px available maximum.
    let handle = cx.debug_bounds("emoji-resize-top").expect("top handle drawn").center();
    drag(cx, handle, point(handle.x, px(1.)));
    assert_eq!(emoji_metrics(&view, cx), (736.0, 686.0, 14));
    view.read_with(cx, |v, _| assert_eq!(v.prefs.data.emoji_picker_height, EMOJI_PICKER_HEIGHT_DEFAULT));

    // Keyboard resizes when the resize handle holds focus rather than moving the grid selection.
    let handle = cx.debug_bounds("emoji-resize-left").expect("left handle drawn").center();
    cx.simulate_click(handle, Modifiers::default());
    draw(cx);
    cx.simulate_keystrokes("right");
    assert_eq!(emoji_metrics(&view, cx), (688.0, 686.0, 13));
    cx.simulate_keystrokes("down");
    assert_eq!(emoji_metrics(&view, cx), (688.0, 670.0, 13));
    cx.simulate_keystrokes("enter");
    assert_eq!(emoji_metrics(&view, cx), (592.0, 686.0, 11));

    // Double-click the top-left corner resets both dimensions after resizing.
    let handle = cx.debug_bounds("emoji-resize-left").expect("left handle drawn").center();
    drag(cx, handle, point(handle.x - px(48.), handle.y));
    let corner = cx.debug_bounds("emoji-resize-top-left").expect("corner handle drawn").center();
    double_click(cx, corner);
    assert_eq!(
        emoji_metrics(&view, cx),
        (
            EMOJI_PICKER_WIDTH_DEFAULT as f32,
            EMOJI_PICKER_HEIGHT_DEFAULT as f32,
            columns_for_width(EMOJI_PICKER_WIDTH_DEFAULT as f32)
        )
    );
    view.read_with(cx, |v, _| {
        assert_eq!(v.prefs.data.emoji_picker_width, EMOJI_PICKER_WIDTH_DEFAULT);
        assert_eq!(v.prefs.data.emoji_picker_height, EMOJI_PICKER_HEIGHT_DEFAULT);
    });

    assert!(EMOJI_PICKER_HEIGHT_MIN <= EMOJI_PICKER_HEIGHT_MAX);
}
