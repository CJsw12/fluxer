//! Channel sidebar: heading, the channels of the selected place, and the
//! user panel at the bottom.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, Context, FontWeight, MouseButton, Role, SharedString, div, prelude::*, px};

use super::avatar::{AvatarSpec, render_avatar};
use super::{access::Scope, badge};
use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::{ChannelIcon, ChannelItem, Place};

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let header_bar = match view.place {
        Place::Home => div()
            .h(px(theme.sizes.header_height))
            .flex_none()
            .px_2()
            .border_b_1()
            .border_color(theme.border)
            .flex()
            .items_center()
            .child(
                div()
                    .id("dm-quick-switcher-btn")
                    .role(Role::Button)
                    .aria_label(l10n::tr("Quick switcher"))
                    .debug_selector(|| "dm-quick-switcher-btn".to_owned())
                    .w_full()
                    .h(px(34.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2p5()
                    .rounded(px(6.))
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted))
                    .on_click(cx.listener(|this, _, window, cx| this.open_quick_switcher(window, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Quick switcher")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.))
                            .bg(theme.background_secondary)
                            .text_size(px(10.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child("CTRL K"),
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
            let menu_open = view.guild_header_popout_open;
            let header_group: SharedString = "guild-header-group".into();
            let header_row = div()
                .group(header_group.clone())
                .relative()
                .h(px(theme.sizes.header_height))
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_1()
                .px_4()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.background_secondary)
                .hover(|s| s.bg(theme.muted))
                .id("guild-header")
                .role(Role::Button)
                .aria_label(l10n::tr("Community menu"))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.toggle_guild_header_popout(cx)))
                .flx("app.guild-header.header-wrapper")
                .flx("app.guild-header-shell.button.context-menu-wrapper")
                .child(
                    div()
                        .id("guild-header-title")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(theme.foreground)
                        .font_weight(FontWeight::SEMIBOLD)
                        .flx_text("app.guild-header.guild-name", name.to_string())
                        .child(name),
                )
                .child(
                    div()
                        .id("guild-header-actions")
                        .absolute()
                        .right(px(40.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_0p5()
                        .opacity(0.)
                        .group_hover(header_group, |s| s.opacity(1.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .when(view.can_manage_guild_channels, |d| {
                            d.child(
                                div().debug_selector(|| "create-channel-btn".to_owned()).child(
                                    Button::new("create-channel-btn")
                                        .ghost()
                                        .xsmall()
                                        .icon(AppIcon::Plus)
                                        .tooltip(l10n::tr("Create channel"))
                                        .accessibility_label(l10n::tr("Create channel"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_create_channel(None, window, cx)
                                        })),
                                ),
                            )
                        })
                        .when(view.can_ban_members, |d| {
                            d.child(
                                div().debug_selector(|| "guild-bans-btn".to_owned()).child(
                                    Button::new("guild-bans-btn")
                                        .ghost()
                                        .xsmall()
                                        .icon(AppIcon::Prohibit)
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
                                        .icon(AppIcon::SlidersHorizontal)
                                        .tooltip(l10n::tr("Roles"))
                                        .accessibility_label(l10n::tr("Roles"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_access(Scope::Guild(id), window, cx)
                                        })),
                                ),
                            )
                        })
                        .when(can_leave, |d| {
                            d.child(
                                div().debug_selector(|| "leave-guild-btn".to_owned()).child(
                                    Button::new("leave-guild-btn")
                                        .ghost()
                                        .xsmall()
                                        .icon(AppIcon::SignOut)
                                        .tooltip(l10n::tr("Leave community"))
                                        .accessibility_label(l10n::tr("Leave community"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_leave_guild(id, window, cx)
                                        })),
                                ),
                            )
                        }),
                )
                .child(
                    div()
                        .debug_selector(|| "guild-header-menu-btn".to_owned())
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(16.))
                        .text_color(theme.foreground)
                        .child(Icon::new(if menu_open { AppIcon::X } else { AppIcon::CaretDown }).with_size(px(16.)))
                        .flx("app.guild-header.caret-icon"),
                );

            let popout = menu_open.then(|| {
                div()
                    .id("guild-header-popout")
                    .debug_selector(|| "guild-header-popout".to_owned())
                    .mx_2()
                    .mt_1()
                    .mb_1()
                    .p_1p5()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background_secondary)
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .when(view.can_manage_guild_channels, |d| {
                        d.child(
                            div()
                                .id("guild-popout-create-channel")
                                .debug_selector(|| "guild-popout-create-channel".to_owned())
                                .px_2()
                                .py_1p5()
                                .rounded(px(5.))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.muted))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_create_channel(None, window, cx);
                                }))
                                .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(l10n::tr("Create channel")))
                                .child(Icon::new(AppIcon::Plus).xsmall().text_color(theme.muted_foreground)),
                        )
                    })
                    .when(can_manage_roles, |d| {
                        d.child(
                            div()
                                .id("guild-popout-roles")
                                .debug_selector(|| "guild-popout-roles".to_owned())
                                .px_2()
                                .py_1p5()
                                .rounded(px(5.))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.muted))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_access(Scope::Guild(id), window, cx);
                                }))
                                .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(l10n::tr("Community Settings")))
                                .child(Icon::new(AppIcon::Gear).xsmall().text_color(theme.muted_foreground)),
                        )
                    })
                    .when(view.can_ban_members, |d| {
                        d.child(
                            div()
                                .id("guild-popout-bans")
                                .debug_selector(|| "guild-popout-bans".to_owned())
                                .px_2()
                                .py_1p5()
                                .rounded(px(5.))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.muted))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_guild_bans(window, cx);
                                }))
                                .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(l10n::tr("Bans")))
                                .child(Icon::new(AppIcon::Prohibit).xsmall().text_color(theme.muted_foreground)),
                        )
                    })
                    .when(can_leave, |d| {
                        d.child(
                            div()
                                .id("guild-popout-leave")
                                .debug_selector(|| "guild-popout-leave".to_owned())
                                .px_2()
                                .py_1p5()
                                .rounded(px(5.))
                                .cursor_pointer()
                                .text_color(theme.danger)
                                .hover(|s| s.bg(theme.muted))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_leave_guild(id, window, cx);
                                }))
                                .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(l10n::tr("Leave community")))
                                .child(Icon::new(AppIcon::SignOutFill).with_size(px(20.)).text_color(theme.danger)),
                        )
                    })
            });

            div()
                .flex()
                .flex_col()
                .child(header_row)
                .children(popout)
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
        .pt(px(11.))
        .pb_2()
        .gap(px(1.))
        .overflow_y_scroll();
    if view.place == Place::Home {
        let active = view.selected_channel.is_none();
        list = list
            .child(
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
                    .mx_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(6.))
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
                            .child(Icon::new(AppIcon::Users).xsmall())
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Friends"))),
                    )
                    .when(view.pending_incoming > 0, |d| d.child(badge(view.pending_incoming, theme))),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Direct messages")),
                    )
                    .child(
                        div().debug_selector(|| "create-group-dm-btn".to_owned()).child(
                            Button::new("create-group-dm-btn")
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Plus)
                                .tooltip(l10n::tr("Create group DM"))
                                .accessibility_label(l10n::tr("Create group DM"))
                                .on_click(cx.listener(|this, _, window, cx| this.open_create_group_dm(window, cx))),
                        ),
                    ),
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
        .bg(theme.background_secondary)
        .child(header_bar)
        .child(list)
}

