//! Hover tooltips (`Tooltip.tsx` / `Tooltip.module.css` + `GuildListItemTooltip.tsx`
//! + `KeybindHint.tsx`):
//! - Right-placed for guild rail buttons (`TOOLTIP_LEFT = 80`), with left-pointing arrow.
//! - Below-placed for channel header buttons (`below`), with up-pointing arrow.
//! - Above-placed for bottom user-area control buttons (`above`), with down-pointing arrow.
//! Drawn through `deferred` so scroll/overflow clips do not cut them off.

use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, FontWeight, Hsla, PathBuilder, Pixels, SharedString, Window, canvas, deferred, div, point, prelude::*,
    px, rgb, rgba,
};

use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;

const SURFACE: u32 = 0x2a2730;
/// Bounding box of the official 10px square pointer rotated 45deg (10 * sqrt 2).
const POINTER_BOX: f32 = 14.14;
/// `--tooltip-border-color`: measured `rgba(201, 197, 211, 0.15)`.
const BORDER_RGBA: u32 = 0xc9c5d326;

/// Rail slot width (72) plus the official `8px` gap, relative to a full-width slot.
pub(crate) const TOOLTIP_LEFT: f32 = 80.;

#[derive(Clone, Copy)]
enum PointerSide {
    Left,
    Up { right_inset: Option<f32> },
    Down,
}

