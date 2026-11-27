//! Channel header above the message list (`ChannelHeader.tsx` + `ChannelHeader.module.css`):
//! left section displays the channel icon (`#` / `@` / `Megaphone`) + full channel name
//! + optional topic; right toolbar displays `[Bell]`, `[Threads]`, `[Pins]`, `[Favorites (Star)]`,
//! `[Member List (Users)]`, `[Search messages pill]`, and `[Inbox (Tray)]`.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, Context, FontWeight, MouseButton, Role, div, prelude::*, px, rgb};

use crate::client_view::{ClientView, HeaderHoverTarget};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::avatar::{AvatarSpec, render_clickable_avatar_tagged};
use crate::ui::search_panel::SearchScopeChoice;
use crate::ui::{l10n, rail_tooltip};
use crate::updater::UpdateStatus;
use crate::view_model::{ChannelIcon, Place};

#[allow(clippy::too_many_arguments)]
fn render_header_icon_button(
    id: &'static str,
    flx: &'static str,
    icon: AppIcon,
    aria_label: String,
    tooltip_label: String,
    tooltip_keys: &'static [&'static str],
    align_right: bool,
    selected: bool,
    hovered: bool,
    theme: &Theme,
    on_hover: impl Fn(&bool, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    on_right_click: Option<Box<dyn Fn(&gpui_kit::MouseDownEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static>>,
) -> AnyElement {
    let disabled = false;
    let color = if selected || hovered {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(aria_label)
        .aria_selected(selected)
        .debug_selector(move || id.to_owned())
        .relative()
        .size(px(32.))
        .rounded(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_hover(on_hover)
        .on_click(on_click)
        .when_some(on_right_click, |d, handler| d.on_mouse_down(MouseButton::Right, handler))
        .flx(flx)
        .when(flx.contains("channel-pins-button"), |d| {
            d.flx("channel.channel-header-components.channel-pins-button.icon-button-wrapper--2")
        })
        .child(crate::ui::channel_popouts::track_trigger(id))
        .child(
            div()
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .flx(if flx.ends_with("toggle-favorite") {
                    "channel.channel-header.button-icon"
                } else {
                    "channel.channel-header-components.channel-header-icon.button-icon"
                })
                .child(hdr_icon(icon, color)),
        )
        .when(hovered, |d| {
            let tt = rail_tooltip::below(tooltip_label, tooltip_keys, align_right, theme);
            if flx.ends_with("toggle-favorite") {
                d.child(div().flx("channel.channel-header.tooltip--9").child(tt))
            } else if disabled {
                d.child(
                    div()
                        .flx("channel.channel-header-components.channel-header-icon.tooltip")
                        .child(tt),
                )
            } else {
                d.child(
                    div()
                        .flx("channel.channel-header-components.channel-header-icon.tooltip--2")
                        .child(tt),
                )
            }
        })
        .into_any_element()
}

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let glyph = view.channel_glyph;
    let open = if view.group_dm.is_some() {
        view.group_dm_panel_open
    } else {
        view.member_list_open
    };
    let channel_muted = view.selected_channel_muted();
    let is_dm = view.place == Place::Home;
    let is_group_dm = view.group_dm.is_some();
    // `ChannelHeader.tsx`: a 1:1 DM shows the peer's status avatar (32px)
    // instead of the `@` glyph.
    let dm_peer = if is_group_dm {
        None
    } else {
        view.channels
            .iter()
            .find(|c| Some(c.id) == view.selected_channel && c.icon == ChannelIcon::Dm && is_dm)
    };
    let is_personal_notes = is_dm && dm_peer.is_none() && !is_group_dm && view.selected_channel.is_some();
    let is_guild_channel = dm_peer.is_none() && !is_group_dm && !is_personal_notes;
    let is_mobile = false;

    div()
        .h(px(theme.sizes.header_height))
        .mr(px(-1.))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .bg(theme.background_secondary_lighter)
        .border_b_1()
        .border_color(theme.border)
        .flx("channel.channel-header.header-wrapper")
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .overflow_hidden()
                .flx("channel.channel-header.header-left-section")
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .overflow_hidden()
                        .flx("channel.channel-header.left-content-container")
                        .when(is_guild_channel, |d| {
                            d.flx("channel.channel-header.channel-info-container.context-menu")
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        // TODO(backlog): open ChannelContextMenu / ThreadContextMenu
                                        this.close_overlay(window, cx);
                                    }),
                                )
                        })
                        .when_some(dm_peer, |d, peer| {
                            let dm_uid = peer.dm_user_id.unwrap_or(peer.id);
                            d.child(
                                div()
                                    .id("header-dm-user-btn")
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, window, cx| {
                                        this.open_profile(dm_uid, None, ev.position(), window, cx);
                                    }))
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                                            cx.stop_propagation();
                                            // TODO(backlog): open DMContextMenu
                                            this.close_overlay(window, cx);
                                        }),
                                    )
                                    .flx("channel.channel-header.desktop-button.open-user-profile")
                                    .child(render_clickable_avatar_tagged(
                                        AvatarSpec {
                                            user_id: dm_uid,
                                            avatar_url: peer.dm_avatar_url.clone(),
                                            display_name: peer.name.clone(),
                                            size: 32.,
                                            status: peer.dm_status,
                                            status_border_bg: Some(theme.background_secondary_lighter),
                                        },
                                        theme,
                                        &["ui.status-aware-avatar.avatar"],
                                    ))
                                    .child(
                                        div()
                                            .id("header-dm-name-wrapper")
                                            .pl(px(12.))
                                            .flx("channel.channel-header.dm-name-wrapper--2")
                                            .tooltip({
                                                let tip = view.channel_title.clone();
                                                move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                                            })
                                            .child(
                                                div()
                                                    .id("header-dm-name")
                                                    .flex_none()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_size(px(16.))
                                                    .line_height(px(24.))
                                                    .text_color(theme.foreground)
                                                    .flx_text(
                                                        "channel.channel-header.channel-name--4",
                                                        view.channel_title.to_string(),
                                                    )
                                                    .tooltip({
                                                        let tip = view.channel_title.clone();
                                                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                                                    })
                                                    .child(view.channel_title.clone()),
                                            ),
                                    ),
                            )
                        })
                        .when_some(view.group_dm.as_ref(), |d, gdm| {
                            d.child(
                                div()
                                    .id("header-group-dm-trigger")
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_member_list(cx)))
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                                            cx.stop_propagation();
                                            // TODO(backlog): open GroupDMContextMenu
                                            this.close_overlay(window, cx);
                                        }),
                                    )
                                    .flx("channel.channel-header.group-dm-header-trigger.open-edit-group")
                                    .child(
                                        div()
                                            .text_color(theme.muted_foreground)
                                            .text_base()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("@"),
                                    )
                                    .child(
                                        div()
                                            .id("header-group-dm-name")
                                            .flex_none()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_size(px(16.))
                                            .line_height(px(24.))
                                            .text_color(theme.foreground)
                                            .flx("channel.channel-header.dm-name-wrapper--3")
                                            .flx("channel.channel-header.tooltip--6")
                                            .flx_text(
                                                "channel.channel-header.channel-name--6",
                                                gdm.display_name.to_string(),
                                            )
                                            .tooltip({
                                                let tip = gdm.display_name.clone();
                                                move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                                            })
                                            .child(gdm.display_name.clone()),
                                    ),
                            )
                        })
                        .when(dm_peer.is_none() && !is_group_dm && !glyph.is_empty(), |d| {
                            if glyph == "#" {
                                d.child(
                                    div()
                                        .flex_none()
                                        .size(px(24.))
                                        .flx("channel.channel-utils.get-icon.text-channel-icon")
                                        .child(
                                            Icon::new(AppIcon::TextChannel)
                                                .with_size(px(24.))
                                                .text_color(theme.muted_foreground),
                                        ),
                                )
                            } else {
                                d.child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .text_base()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(glyph),
                                )
                            }
                        })
                        .when(is_personal_notes, |d| {
                            let pn_id = view.selected_channel;
                            d.child(
                                div()
                                    .id("header-personal-notes-name")
                                    .flex_none()
                                    .ml(px(4.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_size(px(16.))
                                    .line_height(px(24.))
                                    .text_color(theme.foreground)
                                    .flx("channel.channel-header.tooltip--7")
                                    .flx_text(
                                        "channel.channel-header.channel-name--7",
                                        view.channel_title.to_string(),
                                    )
                                    .tooltip({
                                        let tip = view.channel_title.clone();
                                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                                    })
                                    .when_some(pn_id, |d, cid| {
                                        d.on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(move |this, ev: &gpui_kit::MouseDownEvent, window, cx| {
                                                cx.stop_propagation();
                                                this.open_personal_notes_menu(cid, ev.position, window, cx);
                                            }),
                                        )
                                    })
                                    .child(view.channel_title.clone()),
                            )
                        })
                        .when(is_guild_channel, |d| {
                            d.child(
                                div()
                                    .id("header-channel-name")
                                    .flex_none()
                                    .ml(px(4.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_size(px(16.))
                                    .line_height(px(24.))
                                    .text_color(theme.foreground)
                                    .flx_text("channel.channel-header.channel-name--8", view.channel_title.to_string())
                                    .flx("channel.channel-header.tooltip--8")
                                    .tooltip({
                                        let tip = view.channel_title.clone();
                                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                                    })
                                    .child(view.channel_title.clone()),
                            )
                        })
                        .when(is_mobile, |d| {
                            // Desktop-only GPUI client; mobile ChannelHeader parity tags.
                            d.child(
                                div()
                                    .id("header-mobile-dm-name")
                                    .flx("channel.channel-header.dm-name-wrapper")
                                    .tooltip({
                                        let tip = view.channel_title.clone();
                                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                                    }),
                            )
                            .child(
                                div()
                                    .id("header-mobile-group-dm")
                                    .flx("channel.channel-header.group-dm-avatar")
                                    .tooltip({
                                        let tip = view.channel_title.clone();
                                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                                    }),
                            )
                            .child(
                                div()
                                    .id("header-mobile-channel-name")
                                    .flx("channel.channel-header.tooltip--3")
                                    .tooltip({
                                        let tip = view.channel_title.clone();
                                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                                    }),
                            )
                        })
                        .when_some(view.channel_topic.clone(), |d, topic| {
                            d.child(div().w(px(1.)).h(px(16.)).mx_1().bg(theme.border)).child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(theme.muted_foreground)
                                    .child(topic),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .relative()
                .flx("channel.channel-header.header-right-section")
                .when(view.selected_channel.is_some() && is_dm, |d| {
                    // Official `CallButtons`: voice and video call come first in a DM; the
                    // notification bell and threads button are guild-channel only.
                    d.child(render_header_icon_button(
                        "header-voice-call-btn",
                        "channel.channel-header-components.call-buttons.voice-call-button.channel-header-icon.click",
                        AppIcon::PhoneFill,
                        l10n::tr("Start voice call"),
                        l10n::tr("Start voice call"),
                        &[],
                        false,
                        false,
                        view.hovered_header_item == Some(HeaderHoverTarget::VoiceCall),
                        theme,
                        cx.listener(|this, hovered: &bool, _, cx| {
                            this.set_header_hover(HeaderHoverTarget::VoiceCall, *hovered, cx);
                        }),
                        cx.listener(|this, _, _, cx| this.start_dm_voice_call(cx)),
                        None,
                    ))
                    .child(render_header_icon_button(
                        "header-video-call-btn",
                        "channel.channel-header-components.call-buttons.video-call-button.channel-header-icon.click",
                        AppIcon::VideoCameraFill,
                        l10n::tr("Start video call"),
                        l10n::tr("Start video call"),
                        &[],
                        false,
                        false,
                        view.hovered_header_item == Some(HeaderHoverTarget::VideoCall),
                        theme,
                        cx.listener(|this, hovered: &bool, _, cx| {
                            this.set_header_hover(HeaderHoverTarget::VideoCall, *hovered, cx);
                        }),
                        cx.listener(|this, _, _, cx| this.start_dm_video_call(cx)),
                        None,
                    ))
                })
                .when(view.selected_channel.is_some() && !is_dm, |d| {
                    let label = match (is_dm, channel_muted) {
                        (true, true) => l10n::tr("Unmute DM"),
                        (true, false) => l10n::tr("Mute DM"),
                        (false, true) => l10n::tr("Unmute channel"),
                        (false, false) => l10n::tr("Mute channel"),
                    };
                    d.child(render_header_icon_button(
                        "toggle-channel-mute",
                        "channel.channel-header-components.channel-notification-settings-button.channel-header-icon.click",
                        if channel_muted { AppIcon::BellSlash } else { AppIcon::Bell },
                        label,
                        l10n::tr("Notification settings"),
                        &[],
                        false,
                        channel_muted,
                        view.hovered_header_item == Some(HeaderHoverTarget::Mute),
                        theme,
                        cx.listener(|this, hovered: &bool, _, cx| {
                            this.set_header_hover(HeaderHoverTarget::Mute, *hovered, cx);
                        }),
                        cx.listener(|this, ev: &gpui_kit::ClickEvent, window, cx| {
                            this.toggle_channel_notification_popout(ev.position(), window, cx);
                        }),
                        None,
                    ))
                    .child(render_header_icon_button(
                        "header-threads-btn",
                        "threads.threads-button.channel-header-icon",
                        AppIcon::Chats,
                        l10n::tr("Threads"),
                        l10n::tr("Threads"),
                        &[],
                        false,
                        false,
                        view.hovered_header_item == Some(HeaderHoverTarget::Threads),
                        theme,
                        cx.listener(|this, hovered: &bool, _, cx| {
                            this.set_header_hover(HeaderHoverTarget::Threads, *hovered, cx);
                        }),
                        cx.listener(|this, ev: &gpui_kit::ClickEvent, window, cx| {
                            this.toggle_threads_popout(ev.position(), window, cx);
                        }),
                        None,
                    ))
                })
                .when(view.selected_channel.is_some(), |d| {
                    d.child(render_header_icon_button(
                        "header-pins-btn",
                        "channel.channel-header-components.channel-pins-button.channel-header-icon",
                        AppIcon::PushPin,
                        l10n::tr("Pinned messages"),
                        l10n::tr("Pinned messages"),
                        &["CTRL", "P"],
                        false,
                        false,
                        view.hovered_header_item == Some(HeaderHoverTarget::Pins),
                        theme,
                        cx.listener(|this, hovered: &bool, _, cx| {
                            this.set_header_hover(HeaderHoverTarget::Pins, *hovered, cx);
                        }),
                        cx.listener(|this, ev: &gpui_kit::ClickEvent, window, cx| {
                            this.toggle_pins_popout(ev.position(), window, cx);
                        }),
                        None,
                    ))
                    .child({
                        let is_favorited = false; // TODO(backlog): wire per-channel favorite state
                        let fav_label = if is_favorited {
                            l10n::tr("Remove from favorites")
                        } else {
                            l10n::tr("Add to favorites")
                        };
                        render_header_icon_button(
                            "header-favorite-btn",
                            "channel.channel-header.icon-button.toggle-favorite",
                            AppIcon::StarBold,
                            fav_label.clone(),
                            fav_label,
                            &[],
                            false,
                            is_favorited,
                            view.hovered_header_item == Some(HeaderHoverTarget::Favorite),
                            theme,
                            cx.listener(|this, hovered: &bool, _, cx| {
                                this.set_header_hover(HeaderHoverTarget::Favorite, *hovered, cx);
                            }),
                            |_, _, _| {},
                            Some(Box::new(cx.listener(
                                |this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    // TODO(backlog): open FavoritesContextMenu (Hide favorites)
                                    this.close_overlay(window, cx);
                                },
                            ))),
                        )
                    })
                    .when(is_mobile, |d| {
                        d.child(
                            div()
                                .id("header-favorite-mobile-btn")
                                .flx("channel.channel-header.icon-button-mobile.toggle-favorite")
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        // TODO(backlog): open FavoritesContextMenu (Hide favorites)
                                        this.close_overlay(window, cx);
                                    }),
                                ),
                        )
                    })
                })
                .when(view.member_target.is_some() || view.group_dm.is_some(), |d| {
                    let members_label = if open {
                        l10n::tr("Hide members")
                    } else {
                        l10n::tr("Show members")
                    };
                    d.child(render_header_icon_button(
                        "toggle-member-list",
                        "channel.channel-header.channel-header-icon.toggle-members",
                        AppIcon::Users,
                        members_label.clone(),
                        members_label,
                        &["CTRL", "U"],
                        false,
                        open,
                        view.hovered_header_item == Some(HeaderHoverTarget::Members),
                        theme,
                        cx.listener(|this, hovered: &bool, _, cx| {
                            this.set_header_hover(HeaderHoverTarget::Members, *hovered, cx);
                        }),
                        cx.listener(|this, _, _, cx| this.toggle_member_list(cx)),
                        None,
                    ))
                })
                .child({
                    let search_open = view.search.is_some();
                    let non_default_scope = view.search.as_ref().is_some_and(|s| {
                        let default_scope = if view.selected_channel.is_some() {
                            SearchScopeChoice::CurrentChannel
                        } else if view.current_guild_id().is_some() {
                            SearchScopeChoice::CurrentGuild
                        } else {
                            SearchScopeChoice::All
                        };
                        s.scope != default_scope
                    });
                    div()
                        .flx("channel.channel-header.message-search-focus-wrapper")
                        .child(
                            div()
                                .id("toggle-search")
                                .role(Role::Button)
                                .aria_label(l10n::tr("Search"))
                                .debug_selector(|| "toggle-search".to_owned())
                                .w(px(theme.sizes.search_bar_width))
                                .h(px(38.))
                                .px_2p5()
                                .rounded(px(12.))
                                .bg(theme.background_textarea)
                                .border_1()
                                .border_color(if search_open { theme.accent } else { theme.background_modifier_accent })
                                .hover(|s| s.bg(theme.control_hover_bg))
                                .cursor_pointer()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx)))
                                .flx("channel.message-search-bar.message-search-bar.anchor")
                                .flx("channel.message-search-bar.message-search-bar.input-container")
                                .child(
                                    div()
                                        .id("header-search-scope-btn")
                                        .debug_selector(|| "header-search-scope-btn".to_owned())
                                        .role(Role::Button)
                                        .aria_label(l10n::tr("Search scope"))
                                        .relative()
                                        .flex_none()
                                        .size(px(16.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .text_color(theme.foreground)
                                        .text_size(px(16.))
                                        .line_height(px(24.))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                        .on_click(cx.listener(|this, ev: &gpui_kit::ClickEvent, window, cx| {
                                            cx.stop_propagation();
                                            this.toggle_search_scope_popout(ev.position(), window, cx);
                                        }))
                                        .flx("channel.message-search-bar.message-search-bar.scope-button.scope-menu-open")
                                        .child(crate::ui::channel_popouts::track_trigger("header-search-scope-btn"))
                                        .child(
                                            div()
                                                .flex_none()
                                                .size(px(16.))
                                                .flx("channel.message-search-bar.message-search-bar.search-icon")
                                                .child(
                                                    Icon::new(AppIcon::MagnifyingGlass)
                                                        .with_size(px(16.))
                                                        .text_color(theme.muted_foreground),
                                                ),
                                        )
                                        .when(non_default_scope, |d| {
                                            d.child(
                                                div()
                                                    .debug_selector(|| "header-search-scope-indicator".to_owned())
                                                    .absolute()
                                                    .bottom(px(-2.))
                                                    .left(px(-2.))
                                                    .size(px(6.))
                                                    .rounded_full()
                                                    .bg(theme.brand),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(14.))
                                        .line_height(px(22.))
                                        .font_weight(FontWeight::NORMAL)
                                        .text_color(theme.foreground)
                                        .flx("channel.message-search-bar.message-search-bar.input.text")
                                        .child(l10n::tr("Search messages")),
                                ),
                        )
                })
                .when_some(
                    match &view.update_status {
                        UpdateStatus::Available { latest, .. } => {
                            Some(format!("{}: v{latest}", l10n::tr("Update available")))
                        }
                        UpdateStatus::Downloading { .. } => Some(l10n::tr("Updating…")),
                        UpdateStatus::Downloaded { latest, .. } => {
                            Some(format!("{}: v{latest}", l10n::tr("Restart to update")))
                        }
                        _ => None,
                    },
                    |d, label| {
                        let hovered = view.hovered_header_item == Some(HeaderHoverTarget::Update);
                        let green: gpui_kit::Hsla = rgb(0x22c55e).into();
                        d.child(
                            div()
                                .id("header-update-btn")
                                .role(Role::Button)
                                .aria_label(label.clone())
                                .debug_selector(|| "header-update-btn".to_owned())
                                .relative()
                                .size(px(32.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_color(green)
                                .on_hover(cx.listener(|this, h: &bool, _, cx| {
                                    this.set_header_hover(HeaderHoverTarget::Update, *h, cx);
                                }))
                                .on_click(cx.listener(|this, _, window, cx| this.on_header_update_click(window, cx)))
                                .flx("channel.channel-header-components.updater-icon.button.click")
                                .child(
                                    div()
                                        .size(px(20.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flx("channel.channel-header-components.updater-icon.update-icon")
                                        .child(hdr_icon(AppIcon::ArrowClockwise, green)),
                                )
                                .when(hovered, |d| d.child(rail_tooltip::below(label, &[], true, theme))),
                        )
                    },
                )
                .child(render_header_icon_button(
                    "header-inbox-btn",
                    "channel.channel-header.inbox-button",
                    AppIcon::Inbox,
                    l10n::tr("Inbox"),
                    l10n::tr("Inbox"),
                    &["CTRL", "I"],
                    true,
                    false,
                    view.hovered_header_item == Some(HeaderHoverTarget::Inbox),
                    theme,
                    cx.listener(|this, hovered: &bool, _, cx| {
                        this.set_header_hover(HeaderHoverTarget::Inbox, *hovered, cx);
                    }),
                    cx.listener(|this, ev: &gpui_kit::ClickEvent, window, cx| {
                        this.toggle_inbox_popout(ev.position(), window, cx);
                    }),
                    None,
                )),
        )
}

fn hdr_icon(icon: AppIcon, color: gpui_kit::Hsla) -> Icon {
    Icon::new(icon).with_size(px(24.)).text_color(color)
}