fn render_channel(view: &ClientView, c: &ChannelItem, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (muted_fg, fg) = (theme.text_tertiary_muted, theme.foreground);
    let row_group: SharedString = format!("channel-row-{}", c.id.0).into();
    let perm_right = if c.is_category {
        76.
    } else if c.can_manage {
        32.
    } else {
        8.
    };
    let permissions = match view.place {
        Place::Guild(guild) if c.can_manage_permissions => {
            let channel = c.id;
            Some(div().id(("channel-permissions-wrap", channel.0))
                .debug_selector(move || format!("channel-permissions-{}", channel.0))
                .absolute()
                .right(px(perm_right))
                .opacity(0.)
                .group_hover(row_group.clone(), |s| s.opacity(1.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(Button::new(("channel-permissions", channel.0)).ghost().xsmall()
                    .icon(AppIcon::SlidersHorizontal)
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
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .mx_2()
            .px_2()
            .mt(px(3.))
            .h(px(32.))
            .flex_none()
            .flx("app.generic-channel-item.long-pressable.click")
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(muted_fg)
            .font_weight(FontWeight::SEMIBOLD)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .flx_text("app.channel-item-content.category-name", c.name.to_string())
                    .child(c.name.clone()),
            )
            .when(c.can_manage, |d| {
                d.child(
                    div()
                        .debug_selector(move || create_sel.clone())
                        .absolute()
                        .right(px(52.))
                        .opacity(0.)
                        .group_hover(row_group.clone(), |s| s.opacity(1.))
                        .child(
                            Button::new(("create-in-category", cat_id.0))
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Plus)
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
                        .absolute()
                        .right(px(28.))
                        .opacity(0.)
                        .group_hover(row_group.clone(), |s| s.opacity(1.))
                        .child(
                            Button::new(("edit-channel", cat_id.0))
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Gear)
                                .tooltip(l10n::tr("Category settings"))
                                .accessibility_label(l10n::tr("Category settings"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_edit_channel(cat_id, window, cx);
                                })),
                        ),
                )
            })
            .children(permissions)
            .child(div().flex_none().child(Icon::new(AppIcon::CaretDown).with_size(px(12.)).text_color(muted_fg)))
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
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .mx_2()
        .pl_2()
        .pr(px(if selected { 30. } else { 14. }))
        .min_h(px(32.))
        .py(px(6.))
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight::MEDIUM)
        .rounded(px(6.))
        .flx("app.generic-channel-item.long-pressable.click")
        .text_color(if (highlight_unread || selected) && (c.has_messages || is_voice) { fg } else { muted_fg })
        .when(selected, |d| d.bg(theme.accent))
        .hover(|s| s.bg(theme.muted).text_color(fg))
        .when(is_voice || c.has_messages, |d| d.cursor_pointer())
        .when(highlight_unread && !selected, |d| {
            d.child(
                div()
                    .absolute()
                    .left(px(-8.))
                    .top(px(12.))
                    .w(px(4.))
                    .h(px(8.))
                    .rounded_r(px(4.))
                    .bg(fg),
            )
        })
        .child(if c.icon == ChannelIcon::Dm {
            div()
                .debug_selector(|| "dm-row-avatar".to_owned())
                .flex_none()
                .child(render_avatar(
                    AvatarSpec {
                        user_id: c.dm_user_id.unwrap_or(c.id),
                        avatar_url: c.dm_avatar_url.clone(),
                        display_name: c.name.clone(),
                        size: 28.,
                        status: c.dm_status,
                        status_border_bg: Some(if selected { theme.accent } else { theme.background_secondary }),
                    },
                    theme,
                ))
                .into_any_element()
        } else {
            let icon = match c.icon {
                ChannelIcon::Voice => AppIcon::VoiceChannel,
                ChannelIcon::Announcement => AppIcon::AnnouncementChannel,
                ChannelIcon::Forum => AppIcon::ForumChannel,
                ChannelIcon::Thread => AppIcon::Chats,
                ChannelIcon::Link => AppIcon::LinkChannel,
                _ => AppIcon::TextChannel,
            };
            div()
                .size(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).with_size(px(20.)).text_color(if selected { fg } else { muted_fg }))
                .flx("app.channel-item-content.div")
                .into_any_element()
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .flx_text("app.channel-item-content.channel-name", c.name.to_string())
                .child(c.name.clone()),
        );
    if c.muted {
        row = row.child(Icon::new(AppIcon::BellSlash).xsmall());
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
                .absolute()
                .right(px(8.))
                .opacity(0.)
                .group_hover(row_group, |s| s.opacity(1.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    Button::new(("edit-channel", ch_id.0))
                        .ghost()
                        .xsmall()
                        .icon(AppIcon::Gear)
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
        let uid = member.user_id;
        let display = view.voice_member_name(member.guild_id, uid);
        let member_avatar = view.voice_member_avatar_url(member.guild_id, uid);
        let is_muted = member.self_mute || member.mute;
        let is_deaf = member.self_deaf || member.deaf;
        let is_streaming = member.self_stream;
        group = group.child(
            div()
                .id(("voice-member", uid.0))
                .flex()
                .flex_row()
                .items_center()
                .gap_1p5()
                .pl_8()
                .pr_3()
                .py_0p5()
                .child(render_avatar(
                    AvatarSpec {
                        user_id: uid,
                        avatar_url: member_avatar,
                        display_name: display.clone(),
                        size: 20.,
                        status: None,
                        status_border_bg: None,
                    },
                    theme,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted_fg)
                        .truncate()
                        .child(display),
                )
                .when(is_streaming || is_muted || is_deaf, |row| {
                    row.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .flex_none()
                            .when(is_streaming, |r| {
                                r.child(
                                    div()
                                        .debug_selector(move || format!("voice-member-live-{}", uid.0))
                                        .px_1()
                                        .rounded(px(3.))
                                        .bg(theme.danger)
                                        .text_color(theme.on_fill)
                                        .text_size(px(9.))
                                        .font_weight(FontWeight::BOLD)
                                        .child("LIVE"),
                                )
                            })
                            .when(is_muted, |r| {
                                r.child(
                                    div()
                                        .debug_selector(move || format!("voice-member-muted-{}", uid.0))
                                        .child(
                                            Icon::new(AppIcon::MicrophoneSlash)
                                                .xsmall()
                                                .text_color(muted_fg),
                                        ),
                                )
                            })
                            .when(is_deaf, |r| {
                                r.child(
                                    div()
                                        .debug_selector(move || format!("voice-member-deaf-{}", uid.0))
                                        .child(
                                            Icon::new(AppIcon::SpeakerSlash)
                                                .xsmall()
                                                .text_color(muted_fg),
                                        ),
                                )
                            }),
                    )
                }),
        );
    }
    group.into_any_element()
}
