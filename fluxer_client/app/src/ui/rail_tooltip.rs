//! Guild rail hover tooltip (`Tooltip.tsx` / `Tooltip.module.css` + `GuildListItemTooltip.tsx`):
//! right-placed, `8px` from the rail, vertically centred on the button, with a left-pointing
//! arrow. Drawn through `deferred` so the rail's scroll clip does not cut it off.

use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, FontWeight, Hsla, PathBuilder, Pixels, SharedString, Window, canvas, deferred, div, point, prelude::*,
    px, rgb,
};

use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;

const SURFACE: u32 = 0x2a2730;
const ARROW_W: f32 = 7.;
/// `--tooltip-border-color` (`--background-modifier-accent`, ~8% white over the surface).
const BORDER: u32 = 0x3d3a44;

/// Rail slot width (72) plus the official `8px` gap, relative to a full-width slot.
pub(crate) const TOOLTIP_LEFT: f32 = 80.;

fn arrow() -> impl IntoElement {
    let color: Hsla = rgb(SURFACE).into();
    canvas(
        |_, _, _| (),
        move |bounds, (), window: &mut Window, _| {
            let (l, t, r, bt) = (bounds.left(), bounds.top(), bounds.right(), bounds.bottom());
            let mid: Pixels = t + (bt - t) / 2.;
            let border: Hsla = rgb(BORDER).into();
            // Outer triangle in the border colour, inner one inset by 1px in the surface colour.
            let mut tri = |inset: f32, fill: Hsla| {
                let mut b = PathBuilder::fill();
                b.move_to(point(l + px(inset * 1.4), mid));
                b.line_to(point(r + px(1.), t + px(inset * 0.6)));
                b.line_to(point(r + px(1.), bt - px(inset * 0.6)));
                b.close();
                if let Ok(path) = b.build() {
                    window.paint_path(path, fill);
                }
            };
            tri(0., border);
            tri(1., color);
        },
    )
    .absolute()
    .left(px(-ARROW_W - 1.))
    .top(px(0.))
    .w(px(ARROW_W))
    .h_full()
}

fn shell(content: impl IntoElement, centered: bool) -> impl IntoElement {
    div()
        .relative()
        .flex()
        .flex_col()
        .when(centered, |d| d.items_center())
        .rounded(px(8.))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .shadow_lg()
        .whitespace_nowrap()
        .child(arrow())
        .child(content)
        .flx("ui.tooltip.tooltip.tooltip-container.tooltip")
}

fn label(text: impl Into<SharedString>, theme: &Theme, size: f32, bold: bool) -> impl IntoElement {
    div()
        .text_size(px(size))
        .line_height(px(size + 5.))
        .font_weight(if bold { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
        .text_color(theme.foreground)
        .child(text.into())
}

/// Position the tooltip beside a rail slot (`left` is relative to the slot, which must be `relative`).
fn place(tooltip: impl IntoElement, left: f32) -> AnyElement {
    deferred(
        div()
            .absolute()
            .top(px(0.))
            .left(px(left))
            .h_full()
            .flex()
            .items_center()
            .child(tooltip),
    )
    .with_priority(10)
    .into_any_element()
}

/// One-line tooltip (`Direct messages`, `Favorites`, `Explore communities`): height 42.
pub(crate) fn text(text: impl Into<SharedString>, theme: &Theme, left: f32) -> AnyElement {
    place(shell(div().px(px(20.)).py(px(12.)).child(label(text, theme, 13., false)), false), left)
}

/// Guild tooltip: bold name row, plus the `online / members` row when counts are known.
pub(crate) fn guild(name: impl Into<SharedString>, counts: Option<(u32, u32)>, theme: &Theme) -> AnyElement {
    let stat = |dot: Hsla, text: String| {
        div()
            .flex()
            .items_center()
            .gap(px(5.))
            .child(div().size(px(8.)).rounded_full().bg(dot))
            .child(div().text_size(px(13.)).line_height(px(16.)).text_color(theme.muted_foreground).child(text))
    };
    let body = div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(6.))
        .px(px(14.))
        .py(px(12.))
        .min_w(px(220.))
        .child(label(name, theme, 16., true))
        .children(counts.map(|(online, members)| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(stat(theme.status_online, format!("{online} online")))
                .child(stat(theme.muted, format!("{members} {}", if members == 1 { "member" } else { "members" })))
        }));
    place(shell(body, false), TOOLTIP_LEFT)
}

/// `Create or join a community` tooltip with the `CTRL SHIFT N` key chips.
pub(crate) fn create_or_join(label_text: impl Into<SharedString>, theme: &Theme, left: f32) -> AnyElement {
    let chip = |t: &'static str| {
        div()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .bg(theme.muted)
            .text_size(px(11.))
            .line_height(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground)
            .child(t)
    };
    let body = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.))
        .px(px(20.))
        .py(px(14.))
        .child(label(label_text, theme, 13., false))
        .child(div().flex().items_center().gap(px(4.)).child(chip("CTRL")).child(chip("SHIFT")).child(chip("N")));
    place(shell(body, true), left)
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
                .child(Icon::new(AppIcon::SpeakerHigh).with_size(px(12.)).text_color(theme.on_fill)),
        )
        .flx("app.sidebar-nav.guild-list-item-presentation.guild-list-item-badges.guild-badge")
        .into_any_element()
}
