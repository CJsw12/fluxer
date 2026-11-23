//! Integrated custom desktop titlebar (`NativeTitlebar` / `globals.css`
//! `--native-titlebar-height: 32px`), built on `gpui_kit::component::TitleBar`
//! so native Windows/macOS/Linux drag regions and window controls work out of
//! the box while matching the active Fluxer theme surface.

use gpui_kit::component::TitleBar;
use gpui_kit::{AnyElement, FontWeight, SharedString, div, prelude::*, px};

use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;

pub fn render(title: SharedString, theme: &Theme) -> AnyElement {
    let display_title = if title.is_empty() {
        SharedString::new_static("Fluxer")
    } else {
        title
    };
    let bar = TitleBar::new()
        .h(px(theme.sizes.titlebar_height))
        .bg(theme.background_tertiary)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .debug_selector(|| "app-titlebar".to_owned())
                .flex()
                .flex_row()
                .items_center()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(display_title),
                ),
        );
    if !crate::parity_probe::enabled() {
        return bar.into_any_element();
    }
    div()
        .w_full()
        .flex_none()
        .flx("app.native-titlebar.titlebar")
        .child(crate::parity_probe::titlebar_controls_marker())
        .child(bar)
        .into_any_element()
}