fn pointer(side: PointerSide) -> impl IntoElement {
    let fill: Hsla = rgb(SURFACE).into();
    let border: Hsla = rgba(BORDER_RGBA).into();
    let paint_tri = move |side: PointerSide| {
        canvas(
            |_, _, _| (),
            move |bounds, (), window: &mut Window, _| {
                let (l, t, r, b) = (bounds.left(), bounds.top(), bounds.right(), bounds.bottom());
                let mut draw = |pts: [(Pixels, Pixels); 3], color: Hsla| {
                    let mut pb = PathBuilder::fill();
                    pb.move_to(point(pts[0].0, pts[0].1));
                    pb.line_to(point(pts[1].0, pts[1].1));
                    pb.line_to(point(pts[2].0, pts[2].1));
                    pb.close();
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, color);
                    }
                };
                // Official pointer: a 10px square rotated 45deg (14.14 bbox) centred on the
                // tooltip's padding edge; only the half outside the body is visible.
                let cx = l + (r - l) / 2.;
                let cy = t + (b - t) / 2.;
                match side {
                    PointerSide::Left => {
                        draw([(l, cy), (cx + px(1.), cy - px(7.)), (cx + px(1.), cy + px(7.))], border);
                        draw([(l + px(1.4), cy), (cx + px(1.), cy - px(6.4)), (cx + px(1.), cy + px(6.4))], fill);
                    }
                    PointerSide::Up { .. } => {
                        draw([(cx, t), (cx + px(7.), cy + px(1.)), (cx - px(7.), cy + px(1.))], border);
                        draw([(cx, t + px(1.4)), (cx + px(6.4), cy + px(1.)), (cx - px(6.4), cy + px(1.))], fill);
                    }
                    PointerSide::Down => {
                        draw([(cx - px(7.), cy - px(1.)), (cx + px(7.), cy - px(1.)), (cx, b)], border);
                        draw([(cx - px(6.4), cy - px(1.)), (cx + px(6.4), cy - px(1.)), (cx, b - px(1.4))], fill);
                    }
                }
            },
        )
    };
    let tri_box = div()
        .flex_none()
        .size(px(POINTER_BOX))
        .flx("ui.tooltip.tooltip.tooltip-container.tooltip-pointer")
        .child(paint_tri(side).size_full());
    match side {
        PointerSide::Left => div()
            .absolute()
            .left(px(-POINTER_BOX / 2.))
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .child(tri_box)
            .into_any_element(),
        PointerSide::Up { right_inset: None } => div()
            .absolute()
            .top(px(-POINTER_BOX / 2.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(tri_box)
            .into_any_element(),
        PointerSide::Up { right_inset: Some(right) } => div()
            .absolute()
            .top(px(-POINTER_BOX / 2.))
            .right(px(right))
            .child(tri_box)
            .into_any_element(),
        PointerSide::Down => div()
            .absolute()
            .bottom(px(-POINTER_BOX / 2.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(tri_box)
            .into_any_element(),
    }
}

fn shell(content: impl IntoElement, centered: bool, side: PointerSide) -> impl IntoElement {
    shell_max(content, centered, side, 190.)
}

/// `max_w` is the official `max-width: 11.875rem` (190); the guild tooltip widens it.
fn shell_max(content: impl IntoElement, centered: bool, side: PointerSide, max_w: f32) -> impl IntoElement {
    div()
        .flx("ui.tooltip.tooltip.tooltip")
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .when(centered, |d| d.items_center())
                .rounded(px(8.))
                .bg(rgb(SURFACE))
                .border_1()
                .border_color(rgba(BORDER_RGBA))
                .shadow_lg()
                .max_w(px(max_w))
                .child(pointer(side))
                .child(
                    div()
                        .flx("ui.tooltip.tooltip.tooltip-container.tooltip-content")
                        .child(content),
                )
                .flx("ui.tooltip.tooltip.tooltip-container.tooltip"),
        )
}

fn label(text: impl Into<SharedString>, theme: &Theme, size: f32, line_h: f32, bold: bool) -> impl IntoElement {
    div()
        .max_w_full()
        .text_center()
        .text_size(px(size))
        .line_height(px(line_h))
        .font_weight(if bold { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
        .text_color(theme.foreground)
        .child(text.into())
}

fn keybind_content(
    text: impl Into<SharedString>,
    keys: &[&'static str],
    font_size: f32,
    line_h: f32,
    pad_x: f32,
    pad_y: f32,
    theme: &Theme,
) -> impl IntoElement {
    let chip = |t: &'static str| {
        div()
            .h(px(20.))
            .min_w(px(20.))
            .px(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .bg(theme.background_secondary)
            .text_size(px(11.))
            .line_height(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground)
            .flx_text("ui.keybind-hint.keybind-hint.key-symbol", t)
            .child(t)
    };
    div()
        .px(px(pad_x))
        .py(px(pad_y))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.))
        .flx("ui.keybind-hint.keybind-hint.tooltip-with-keybind.tooltip-content")
        .child(
            div()
                .flx("ui.keybind-hint.keybind-hint.tooltip-with-keybind.label")
                .child(label(text, theme, font_size, line_h, false)),
        )
        .when(!keys.is_empty(), |d| {
            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(2.))
                .flx("ui.keybind-hint.keybind-hint.keybind-hint");
            for &k in keys {
                row = row.child(chip(k));
            }
            d.child(row)
        })
}

/// Position the tooltip beside a rail slot (`left` is relative to the slot, which must be `relative`).
/// The measured `flx` sits on a shrink-wrapped flex item so its rect equals the tooltip box.
fn place_side(tooltip: impl IntoElement, left: f32, flx: &'static str) -> AnyElement {
    deferred(
        div()
            .absolute()
            .top(px(0.))
            .left(px(left))
            .h_full()
            .flex()
            .items_center()
            .child(div().flx(flx).child(tooltip)),
    )
    .with_priority(10)
    .into_any_element()
}

/// One-line rail tooltip (`Direct messages`, `Favorites`, `Explore communities`): height 42.
pub(crate) fn text(text: impl Into<SharedString>, theme: &Theme, left: f32, flx: &'static str) -> AnyElement {
    place_side(
        shell(
            div().px(px(14.)).py(px(10.)).child(label(text, theme, 15., 20., false)),
            false,
            PointerSide::Left,
        ),
        left,
        flx,
    )
}

/// Guild tooltip: bold name row, plus the `online / members` row when counts are known.
pub(crate) fn guild(name: impl Into<SharedString>, counts: Option<(u32, u32)>, theme: &Theme, flx: &'static str) -> AnyElement {
    let stat = |dot: Hsla, text: String, count_flx: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(5.))
            .flx("app.sidebar-nav.guild-list-item-tooltip.guild-tooltip-stat")
            .child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(dot)
                    .flx("app.sidebar-nav.guild-list-item-tooltip.guild-tooltip-stat-dot"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .text_color(theme.muted_foreground)
                    .flx_text(count_flx, text.clone())
                    .child(text),
            )
    };
    let body = div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(12.))
        .px(px(14.))
        .py(px(10.))
        .min_w(px(248.))
        .flx("app.sidebar-nav.guild-list-item-tooltip.guild-tooltip-container")
        .child(
            div()
                .flx("app.sidebar-nav.guild-list-item-tooltip.guild-tooltip-header")
                .child(
                    div()
                        .flx("app.sidebar-nav.guild-list-item-tooltip.guild-tooltip-name")
                        .child(label(name, theme, 15., 22.4, true)),
                ),
        )
        .children(counts.map(|(online, members)| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .flx("app.sidebar-nav.guild-list-item-tooltip.guild-tooltip-stats")
                .child(stat(
                    theme.status_online,
                    format!("{online} online"),
                    "app.sidebar-nav.guild-list-item-tooltip.render-online-count.guild-tooltip-stat-text",
                ))
                .child(stat(
                    theme.status_offline,
                    format!("{members} {}", if members == 1 { "member" } else { "members" }),
                    "app.sidebar-nav.guild-list-item-tooltip.render-member-count.guild-tooltip-stat-text",
                ))
        }));
    place_side(shell_max(body, false, PointerSide::Left, 400.), TOOLTIP_LEFT, flx)
}

