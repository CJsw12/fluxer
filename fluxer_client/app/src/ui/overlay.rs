//! The client's single modal slot and the container it renders in.

use std::rc::Rc;

use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    Anchor, AnyElement, App, FontWeight, Hsla, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString,
    Size, Window, anchored, div, prelude::*, px, rgb, rgba,
};

use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::ui::account_switcher::AccountSwitcherModal;
use crate::ui::access::AccessModal;
use crate::ui::channel_modal::{CreateChannelModal, EditChannelModal};
use crate::ui::emoji_picker::EmojiPicker;
use crate::ui::group_dm::{CreateGroupDmModal, LeaveGroupDmModal};
use crate::ui::guild_modal::{JoinGuildModal, LeaveGuildModal};
use crate::ui::link_warning::LinkWarningModal;
use crate::ui::media_viewer::MediaViewerModal;
use crate::ui::moderation_modal::{GuildBansModal, MemberModerationModal};
use crate::ui::profile_popout::ProfilePopout;
use crate::ui::quick_switcher::QuickSwitcher;
use crate::ui::settings::SettingsModal;

/// What covers the client layout (at most one thing at a time).
#[derive(Default)]
pub enum Overlay {
    #[default]
    None,
    QuickSwitcher(QuickSwitcher),
    EmojiPicker(EmojiPicker),
    Profile(ProfilePopout),
    Settings(SettingsModal),
    AccountSwitcher(AccountSwitcherModal),
    JoinGuild(JoinGuildModal),
    LeaveGuild(LeaveGuildModal),
    CreateGroupDm(CreateGroupDmModal),
    LeaveGroupDm(LeaveGroupDmModal),
    MediaViewer(MediaViewerModal),
    LinkWarning(LinkWarningModal),
    CreateChannel(CreateChannelModal),
    EditChannel(EditChannelModal),
    MemberModeration(MemberModerationModal),
    GuildBans(GuildBansModal),
    Access(AccessModal),
}

