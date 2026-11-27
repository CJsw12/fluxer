//! Channel sidebar: heading, the channels of the selected place, and the
//! user panel at the bottom.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, Context, FontWeight, deferred, MouseButton, Role, SharedString, div, prelude::*, px, rgb, rgba};

use super::avatar::{AvatarSpec, render_avatar};
use super::{access::Scope, badge};
use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::{ChannelIcon, ChannelItem, Place};

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    if view.place == Place::Explore {
        return super::explore::render_sidebar(view, theme, cx);
    }
    let header_bar = match view.place {
        Place::Home => div()
            .h(px(theme.sizes.header_height))
            .flex_none()
            .px_2()
            .bg(theme.background_secondary)
            .border_b_1()
            .border_color(theme.user_area_divider)
            .flex()
            .items_center()
            .flx("channel.direct-message.dm-list.dm-list-header.show.button")
            .child(
                div()
                    .id("dm-quick-switcher-btn")
                    .role(Role::Button)
                    .aria_label(l10n::tr("Quick switcher"))
                    .debug_selector(|| "dm-quick-switcher-btn".to_owned())
                    .relative()
                    .w_full()
                    .h(px(28.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(px(12.))
                    .rounded(px(6.))
                    .text_color(theme.muted_foreground)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover_bg).text_color(theme.foreground))
                    .on_click(cx.listener(|this, _, window, cx| this.open_quick_switcher(window, cx)))
                    .flx("channel.direct-message.dm-list.dm-list-header-button")
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(19.5))
                            .font_weight(FontWeight::MEDIUM)
                            .flx("channel.direct-message.dm-list.dm-list-header-text")
                            .child(l10n::tr("Quick switcher")),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(8.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(2.))
                            .flex_none()
                            .flx("channel.direct-message.dm-list.dm-list-header-shortcut")
                            .child(
                                div()
                                    .h(px(18.))
                                    .min_w(px(18.))
                                    .px(px(4.))
                                    .rounded(px(4.))
                                    .bg(theme.background_modifier_accent)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.foreground)
                                    .child("CTRL"),
                            )
                            .child(
                                div()
                                    .h(px(18.))
                                    .min_w(px(18.))
                                    .px(px(4.))
                                    .rounded(px(4.))
                                    .bg(theme.background_modifier_accent)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.foreground)
                                    .child("K"),
                            ),
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
            let can_manage_roles = view.backend.state().can_manage_guild_roles(id);
            let menu_open = view.guild_header_popout_open;
            let header_row = div()
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
                .hover(|s| s.bg(theme.hover_bg))
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
                        .debug_selector(|| "guild-header-menu-btn".to_owned())
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(16.))
                        .text_color(theme.foreground)
                        .child({
                            let caret = Icon::new(AppIcon::CaretDown).with_size(px(16.));
                            if menu_open { caret.rotate(gpui_kit::radians(std::f32::consts::PI)) } else { caret }
                        })
                        .flx("app.guild-header.caret-icon"),
                );

            let dark = theme.background.l < 0.5;
            let popout_bg: gpui_kit::Hsla = if dark { rgb(0x2a2730).into() } else { theme.background_secondary };
            let popout_border: gpui_kit::Hsla = if dark { rgba(0xc9c5d326).into() } else { theme.border };
            let item_color: gpui_kit::Hsla = if dark { rgb(0xf4f4f6).into() } else { theme.foreground };
            let danger_color: gpui_kit::Hsla = rgb(0xe8605f).into();
            let is_owner = view.current_guild_is_owner();
            let can_manage = view.can_manage_guild_channels;
            let (can_invite, hide_muted) = {
                let state = view.backend.state();
                (
                    crate::ui::invite_modal::can_invite_in_guild(&state, id, view.selected_channel),
                    state.guild_hides_muted_channels(id),
                )
            };
            let hover_bg = theme.hover_bg;
            let item = move |key: &'static str, label: &'static str, color: gpui_kit::Hsla| {
                div()
                    .id(key)
                    .debug_selector(move || key.to_owned())
                    .flx("guild.guild-header-popout.guild-header-popout-item.item-button.select")
                    .w_full()
                    .h(px(32.))
                    .flex_none()
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_color(color)
                    .hover(move |s| s.bg(hover_bg))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .font_weight(FontWeight::MEDIUM)
                            .flx_text("guild.guild-header-popout.guild-header-popout-item.item-title", label)
                            .child(l10n::tr(label)),
                    )
            };
            let icon = |i: AppIcon, color: gpui_kit::Hsla| {
                div().flex_none().size(px(20.)).mr(px(1.)).child(Icon::new(i).with_size(px(20.)).text_color(color))
            };
            let popout = menu_open.then(|| {
                let c = item_color;
                let list = div()
                    .id("guild-header-popout")
                    .debug_selector(|| "guild-header-popout".to_owned())
                    .flx("guild.guild-header-popout.container")
                    .absolute()
                    .top(px(theme.sizes.header_height + 8.))
                    .left(px(16.))
                    .w(px(288.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(popout_border)
                    .bg(popout_bg)
                    .occlude()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .when(can_invite, |d| {
                        d.child(
                            item("guild-popout-invite", "Invite members", c)
                                .child(icon(AppIcon::UserPlusFill, c))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_invite_members(id, window, cx);
                                })),
                        )
                    })
                    .when(can_manage_roles, |d| {
                        d.child(
                            item("guild-popout-roles", "Community settings", c)
                                .child(icon(AppIcon::Gear, c))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_access(Scope::Guild(id), window, cx);
                                })),
                        )
                    })
                    .when(can_manage, |d| {
                        d.child(
                            item("guild-popout-create-channel", "Create channel", c)
                                .child(icon(AppIcon::PlusCircleFill, c))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_create_channel(None, window, cx);
                                })),
                        )
                        .child(
                            item("guild-popout-create-category", "Create category", c)
                                .child(icon(AppIcon::FolderPlusFill, c))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_create_category(window, cx);
                                })),
                        )
                    })
                    .child(
                        item("guild-popout-notifications", "Notification settings", c)
                            .child(icon(AppIcon::Bell, c))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.guild_header_popout_open = false;
                                this.open_guild_notifications(id, window, cx);
                            })),
                    )
                    .child(
                        item("guild-popout-privacy", "Privacy settings", c)
                            .child(icon(AppIcon::Shield, c))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.guild_header_popout_open = false;
                                this.open_guild_privacy(id, window, cx);
                            })),
                    )
                    .child(
                        item("guild-popout-edit-profile", "Edit community profile", c)
                            .child(icon(AppIcon::UserCircle, c))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.guild_header_popout_open = false;
                                this.open_guild_profile(id, window, cx);
                            })),
                    )
                    .child(
                        item("guild-popout-hide-muted", "Hide muted channels", c)
                            .flx("guild.guild-header-popout.guild-header-popout-checkbox-item.item-button.click")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_hide_muted_channels(id, cx);
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(20.))
                                    .mr(px(1.))
                                    .rounded(px(4.))
                                    .border_2()
                                    .border_color(c)
                                    .when(hide_muted, |d| d.bg(c))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(hide_muted, |d| {
                                        d.child(Icon::new(AppIcon::Check).with_size(px(14.)).text_color(popout_bg))
                                    }),
                            ),
                    )
                    .when(!is_owner, |d| {
                        d.child(
                            item("guild-popout-delete-messages", "Delete my messages", danger_color)
                                .child(icon(AppIcon::Trash, danger_color))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_delete_my_guild_messages(id, window, cx);
                                })),
                        )
                        .child(
                            item("guild-popout-leave", "Leave community", danger_color)
                                .child(icon(AppIcon::SignOutFill, danger_color))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.guild_header_popout_open = false;
                                    this.open_leave_guild(id, window, cx);
                                })),
                        )
                    });
                deferred(list).with_priority(10)
            });

            div()
                .relative()
                .flex()
                .flex_col()
                .child(header_row)
                .children(popout)
                .into_any_element()
        }
        Place::Explore => return super::explore::render_sidebar(view, theme, cx),
    };
    let scroller = div()
        .id("channel-list")
        .role(Role::Navigation)
        .aria_label(l10n::tr("Channels"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .when(view.place == Place::Home, |d| {
            d.bg(theme.background_secondary)
                .flx("channel.direct-message.dm-list.desktop-scroller")
        })
        .when(view.place != Place::Home, |d| {
            d.on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    // TODO(backlog): open ChannelListContextMenu (Hide muted / Invite people / Create channel / Create category)
                    this.close_overlay(window, cx);
                }),
            )
        })
        .overflow_y_scroll();
    let mut list = div()
        .flex()
        .flex_col()
        .w_full()
        .pt(px(if view.place == Place::Home { 0. } else { 11. }))
        .pb(px(if view.place == Place::Home { 0. } else { 8. }))
        .gap(px(1.))
        .when(view.place == Place::Home, |d| {
            d.px(px(8.)).flx("channel.direct-message.dm-list.scroller-content")
        });
    if view.place == Place::Home {
        let notes_id = view
            .backend
            .state()
            .dm_channels()
            .into_iter()
            .find(|c| c.kind == fluxer_client_core::state::ChannelKind::DmPersonalNotes)
            .map(|c| c.id);
        let friends_active = view.selected_channel.is_none();
        let notes_active = notes_id.is_some() && view.selected_channel == notes_id;
        let flx = ClickableFlx {
            button: "channel.direct-message.dm-list.clickable-item.clickable-item.button",
            inner: "channel.direct-message.dm-list.clickable-item-inner",
            content: "channel.direct-message.dm-list.clickable-item-content",
            icon: "channel.direct-message.dm-list.clickable-item-icon",
            icon_svg: "channel.direct-message.dm-list.icon-size5--4",
            text: "channel.direct-message.dm-list.clickable-item-text",
        };
        let flx_notes = ClickableFlx {
            button: "channel.direct-message.dm-list.clickable-item.clickable-item.button",
            inner: "",
            content: "channel.direct-message.dm-list.clickable-item-content--2",
            icon: "channel.direct-message.dm-list.clickable-item-icon--2",
            icon_svg: "channel.direct-message.dm-list.icon-size5--5",
            text: "channel.direct-message.dm-list.clickable-item-text--2",
        };
        let pending = view.pending_incoming;
        let notes_target_id = notes_id.or_else(|| view.backend.state().current_user_id());
        list = list
            // `DirectMessageList` leading `var(--spacing-2)` spacer.
            .child(div().flex_none().h(px(8.)).flx("channel.direct-message.dm-list.div--2"))
            .child(clickable_item(
                "home-friends",
                l10n::tr("Friends"),
                AppIcon::UsersThreeFill,
                friends_active,
                flx,
                theme,
                (pending > 0).then(|| badge(pending, theme).into_any_element()),
                None,
                cx.listener(|this, _, _, cx| this.open_friends(cx)),
            ))
            .child(clickable_item(
                "home-personal-notes",
                l10n::tr("Personal notes"),
                AppIcon::NotePencilFill,
                notes_active,
                flx_notes,
                theme,
                None,
                notes_target_id.map(|cid| {
                    Box::new(cx.listener(move |this, ev: &gpui_kit::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_personal_notes_menu(cid, ev.position, window, cx);
                    }))
                        as Box<dyn Fn(&gpui_kit::MouseDownEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static>
                }),
                cx.listener(|this, _, _, cx| this.open_personal_notes(cx)),
            ))
            // `.dmSectionSeparator`: 1px, full width, 0.5rem vertical margin.
            .child(
                div()
                    .flex_none()
                    .h(px(1.))
                    .my(px(8.))
                    .mx(px(-8.))
                    .bg(theme.subtle_fill)
                    .flx("channel.direct-message.dm-list.dm-section-separator"),
            )
            // `.dmSectionHeader`: 0.5rem margins and inline padding.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .my(px(8.))
                    .px(px(8.))
                    .flx("channel.direct-message.dm-list.dm-section-header")
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .text_color(theme.muted_foreground)
                            .flx("channel.direct-message.dm-list.dm-section-header-text")
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .flx("channel.direct-message.dm-list.dm-section-header-label")
                                    .child(l10n::tr("Direct messages")),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .size(px(16.))
                            .debug_selector(|| "create-group-dm-btn".to_owned())
                            .flx("channel.direct-message.direct-message-list.dm-list.dm-create-button-tooltip-target")
                            .child(
                                div()
                                    .id("create-group-dm-btn")
                                    .role(Role::Button)
                                    .aria_label(l10n::tr("Create group DM"))
                                    .size(px(16.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .text_color(theme.muted_foreground)
                                    .hover(|s| s.text_color(theme.foreground))
                                    .on_click(cx.listener(|this, _, window, cx| this.open_create_group_dm(window, cx)))
                                    .flx("channel.direct-message.dm-list.dm-create-button.open-create-dm-modal")
                                    .tooltip(move |window, cx| Tooltip::new(l10n::tr("Create DM")).build(window, cx))
                                    .child(
                                        div()
                                            .size(px(16.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .flx("channel.direct-message.dm-list.icon-size4--2")
                                            .child(Icon::new(AppIcon::Plus).with_size(px(16.))),
                                    ),
                            ),
                    ),
            );
    }
    if view.place == Place::Home {
        let mut dm_list = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .bg(theme.background_secondary)
            .flx("channel.direct-message.dm-list.dm-channel-list");
        for c in &view.channels {
            dm_list = dm_list.child(render_channel(view, c, theme, cx));
        }
        list = list
            .child(dm_list)
            .child(div().flex_none().h(px(73.)).flx("channel.direct-message.dm-list.bottom-spacer"));
    } else {
        for c in &view.channels {
            list = list.child(render_channel(view, c, theme, cx));
        }
    }
    div()
        .flex()
        .flex_col()
        .w(px(view.effective_sidebar_width()))
        .h_full()
        .flex_none()
        .bg(theme.background_secondary)
        .when(view.place == Place::Home, |d| {
            d.flx("channel.direct-message.dm-list.dm-list-container")
        })
        .child(header_bar)
        .child(scroller.child(list))
        .into_any_element()
}

/// Parity tags of one `ClickableItem` row of the DM list.
struct ClickableFlx {
    button: &'static str,
    inner: &'static str,
    content: &'static str,
    icon: &'static str,
    icon_svg: &'static str,
    text: &'static str,
}

/// `DirectMessageList.module.css .clickableItem`: a 42px row with a 32px
/// round icon (`--guild-list-foreground`, brand when selected) and a
/// 16px/500 label.
#[allow(clippy::too_many_arguments)]
fn clickable_item(
    id: &'static str,
    label: String,
    icon: AppIcon,
    selected: bool,
    flx: ClickableFlx,
    theme: &Theme,
    trailing: Option<AnyElement>,
    on_right_click: Option<Box<dyn Fn(&gpui_kit::MouseDownEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static>>,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let (fg, icon_bg, icon_fg) = if selected {
        (theme.foreground, theme.brand, theme.on_fill)
    } else {
        (theme.muted_foreground, theme.guild_list_foreground, theme.foreground)
    };
    let content = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex_none()
                .size(px(32.))
                .rounded_full()
                .bg(icon_bg)
                .flex()
                .items_center()
                .justify_center()
                .flx(flx.icon)
                .child(
                    div()
                        .size(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flx(flx.icon_svg)
                        .child(Icon::new(icon).with_size(px(20.)).text_color(icon_fg)),
                ),
        )
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(24.))
                .font_weight(FontWeight::MEDIUM)
                .flx(flx.text)
                .child(label.clone()),
        )
        .flx(flx.content);
    let mut inner = div().flex().flex_row().items_center().justify_between().w_full().child(content);
    if let Some(trailing) = trailing {
        inner = inner.child(trailing);
    }
    if !flx.inner.is_empty() {
        inner = inner.flx(flx.inner);
    }
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_selected(selected)
        .debug_selector(move || id.to_owned())
        .flex()
        .flex_row()
        .items_center()
        .flex_none()
        .w_full()
        .h(px(42.))
        .px(px(8.))
        .rounded(px(6.))
        .text_color(fg)
        .cursor_pointer()
        .when(selected, |d| d.bg(theme.subtle_fill))
        .when(!selected, |d| d.hover(|s| s.bg(theme.hover_bg).text_color(theme.foreground)))
        .when_some(on_right_click, |d, handler| d.on_mouse_down(gpui_kit::MouseButton::Right, handler))
        .on_click(on_click)
        .flx(flx.button)
        .child(inner)
        .into_any_element()
}

