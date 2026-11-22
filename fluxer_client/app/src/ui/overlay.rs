//! The client's single modal slot and the container it renders in.

use std::rc::Rc;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{Anchor, AnyElement, App, FontWeight, MouseButton, Pixels, Point, SharedString, Window, anchored, div, prelude::*, px};

use crate::icons::AppIcon;
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
        .border_color(theme.border)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
        .child(close_button);

    let content_scroll = div()
        .id("settings-content-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px_8()
        .py_6()
        .child(div().max_w(px(theme.sizes.settings_content_max_width)).flex().flex_col().gap_5().child(body));

    let sidebar_pane = div()
        .w(px(theme.sizes.settings_sidebar_width))
        .flex_none()
        .h_full()
        .bg(theme.background_secondary)
        .border_r_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .child(sidebar);

    let content_pane = div()
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(theme.background)
        .flex()
        .flex_col()
        .child(header)
        .child(content_scroll);

    let panel = div()
        .w_full()
        .max_w(px(theme.sizes.settings_modal_max_width))
        .h_full()
        .max_h(px(860.))
        .rounded(px(12.))
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .shadow_lg()
        .flex()
        .flex_row()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(sidebar_pane)
        .child(content_pane);

    div()
        .id("settings-modal-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .p(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui_kit::hsla(0., 0., 0., 0.65))
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