impl Overlay {
    pub fn is_open(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// A dimmed full-window backdrop with `content` centred near the top. A
/// press on the backdrop (not on the content) calls `on_dismiss`. Render it
/// as the last child of a `relative` root so it paints and hit-tests on top.
pub fn modal(
    content: impl IntoElement,
    theme: &Theme,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let panel = div()
        .w(px(560.))
        .max_w_full()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(content);
    div()
        .id("modal-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .pt(px(96.))
        .bg(gpui_kit::hsla(0., 0., 0., 0.55))
        .occlude()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| on_dismiss(window, cx))
        .child(panel)
        .into_any_element()
}

/// Reusable 2-pane settings modal shell (sidebar + scrollable content area
/// with a fixed top header). Clicking the dimmed backdrop or close button
/// calls `on_dismiss`.
pub fn settings_modal(
    sidebar: impl IntoElement,
    title: SharedString,
    body: impl IntoElement,
    theme: &Theme,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_dismiss = Rc::new(on_dismiss);
    let on_dismiss_bg = on_dismiss.clone();
    let on_dismiss_btn = on_dismiss;
    let dark = theme.background.l < 0.5;
    let modal_bg: Hsla = if dark { rgb(0x1a181e).into() } else { theme.background };
    let sidebar_bg: Hsla = if dark { rgb(0x2a2730).into() } else { theme.background_secondary };
    let modal_border: Hsla = if dark { rgb(0x2f2c36).into() } else { theme.border };

    let close_button = div()
        .debug_selector(|| "settings-close".to_owned())
        .child(
            Button::new("settings-close")
                .ghost()
                .small()
                .icon(AppIcon::X)
                .tooltip(l10n::tr("Close"))
                .accessibility_label(l10n::tr("Close"))
                .on_click(move |_, window, cx| on_dismiss_btn(window, cx)),
        );

    let header = div()
        .h(px(theme.sizes.settings_header_height))
        .flex_none()
        .px_6()
        .border_b_1()
        .border_color(modal_border)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(Icon::new(AppIcon::LinkSimple).with_size(px(14.)).text_color(theme.muted_foreground)),
        )
        .child(close_button);

    let content_scroll = div()
        .id("settings-content-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px_10()
        .py_6()
        .child(
            div()
                .w_full()
                .max_w(px(theme.sizes.settings_content_max_width))
                .mx_auto()
                .flex()
                .flex_col()
                .gap_6()
                .child(body),
        );

    let sidebar_pane = div()
        .flex_none()
        .min_w(px(theme.sizes.settings_sidebar_width))
        .w(gpui_kit::relative(0.24))
        .max_w(px(320.))
        .h_full()
        .bg(sidebar_bg)
        .border_r_1()
        .border_color(modal_border)
        .flex()
        .flex_col()
        .flx("app.settings-modal-layout.settings-modal-desktop-sidebar.desktop-sidebar")
        .child(sidebar);

    let content_pane = div()
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(modal_bg)
        .flex()
        .flex_col()
        .child(header)
        .child(content_scroll);

    let panel = div()
        .w_full()
        .max_w(px(theme.sizes.settings_modal_max_width))
        .h_full()
        .max_h(px(1248.))
        .rounded(px(12.))
        .border_1()
        .border_color(modal_border)
        .bg(modal_bg)
        .overflow_hidden()
        .shadow_lg()
        .flex()
        .flex_row()
        .flx("user.user-settings-modal.modal-root")
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(sidebar_pane)
        .child(content_pane);

    div()
        .id("settings-modal-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .p(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui_kit::hsla(0., 0., 0., 0.72))
        .occlude()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| on_dismiss_bg(window, cx))
        .child(panel)
        .into_any_element()
}

/// Anchored popover at window coordinates `anchor`, clamped inside the window.
/// Clicking outside calls `on_dismiss`.
pub fn popover(
    content: impl IntoElement,
    anchor: Point<Pixels>,
    theme: &Theme,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let panel = div()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(content);
    div()
        .id("popover-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| on_dismiss(window, cx))
        .child(
            anchored()
                .position(anchor)
                .anchor(Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.))
                .child(panel),
        )
        .into_any_element()
}

/// Anchored bottom-right resizable popover positioned relative to `anchor_br`
/// within 16px viewport margins (official `ResizablePane` popout contract).
#[allow(clippy::too_many_arguments)]
pub fn resizable_popover(
    id: &'static str,
    flx: &'static str,
    content: impl IntoElement,
    anchor_br: Point<Pixels>,
    width: f32,
    height: f32,
    viewport: Size<Pixels>,
    theme: &Theme,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    on_drag_move: impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static,
    on_drag_end: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let dark = theme.background.l < 0.5;
    let bg: Hsla = if dark { rgb(0x2a2730).into() } else { theme.background_secondary };
    let border: Hsla = if dark { rgba(0xc9c5d326).into() } else { theme.border };
    let vw = f32::from(viewport.width);
    let vh = f32::from(viewport.height);
    let min_top = if vh >= 700. { 44. } else { 16. };
    let eff_right = f32::from(anchor_br.x).clamp((width + 16.).min(vw - 16.), (vw - 16.).max(width + 16.));
    let eff_bottom = f32::from(anchor_br.y).clamp((min_top + height).min(vh - 16.), (vh - 16.).max(min_top + height));
    let left = (eff_right - width).clamp(16., (vw - width - 16.).max(16.));
    let top = (eff_bottom - height).clamp(min_top, (vh - height - 16.).max(min_top));
    let panel = div()
        .id(id)
        .w(px(width))
        .h(px(height))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .flx(flx)
        .child(content);
    let on_drag_end = Rc::new(on_drag_end);
    let on_drag_end_out = Rc::clone(&on_drag_end);
    div()
        .id("popover-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| on_dismiss(window, cx))
        .on_mouse_move(on_drag_move)
        .on_mouse_up(MouseButton::Left, move |ev, window, cx| on_drag_end(ev, window, cx))
        .on_mouse_up_out(MouseButton::Left, move |ev, window, cx| on_drag_end_out(ev, window, cx))
        .child(anchored().position(gpui_kit::point(px(left), px(top))).anchor(Anchor::TopLeft).child(panel))
        .into_any_element()
}