fn render_channel(view: &ClientView, c: &ChannelItem, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (muted_fg, fg) = (theme.text_tertiary_muted, theme.foreground);
    let row_group: SharedString = format!("channel-row-{}", c.id.0).into();
    let permissions = match view.place {
        Place::Guild(guild) if c.can_manage_permissions => {
            let channel = c.id;
            Some(div().id(("channel-permissions-wrap", channel.0))
                .debug_selector(move || format!("channel-permissions-{}", channel.0))
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
        let has_affordances = c.can_manage || permissions.is_some();
        return div()
            .group(row_group.clone())
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
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
                    .id(("category-name", cat_id.0))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .flx_text("app.channel-item-content.category-name", c.name.to_string())
                    .flx("app.channel-item-content.tooltip--2")
                    .flx("app.channel-item-content.tooltip--3")
                    .tooltip({
                        let tip = c.name.clone();
                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                    })
                    .child(c.name.clone()),
            )
            .when(has_affordances, |d| {
                d.child(
                    div()
                        .id(("category-hover-affordance", cat_id.0))
                        .absolute()
                        .right(px(28.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .flex_row_reverse()
                        .items_center()
                        .gap(px(2.))
                        .opacity(0.)
                        .group_hover(row_group.clone(), |s| s.opacity(1.))
                        .when(c.can_manage, |d| {
                            d.child(
                                div()
                                    .debug_selector(move || edit_sel.clone())
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
                            .child(
                                div()
                                    .debug_selector(move || create_sel.clone())
                                    .flx("app.channel-item.tooltip--2")
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
                        })
                        .children(permissions),
                )
            })
            .child(
                div()
                    .flex_none()
                    .size(px(12.))
                    .flx("app.channel-item.category-icon")
                    .child(Icon::new(AppIcon::CaretDown).with_size(px(12.)).text_color(muted_fg)),
            )
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
    let is_dm = c.icon == ChannelIcon::Dm;
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
        .gap(px(if is_dm { 12. } else { 6. }))
        .when(!is_dm, |d| d.mx_2())
        .pl_2()
        .pr_2()
        .when(is_dm, |d| d.h(px(42.)).flex_none())
        .when(!is_dm, |d| d.min_h(px(32.)).py(px(6.)))
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight::MEDIUM)
        .rounded(px(6.))
        .when(!is_dm, |d| d.flx("app.generic-channel-item.long-pressable.click"))
        .when(is_dm && selected, |d| d.flx("channel.direct-message.dm-list-item.dm-item-selected.navigate-to"))
        .text_color(if is_dm && !selected && !highlight_unread {
            theme.muted_foreground
        } else if (highlight_unread || selected) && (c.has_messages || is_voice) {
            fg
        } else {
            muted_fg
        })
        .when(selected, |d| d.bg(if is_dm { theme.subtle_fill } else { theme.accent }))
        .when(!selected, |d| d.hover(|s| s.bg(theme.hover_bg).text_color(fg)))
        .when(selected, |d| d.hover(|s| s.text_color(fg)))
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
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(if is_dm { 12. } else { 6. }))
                .when(is_dm && selected, |d| {
                    d.flx("channel.direct-message.dm-list-item.dm-item-content--2")
                })
                .when(!is_dm, |d| {
                    d.flx("app.channel-item.channel-item-core.tooltip")
                        .flx("app.channel-item.channel-item-core.tooltip--2")
                })
                .child(if c.icon == ChannelIcon::Dm {
                    div()
                        .debug_selector(|| "dm-row-avatar".to_owned())
                        .flex_none()
                        .size(px(32.))
                        .when(selected, |d| d.flx("channel.direct-message.dm-list-item.dm-item-avatar-wrapper--2"))
                        .child(render_avatar(
                            AvatarSpec {
                                user_id: c.dm_user_id.unwrap_or(c.id),
                                avatar_url: c.dm_avatar_url.clone(),
                                display_name: c.name.clone(),
                                size: 32.,
                                status: c.dm_status,
                                status_border_bg: Some(theme.background_secondary),
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
                        .id(("channel-icon", c.id.0))
                        .size(px(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .flex_none()
                                .size(px(20.))
                                .flx(match c.icon {
                                    ChannelIcon::Voice => "channel.channel-utils.get-icon.voice-channel-icon",
                                    _ => "channel.channel-utils.get-icon.text-channel-icon",
                                })
                                .child(
                                    Icon::new(icon)
                                        .with_size(px(20.))
                                        .text_color(if selected { fg } else { muted_fg }),
                                ),
                        )
                        .flx("app.channel-item-content.div")
                        .flx("app.channel-item-icon.tooltip")
                        .tooltip({
                            let tip = c.name.clone();
                            move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                        })
                        .into_any_element()
                })
                .child(if is_dm {
                    // `.dmItemInfo` / `.dmItemName` (20px lines) around the name text.
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .h(px(20.))
                        .when(selected, |d| {
                            d.flx("channel.direct-message.dm-list-item.dm-item-info--2")
                                .flx("channel.direct-message.dm-list-item.dm-item-name--2")
                        })
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .when(selected, |d| {
                                    d.flx_text("channel.direct-message.dm-list-item.dm-item-name-text--2", c.name.to_string())
                                })
                                .child(c.name.clone()),
                        )
                        .into_any_element()
                } else {
                    div()
                        .id(("channel-name", c.id.0))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .flx_text("app.channel-item-content.channel-name", c.name.to_string())
                        .flx("app.channel-item-content.tooltip")
                        .tooltip({
                            let tip = c.name.clone();
                            move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                        })
                        .child(c.name.clone())
                        .into_any_element()
                })
                .when(is_dm, |d| {
                    let close_fg = if selected { fg } else { theme.muted_foreground };
                    d.child(
                        div()
                            .id(("dm-close", c.id.0))
                            .role(Role::Button)
                            .aria_label(format!("{} {}", l10n::tr("Close Direct Message"), c.name))
                            .flex_none()
                            .size(px(24.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(16.))
                            .line_height(px(24.))
                            .font_weight(FontWeight::NORMAL)
                            .cursor_pointer()
                            .opacity(0.)
                            .group_hover(row_group.clone(), |s| s.opacity(1.))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                // TODO(backlog): close DM channel (REST DELETE /channels/{id})
                                this.close_overlay(window, cx);
                            }))
                            .when(selected, |d| {
                                d.flx("channel.direct-message.dm-list-item.dm-item-close-button.remove-channel")
                            })
                            .child(
                                div()
                                    .size(px(16.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(selected, |d| d.flx("channel.direct-message.dm-list-item.icon-size4"))
                                    .child(Icon::new(AppIcon::X).with_size(px(16.)).text_color(close_fg)),
                            ),
                    )
                }),
        )
        // Official trailing slot: gap plus reserved icon space (16px when selected).
        .when(!is_dm, |d| d.child(div().flex_none().w(px(if selected { 16. } else { 0. }))));
    if c.muted {
        row = row.child(Icon::new(AppIcon::BellSlash).xsmall());
    }
    if c.unread.mention_count > 0 {
        row = row.child(badge(c.unread.mention_count, theme));
    } else if highlight_unread {
        row = row.child(div().text_color(fg).child("•"));
    }
    let voice_members = if is_voice {
        view.voice.members_in_channel(c.id)
    } else {
        Vec::new()
    };
    if is_voice && !voice_members.is_empty() {
        let count_label = format!("{}", voice_members.len());
        let hover_affordances_active = can_edit_row || matches!(view.place, Place::Guild(_) if c.can_manage_permissions);
        row = row.child(
            div()
                .mr(px(8.))
                .px(px(6.))
                .h(px(16.))
                .rounded(px(8.))
                .bg(theme.background_modifier_accent)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted_fg)
                .flx("app.channel-item.voice-user-count")
                .when(!hover_affordances_active, |d| {
                    d.group_hover(row_group.clone(), |s| s.opacity(0.))
                })
                .child(count_label),
        );
    }
    if can_edit_row || permissions.is_some() {
        let ch_id = c.id;
        let edit_sel = format!("edit-channel-{}", ch_id.0);
        row = row.child(
            div()
                .id(("channel-hover-affordance", ch_id.0))
                .absolute()
                .right(px(8.))
                .top_0()
                .bottom_0()
                .flex()
                .flex_row_reverse()
                .items_center()
                .gap(px(4.))
                .opacity(if selected { 1. } else { 0. })
                .when(!selected, |d| d.group_hover(row_group, |s| s.opacity(1.)))
                .when(can_edit_row, |d| {
                    d.child(
                        div()
                            .id(("edit-channel-wrap", ch_id.0))
                            .debug_selector(move || edit_sel.clone())
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
                    )
                })
                .children(permissions),
        );
    }
    if c.has_messages {
        let id = c.id;
        row = row.on_click(cx.listener(move |this, _, _, cx| this.select_channel(id, cx)));
    } else if is_voice {
        let id = c.id;
        row = row.on_click(cx.listener(move |this, _, _, cx| this.join_voice_channel(id, cx)));
    }
    if !is_voice {
        return row.into_any_element();
    }
    let members = voice_members;
    if members.is_empty() {
        return row.into_any_element();
    }
    let mut list = div()
        .pl(px(24.))
        .pr(px(8.))
        .pt(px(4.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .flx("app.voice-participants-list.container");
    for member in members {
        let uid = member.user_id;
        let display = view.voice_member_name(member.guild_id, uid);
        let member_avatar = view.voice_member_avatar_url(member.guild_id, uid);
        let is_muted = member.self_mute || member.mute;
        let is_deaf = member.self_deaf || member.deaf;
        let is_streaming = member.self_stream;
        list = list.child(
            div()
                .id(("voice-member", uid.0))
                .h(px(32.))
                .px(px(6.))
                .rounded(px(6.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover_bg))
                .flx("app.voice-participant-item.participant-row.click")
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        // TODO(backlog): open VoiceParticipantContextMenu (local mute / volume / entrance sound)
                        this.close_overlay(window, cx);
                    }),
                )
                .child(render_avatar(
                    AvatarSpec {
                        user_id: uid,
                        avatar_url: member_avatar,
                        display_name: display.clone(),
                        size: 24.,
                        status: None,
                        status_border_bg: None,
                    },
                    theme,
                ))
                .child(
                    div()
                        .id(("voice-member-name", uid.0))
                        .flex_1()
                        .min_w_0()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted_fg)
                        .truncate()
                        .flx_text("app.voice-participant-item.participant-name", display.to_string())
                        .flx("app.voice-participant-item.tooltip")
                        .tooltip({
                            let tip = display.clone();
                            move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                        })
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
                            .flx("app.voice-participant-item.icons-container")
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .flx("app.voice-state-icons.container")
                                    .when(is_streaming, |r| {
                                        r.child(
                                            div()
                                                .id(("voice-member-live", uid.0))
                                                .debug_selector(move || format!("voice-member-live-{}", uid.0))
                                                .px_1()
                                                .rounded(px(3.))
                                                .bg(theme.danger)
                                                .text_color(theme.on_fill)
                                                .text_size(px(9.))
                                                .font_weight(FontWeight::BOLD)
                                                .flx("app.voice-state-icons.tooltip")
                                                .tooltip(move |window, cx| {
                                                    Tooltip::new(l10n::tr("Live")).build(window, cx)
                                                })
                                                .child("LIVE"),
                                        )
                                    })
                                    .when(is_muted, |r| {
                                        r.child(
                                            div()
                                                .id(("voice-member-muted", uid.0))
                                                .debug_selector(move || format!("voice-member-muted-{}", uid.0))
                                                .size(px(16.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .flx("app.voice-state-icons.icon")
                                                .flx("app.voice-state-icons.tooltip--2")
                                                .tooltip(move |window, cx| {
                                                    Tooltip::new(l10n::tr("Muted")).build(window, cx)
                                                })
                                                .child(
                                                    Icon::new(AppIcon::MicrophoneSlash)
                                                        .with_size(px(16.))
                                                        .text_color(theme.muted_foreground),
                                                ),
                                        )
                                    })
                                    .when(is_deaf, |r| {
                                        r.child(
                                            div()
                                                .id(("voice-member-deaf", uid.0))
                                                .debug_selector(move || format!("voice-member-deaf-{}", uid.0))
                                                .size(px(16.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .flx("app.voice-state-icons.icon")
                                                .flx("app.voice-state-icons.tooltip--3")
                                                .tooltip(move |window, cx| {
                                                    Tooltip::new(l10n::tr("Deafened")).build(window, cx)
                                                })
                                                .child(
                                                    Icon::new(AppIcon::SpeakerSlash)
                                                        .with_size(px(16.))
                                                        .text_color(theme.muted_foreground),
                                                ),
                                        )
                                    }),
                            ),
                    )
                }),
        );
    }
    div().flex().flex_col().child(row).child(list).into_any_element()
}
