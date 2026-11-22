//! Channel sidebar: heading, the channels of the selected place, and the
//! user panel at the bottom.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, Context, FontWeight, MouseButton, Role, SharedString, div, prelude::*, px};

use super::{access::Scope, badge, user_panel, voice_panel};
use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::{ChannelIcon, ChannelItem, Place};

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
            .border_b_1()
            .border_color(theme.border)
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::BOLD).child(l10n::tr("Direct messages")))
            .child(
                div().debug_selector(|| "create-group-dm-btn".to_owned()).child(
                    Button::new("create-group-dm-btn")
                        .ghost()
                        .small()
                        .icon(IconName::Plus)
                        .tooltip(l10n::tr("Create group DM"))
                        .accessibility_label(l10n::tr("Create group DM"))
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
                .border_b_1()
                .border_color(theme.border)
                .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::BOLD).child(name))
                .when(view.can_manage_guild_channels, |d| {
                    d.child(
                        div().debug_selector(|| "create-channel-btn".to_owned()).child(
                            Button::new("create-channel-btn")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Plus)
                                .tooltip(l10n::tr("Create channel"))
                                .accessibility_label(l10n::tr("Create channel"))
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
                                .icon(IconName::CircleX)
                                .tooltip(l10n::tr("Bans"))
                                .accessibility_label(l10n::tr("Bans"))
                                .on_click(cx.listener(|this, _, window, cx| this.open_guild_bans(window, cx))),
                        ),
                    )
                })
                .when(can_manage_roles, |d| {
                    d.child(
                        div().debug_selector(|| "guild-roles-btn".to_owned()).child(
                            Button::new("guild-roles-btn")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Settings2)
                                .tooltip(l10n::tr("Roles"))
                                .accessibility_label(l10n::tr("Roles"))
                                .on_click(cx.listener(move |this, _, window, cx| this.open_access(Scope::Guild(id), window, cx))),
                        ),
                    )
                })
                .when(can_leave, |d| {
                    d.child(
                        div().debug_selector(|| "leave-guild-btn".to_owned()).child(
                            Button::new("leave-guild-btn")
                                .ghost()
                                .xsmall()
                                .icon(IconName::ExternalLink)
                                .tooltip(l10n::tr("Leave community"))
                                .accessibility_label(l10n::tr("Leave community"))
                                .on_click(cx.listener(move |this, _, window, cx| this.open_leave_guild(id, window, cx))),
                        ),
                    )
                })
                .into_any_element()
        }
    };
    let mut list = div()
        .id("channel-list")
        .role(Role::Navigation)
        .aria_label(l10n::tr("Channels"))
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
                .role(Role::Button)
                .aria_label(l10n::tr("Friends"))
                .aria_selected(active)
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
        .children(voice_panel::render(view, theme, cx))
        .child(user_panel::render(view, theme, cx))
}

fn render_channel(view: &ClientView, c: &ChannelItem, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (muted_fg, fg) = (theme.muted_foreground, theme.foreground);
    let row_group: SharedString = format!("channel-row-{}", c.id.0).into();
    let permissions = match view.place {
        Place::Guild(guild) if c.can_manage_permissions => {
            let channel = c.id;
            Some(div().id(("channel-permissions-wrap", channel.0))
                .debug_selector(move || format!("channel-permissions-{}", channel.0))
                .opacity(0.)
                .group_hover(row_group.clone(), |s| s.opacity(1.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(Button::new(("channel-permissions", channel.0)).ghost().xsmall()
                    .icon(IconName::Settings2)
                    .tooltip(l10n::tr("Permissions"))
                    .accessibility_label(l10n::tr("Permissions"))
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
            .group(row_group.clone())
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
                    div()
                        .debug_selector(move || create_sel.clone())
                        .opacity(0.)
                        .group_hover(row_group.clone(), |s| s.opacity(1.))
                        .child(
                            Button::new(("create-in-category", cat_id.0))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Plus)
                                .tooltip(l10n::tr("Create channel"))
                                .accessibility_label(l10n::tr("Create channel"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_create_channel(Some(cat_id), window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .debug_selector(move || edit_sel.clone())
                        .opacity(0.)
                        .group_hover(row_group.clone(), |s| s.opacity(1.))
                        .child(
                            Button::new(("edit-channel", cat_id.0))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Settings)
                                .tooltip(l10n::tr("Category settings"))
                                .accessibility_label(l10n::tr("Category settings"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_edit_channel(cat_id, window, cx);
                                })),
                        ),
                )
            })
            .children(permissions)
            .into_any_element();
    }
    let is_voice = c.icon == ChannelIcon::Voice;
    let voice_active = is_voice && view.voice.channel_id == Some(c.id);
    let selected = view.selected_channel == Some(c.id) || voice_active;
    let highlight_unread = c.unread.unread && !c.muted;
    let can_edit_row = view.place != Place::Home && c.can_manage;
    let channel_aria_label = if is_voice {
        format!("{} ({})", c.name, l10n::tr("Voice channel"))
    } else {
        c.name.to_string()
    };
    let mut row = div()
        .id(("channel", c.id.0))
        .group(row_group.clone())
        .role(Role::Button)
        .aria_label(channel_aria_label)
        .aria_selected(selected)
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .mx_2()
        .px_2()
        .py_1()
        .rounded(px(6.))
        .when(c.nested, |d| d.pl_5())
        .text_color(if (highlight_unread || selected) && (c.has_messages || is_voice) { fg } else { muted_fg })
        .when(highlight_unread || voice_active, |d| d.font_weight(FontWeight::BOLD))
        .when(selected, |d| d.bg(theme.accent))
        .hover(|s| s.bg(theme.muted))
        .when(is_voice, |d| d.cursor_pointer())
        .child(div().w(px(18.)).flex_none().text_color(muted_fg).child(c.icon.glyph()))
        .child(div().flex_1().min_w_0().truncate().child(c.name.clone()));
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
                .opacity(0.)
                .group_hover(row_group, |s| s.opacity(1.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    Button::new(("edit-channel", ch_id.0))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Settings)
                        .tooltip(l10n::tr("Channel settings"))
                        .accessibility_label(l10n::tr("Channel settings"))
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
    } else if is_voice {
        let id = c.id;
        row = row.on_click(cx.listener(move |this, _, _, cx| this.join_voice_channel(id, cx)));
    }
    let row = row.children(permissions);
    if !is_voice {
        return row.into_any_element();
    }
    let members = view.voice.members_in_channel(c.id);
    if members.is_empty() {
        return row.into_any_element();
    }
    let mut group = div().flex().flex_col().child(row);
    for member in members {
        let display = view.voice_member_name(member.guild_id, member.user_id);
        let suffix = match (member.self_deaf || member.deaf, member.self_mute || member.mute) {
            (true, _) => " (deafened)",
            (false, true) => " (muted)",
            (false, false) => "",
        };
        group = group.child(
            div()
                .id(("voice-member", member.user_id.0))
                .pl_8()
                .pr_3()
                .py_0p5()
                .text_xs()
                .text_color(muted_fg)
                .truncate()
                .child(format!("{display}{suffix}")),
        );
    }
    group.into_any_element()
}
