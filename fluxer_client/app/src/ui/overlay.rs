//! The client's single modal slot and the container it renders in.

use gpui_kit::{Anchor, AnyElement, App, MouseButton, Pixels, Point, Window, anchored, div, prelude::*, px};

use crate::theme::Theme;
use crate::ui::account_switcher::AccountSwitcherModal;
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
