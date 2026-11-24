//! Channel header above the message list (`ChannelHeader.tsx` + `ChannelHeader.module.css`):
//! left section displays the channel icon (`#` / `@` / `Megaphone`) + full channel name
//! + optional topic; right toolbar displays `[Bell]`, `[Threads]`, `[Pins]`, `[Favorites (Star)]`,
//! `[Member List (Users)]`, `[Search messages pill]`, and `[Inbox (Tray)]`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, Context, FontWeight, Role, SharedString, div, prelude::*, px, rgb};

use crate::client_view::{ClientView, HeaderHoverTarget};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::{l10n, rail_tooltip};
use crate::view_model::Place;

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
) -> AnyElement {
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
        .flx(flx)
        .child(
            div()
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .flx("channel.channel-header-components.channel-header-icon.button-icon")
                .child(hdr_icon(icon, color)),
        )
        .when(hovered, |d| {
            d.child(rail_tooltip::below(tooltip_label, tooltip_keys, align_right, theme))
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
    let header_group: SharedString = "channel-header-group".into();

    div()
        .group(header_group.clone())
        .h(px(theme.sizes.header_height))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
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
                .gap(px(12.))
                .when(!glyph.is_empty(), |d| {
                    if glyph == "#" {
                        d.child(Icon::new(AppIcon::TextChannel).with_size(px(24.)).text_color(theme.muted_foreground))
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
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(16.))
                        .line_height(px(24.))
                        .text_color(theme.foreground)
                        .flx_text("channel.channel-header.channel-name--8", view.channel_title.to_string())
                        .child(view.channel_title.clone()),
                )
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
                .when_some(view.selected_channel.filter(|_| view.can_manage_selected_channel), |d, ch_id| {
                    d.child(
                        div()
                            .absolute()
                            .left(px(-40.))
                            .debug_selector(|| "edit-selected-channel-btn".to_owned())
                            .opacity(0.)
                            .group_hover(header_group, |s| s.opacity(1.))
                            .child(
                                Button::new("edit-selected-channel-btn")
                                    .ghost()
                                    .with_size(px(32.))
                                    .rounded(px(16.))
                                    .icon(hdr_icon(AppIcon::Gear, theme.muted_foreground))
                                    .tooltip(l10n::tr("Channel settings"))
                                    .accessibility_label(l10n::tr("Channel settings"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_edit_channel(ch_id, window, cx)
                                    })),
                            ),
                    )
                })
                .when(view.selected_channel.is_some(), |d| {
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
                        cx.listener(|this, _, _, cx| this.toggle_channel_mute(cx)),
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
                        cx.listener(|this, _, window, cx| this.toggle_search(window, cx)),
                    ))
                    .child(render_header_icon_button(
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
                        cx.listener(|this, _, window, cx| this.toggle_search(window, cx)),
                    ))
                    .child(render_header_icon_button(
                        "header-favorite-btn",
                        "channel.channel-header.icon-button.toggle-favorite",
                        AppIcon::StarBold,
                        l10n::tr("Add to favorites"),
                        l10n::tr("Add to favorites"),
                        &[],
                        false,
                        false,
                        view.hovered_header_item == Some(HeaderHoverTarget::Favorite),
                        theme,
                        cx.listener(|this, hovered: &bool, _, cx| {
                            this.set_header_hover(HeaderHoverTarget::Favorite, *hovered, cx);
                        }),
                        |_, _, _| {},
                    ))
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
                    ))
                })
                .child({
                    let search_open = view.search.is_some();
                    div()
                        .id("toggle-search")
                        .role(Role::Button)
                        .aria_label(l10n::tr("Search"))
                        .debug_selector(|| "toggle-search".to_owned())
                        .w(px(theme.sizes.search_bar_width))
                        .h(px(38.))
                        .px_2p5()
                        .mr(px(40.))
                        .rounded(px(12.))
                        .bg(rgb(0x242229))
                        .when(search_open, |d| d.border_1().border_color(theme.accent))
                        .hover(|s| s.bg(theme.control_hover_bg))
                        .cursor_pointer()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx)))
                        .flx("channel.message-search-bar.message-search-bar.input-container")
                        .child(Icon::new(AppIcon::MagnifyingGlass).with_size(px(16.)).text_color(theme.muted_foreground))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(14.))
                                .line_height(px(22.))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("Search messages")),
                        )
                })
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
                    cx.listener(|this, _, window, cx| this.toggle_search(window, cx)),
                )),
        )
}

fn hdr_icon(icon: AppIcon, color: gpui_kit::Hsla) -> Icon {
    Icon::new(icon).with_size(px(24.)).text_color(color)
}