/// `Create or join a community` tooltip with the `CTRL SHIFT N` key chips.
pub(crate) fn create_or_join(label_text: impl Into<SharedString>, theme: &Theme, left: f32, flx: &'static str) -> AnyElement {
    let body = keybind_content(label_text, &["CTRL", "SHIFT", "N"], 15., 20., 14., 10., theme);
    place_side(shell(body, true, PointerSide::Left), left, flx)
}

/// Header button tooltip placed below a `32x32` button with an `8px` gap (`top: 40`).
pub(crate) fn below(
    label_text: impl Into<SharedString>,
    keys: &[&'static str],
    align_right: bool,
    theme: &Theme,
) -> AnyElement {
    let tip = shell(
        keybind_content(label_text, keys, 13., 18., 10., 7., theme),
        true,
        PointerSide::Up {
            right_inset: align_right.then_some(19.),
        },
    );
    let container = if align_right {
        div().absolute().top(px(40.)).right(px(-10.)).flex().justify_end().child(tip)
    } else {
        div().absolute().top(px(40.)).left(px(-104.)).w(px(240.)).flex().justify_center().child(tip)
    };
    deferred(container).with_priority(10).into_any_element()
}

/// User-area control tooltip placed above a `32x32` button with an `8px` gap (`bottom: 40`).
pub(crate) fn above(label_text: impl Into<SharedString>, keys: &[&'static str], theme: &Theme) -> AnyElement {
    let tip = shell(
        keybind_content(label_text, keys, 13., 18., 10., 7., theme),
        true,
        PointerSide::Down,
    );
    deferred(
        div()
            .absolute()
            .bottom(px(40.))
            .left(px(-104.))
            .w(px(240.))
            .flex()
            .justify_center()
            .child(tip),
    )
    .with_priority(10)
    .into_any_element()
}

/// Official `TooltipWithKeybind` at `position="top"` for a `32x32` button: box centred above it with
/// the `8px` offset (`bottom: 40`). The shell already carries the official `ui.tooltip.tooltip.*` flx names.
pub(crate) fn top(label_text: impl Into<SharedString>, keys: &[&'static str], theme: &Theme) -> AnyElement {
    let tip = shell(keybind_content(label_text, keys, 13., 18., 10., 7., theme), true, PointerSide::Down);
    deferred(
        div()
            .absolute()
            .bottom(px(40.))
            .left(px(-104.))
            .w(px(240.))
            .flex()
            .justify_center()
            .child(tip),
    )
    .with_priority(10)
    .into_any_element()
}

/// Green voice-activity badge (`guildVoiceBadge`): 20px circle with a 3px rail-coloured ring.
pub(crate) fn voice_badge(theme: &Theme) -> AnyElement {
    div()
        .absolute()
        .right(px(-7.))
        .top(px(-7.))
        .size(px(26.))
        .rounded_full()
        .bg(theme.background_secondary)
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(20.))
                .rounded_full()
                .bg(theme.status_online)
                .flex()
                .items_center()
                .justify_center()
                .flx("app.sidebar-nav.voice-badge.div")
                .child(
                    div()
                        .size(px(12.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flx("app.sidebar-nav.voice-badge.get-voice-badge-icon.speaker-high-icon")
                        .child(Icon::new(AppIcon::SpeakerHigh).with_size(px(12.)).text_color(theme.on_fill)),
                ),
        )
        .into_any_element()
}
