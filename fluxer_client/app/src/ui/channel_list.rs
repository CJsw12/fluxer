//! Channel sidebar: heading, the channels of the selected place, and the
//! user panel at the bottom.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, Context, FontWeight, MouseButton, div, prelude::*, px};

use super::{access::Scope, badge, user_panel};
use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::{ChannelItem, Place};

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let header_bar = match view.place {
        Place::Home => div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_1()
            .px_3()
            .py_2()
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::BOLD).child(l10n::tr("Direct messages")))
            .child(
                div().debug_selector(|| "create-group-dm-btn".to_owned()).child(
                    Button::new("create-group-dm-btn")
                        .ghost()
                        .small()
                        .icon(IconName::Plus)
                        .tooltip(l10n::tr("Create group DM"))
                        .on_click(cx.listener(|this, _, window, cx| this.open_create_group_dm(window, cx))),
                ),
            )
            .into_any_element(),
        Place::Guild(id) => {
            let name = view
                .guilds
                .iter()
                .find(|g| g.id == id)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            let can_leave = !view.current_guild_is_owner();
            let can_manage_roles = view.backend.state().can_manage_guild_roles(id);
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_1()
                .px_3()
                .py_2()
                .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::BOLD).child(name))
                .when(view.can_manage_guild_channels, |d| {
                    d.child(
                        div().debug_selector(|| "create-channel-btn".to_owned()).child(
                            Button::new("create-channel-btn")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Plus)
                                .tooltip(l10n::tr("Create channel"))
                                .on_click(cx.listener(|this, _, window, cx| this.open_create_channel(None, window, cx))),
                        ),
                    )
                })
                .when(view.can_ban_members, |d| {
                    d.child(
                        div().debug_selector(|| "guild-bans-btn".to_owned()).child(
                            Button::new("guild-bans-btn")
                                .ghost()
                                .xsmall()
                                .label(l10n::tr("Bans"))
                                .on_click(cx.listener(|this, _, window, cx| this.open_guild_bans(window, cx))),
                        ),
                    )
                })
                .when(can_manage_roles, |d| {
                    d.child(div().debug_selector(|| "guild-roles-btn".to_owned()).child(
                        Button::new("guild-roles-btn").ghost().xsmall().label(l10n::tr("Roles"))
                            .on_click(cx.listener(move |this, _, window, cx| this.open_access(Scope::Guild(id), window, cx))),
                    ))
                })
                .when(can_leave, |d| {
                    d.child(
                        div().debug_selector(|| "leave-guild-btn".to_owned()).child(
                            Button::new("leave-guild-btn")
                                .ghost()
                                .small()
                                .label(l10n::tr("Leave community"))
                                .on_click(cx.listener(move |this, _, window, cx| this.open_leave_guild(id, window, cx))),
                        ),
                    )
                })
                .into_any_element()
        }
    };
    let mut list = div()
        .id("channel-list")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(header_bar);
    if view.place == Place::Home {
        let active = view.selected_channel.is_none();
        list = list.child(
            div()
                .id("home-friends")
                .debug_selector(|| "home-friends".to_owned())
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .py_2()
                .cursor_pointer()
                .when(active, |d| d.bg(theme.accent))
                .hover(|s| s.bg(theme.muted))
                .on_click(cx.listener(|this, _, _, cx| this.open_friends(cx)))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(IconName::Users).xsmall())
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Friends"))),
                )
                .when(view.pending_incoming > 0, |d| d.child(badge(view.pending_incoming, theme))),
        );
    }
    for c in &view.channels {
        list = list.child(render_channel(view, c, theme, cx));
    }
    div()
        .flex()
        .flex_col()
        .w(px(theme.sizes.channel_list_width))
        .h_full()
        .flex_none()
        .border_r_1()
        .border_color(theme.border)
        .child(list)
        .child(user_panel::render(view, theme, cx))
}

fn render_channel(view: &ClientView, c: &ChannelItem, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (muted_fg, fg) = (theme.muted_foreground, theme.foreground);
    let permissions = match view.place {
        Place::Guild(guild) if c.can_manage_permissions => {
            let channel = c.id;
            Some(div().id(("channel-permissions-wrap", channel.0))
                .debug_selector(move || format!("channel-permissions-{}", channel.0))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(Button::new(("channel-permissions", channel.0)).ghost().xsmall()
                    .label(l10n::tr("Permissions"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_access(Scope::Channel { guild, channel }, window, cx);
                    }))))
        }
        _ => None,
    };
    if c.is_category {
        let cat_id = c.id;
        let create_sel = format!("create-in-category-{}", cat_id.0);
        let edit_sel = format!("edit-channel-{}", cat_id.0);
        return div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_3()
            .pt_3()
            .text_xs()
            .text_color(muted_fg)
            .font_weight(FontWeight::MEDIUM)
            .child(div().flex_1().min_w_0().truncate().child(c.name.clone()))
            .when(c.can_manage, |d| {
                d.child(
                    div().debug_selector(move || create_sel.clone()).child(
                        Button::new(("create-in-category", cat_id.0))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip(l10n::tr("Create channel"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_create_channel(Some(cat_id), window, cx);
                            })),
                    ),
                )
                .child(
                    div().debug_selector(move || edit_sel.clone()).child(
                        Button::new(("edit-channel", cat_id.0))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Settings)
                            .tooltip(l10n::tr("Category settings"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_edit_channel(cat_id, window, cx);
                            })),
                    ),
                )
            })
            .children(permissions)
            .into_any_element();
    }
    let selected = view.selected_channel == Some(c.id);
    let highlight_unread = c.unread.unread && !c.muted;
    let can_edit_row = view.place != Place::Home && c.can_manage;
    let mut row = div()
        .id(("channel", c.id.0))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_3()
        .py_1()
        .when(c.nested, |d| d.pl_5())
        .text_color(if (highlight_unread || selected) && c.has_messages { fg } else { muted_fg })
        .when(highlight_unread, |d| d.font_weight(FontWeight::BOLD))
        .when(selected, |d| d.bg(theme.accent))
        .child(div().w(px(18.)).flex_none().text_color(muted_fg).child(c.icon.glyph()))
        .child(div().flex_1().truncate().child(c.name.clone()));
    if c.muted {
        row = row.child(Icon::new(IconName::BellOff).xsmall());
    }
    if c.unread.mention_count > 0 {
        row = row.child(badge(c.unread.mention_count, theme));
    } else if highlight_unread {
        row = row.child(div().text_color(fg).child("•"));
    }
    if can_edit_row {
        let ch_id = c.id;
        let edit_sel = format!("edit-channel-{}", ch_id.0);
        row = row.child(
            div()
                .id(("edit-channel-wrap", ch_id.0))
                .debug_selector(move || edit_sel.clone())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    Button::new(("edit-channel", ch_id.0))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Settings)
                        .tooltip(l10n::tr("Channel settings"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.open_edit_channel(ch_id, window, cx);
                        })),
                ),
        );
    }
    if c.has_messages {
        let id = c.id;
        row = row.on_click(cx.listener(move |this, _, _, cx| this.select_channel(id, cx)));
    }
    row.children(permissions).into_any_element()
}
