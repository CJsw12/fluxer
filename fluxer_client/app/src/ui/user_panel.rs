//! Bottom-left user panel (`UserArea` in the web client): avatar with a
//! status dot, display name, status text and a "Log out" button. Clicking
//! the dot cycles the own status (online, idle, dnd, invisible).

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{Context, FontWeight, MouseButton, Role, SharedString, div, prelude::*, px};

use super::l10n;
use super::member_list::{status_color, status_label};
use super::message_row::avatar;
use crate::client_view::{ClientView, Connection};
use crate::theme::Theme;

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let sizes = theme.sizes;
    let (dot, label): (_, SharedString) = match view.connection {
        Connection::Online => (status_color(view.own_status, theme), status_label(view.own_status).into()),
        Connection::Connecting => (theme.status_connecting, view.status.clone()),
        Connection::Offline => (theme.status_offline, l10n::tr("Offline").into()),
    };
    let (picture, name) = match &view.user {
        Some(user) => (
            avatar(
                user.avatar_url.clone(),
                user.default_avatar,
                user.initials.clone(),
                sizes.user_panel_avatar,
                theme,
            ),
            user.name.clone(),
        ),
        None => (
            div().size(px(sizes.user_panel_avatar)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("…"),
        ),
    };
    let self_id = view.user.as_ref().map(|u| u.id);
    let dot_size = sizes.status_dot;
    let status_aria_label = format!("{}: {}", l10n::tr("Status"), label);
    div()
        .id("user-panel")
        .role(Role::Region)
        .aria_label(l10n::tr("User controls"))
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap_2()
        .px_2()
        .py_2()
        .border_t_1()
        .border_color(theme.border)
        .child(
            div().relative().flex_none().child(picture).child(
                div()
                    .id("user-status-dot")
                    .role(Role::Button)
                    .aria_label(status_aria_label)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.cycle_status(cx);
                    }))
                    .absolute()
                    .right(px(-1.))
                    .bottom(px(-1.))
                    .size(px(dot_size))
                    .rounded_full()
                    .border_2()
                    .border_color(theme.background)
                    .bg(dot),
            ),
        )
        .child(
            div()
                .id("user-panel-profile")
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .when(self_id.is_some(), |d| d.cursor_pointer())
                .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, window, cx| {
                    if let Some(id) = self_id
                        && !ev.modifiers().shift
                    {
                        this.open_profile(id, None, ev.position(), window, cx);
                    }
                }))
                .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(name))
                .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(label)),
        )
        .when(view.accounts_enabled, |d| {
            d.child(
                div().debug_selector(|| "switch-accounts".to_owned()).child(
                    Button::new("switch-accounts")
                        .ghost()
                        .small()
                        .icon(IconName::Users)
                        .tooltip(l10n::tr("Switch accounts"))
                        .accessibility_label(l10n::tr("Switch accounts"))
                        .on_click(cx.listener(|this, _, window, cx| this.open_account_switcher(window, cx))),
                ),
            )
        })
        .child(
            Button::new("open-settings")
                .ghost()
                .small()
                .icon(IconName::Settings)
                .tooltip(l10n::tr("Settings"))
                .accessibility_label(l10n::tr("Settings"))
                .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
        )
        .child(
            Button::new("log-out")
                .ghost()
                .small()
                .label(l10n::tr("Log out"))
                .on_click(cx.listener(|this, _, _, cx| this.log_out(cx))),
        )
}
