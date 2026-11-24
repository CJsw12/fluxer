//! Integrated custom desktop titlebar (`NativeTitlebar` / `globals.css`
//! `--native-titlebar-height: 32px`). The drag region and the three 48x32
//! window-control buttons are built here (instead of
//! `gpui_component::TitleBar`, whose controls are a fixed 34px wide) so the
//! geometry matches the official app and each control carries its own
//! `data-flx` tag. On Windows the OS handles the caption hit-test through
//! `window_control_area`; on other platforms the click handlers drive the window.

use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::{
    AnyElement, ElementId, FontWeight, SharedString, WindowControlArea, div, prelude::*, px,
};

use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;

/// Official `NativeWindowControls` button width.
const CONTROL_WIDTH: f32 = 48.;
const CONTROL_ICON: f32 = 17.;

fn control_button(
    id: &'static str,
    icon: IconName,
    area: WindowControlArea,
    close: bool,
    theme: &Theme,
    flx: &'static str,
) -> AnyElement {
    let hover_bg = if close { theme.danger } else { theme.muted };
    let hover_fg = if close { theme.on_fill } else { theme.foreground };
    let d = div()
        .id(ElementId::from(id))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(CONTROL_WIDTH))
        .h_full()
        .text_color(theme.foreground)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        .window_control_area(area)
        .child(Icon::new(icon).with_size(px(CONTROL_ICON)));
    #[cfg(not(target_os = "windows"))]
    let d = d.on_click(move |_, window, _| match area {
        WindowControlArea::Min => window.minimize_window(),
        WindowControlArea::Max => window.zoom_window(),
        _ => window.remove_window(),
    });
    d.flx(flx).into_any_element()
}

/// The official wordmark shows the instance name; no instance display name is available
/// to the client yet, so the product name is used and the window title is left to the OS.
pub fn render(_title: SharedString, theme: &Theme) -> AnyElement {
    let display_title = SharedString::new_static("Fluxer");
    div()
        .w_full()
        .h(px(theme.sizes.titlebar_height))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .bg(theme.background_secondary)
        .flx("app.native-titlebar.titlebar")
        .child(
            div()
                .debug_selector(|| "app-titlebar".to_owned())
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_row()
                .items_center()
                .pl(px(12.))
                .window_control_area(WindowControlArea::Drag)
                .child(
                    div()
                        .truncate()
                        .h(px(13.))
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_color(theme.muted_foreground.opacity(0.8))
                        .flx_text("app.native-titlebar.wordmark", display_title.to_string())
                        .child(display_title),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .h_full()
                .child(control_button(
                    "titlebar-minimize",
                    IconName::WindowMinimize,
                    WindowControlArea::Min,
                    false,
                    theme,
                    "app.native-titlebar.control-button.minimize",
                ))
                .child(control_button(
                    "titlebar-maximize",
                    IconName::WindowMaximize,
                    WindowControlArea::Max,
                    false,
                    theme,
                    "app.native-titlebar.control-button.toggle-maximize",
                ))
                .child(control_button(
                    "titlebar-close",
                    IconName::WindowClose,
                    WindowControlArea::Close,
                    true,
                    theme,
                    "app.native-titlebar.control-button.close",
                )),
        )
        .into_any_element()
}
