//! Channel header above the message list (`ChannelHeader.tsx` + `ChannelHeader.module.css`):
//! left section displays the channel icon (`#` / `@` / `Megaphone`) + full channel name
//! + optional topic; right toolbar displays `[Bell]`, `[Threads]`, `[Pins]`, `[Favorites (Star)]`,
//! `[Member List (Users)]`, `[Search messages pill]`, and `[Inbox (Tray)]`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Selectable as _, Sizable as _};
use gpui_kit::{Context, FontWeight, Role, SharedString, div, prelude::*, px};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::Place;

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
                .gap_2()
                .when(!glyph.is_empty(), |d| {
                    if glyph == "#" {
                        d.child(Icon::new(AppIcon::Hash).small().text_color(theme.muted_foreground))
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
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .flx_text("channel.channel-header.channel-name", view.channel_title.to_string())
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
                .gap_1p5()
                .when_some(view.selected_channel.filter(|_| view.can_manage_selected_channel), |d, ch_id| {
                    d.child(
                        div()
                            .debug_selector(|| "edit-selected-channel-btn".to_owned())
                            .opacity(0.)
                            .group_hover(header_group, |s| s.opacity(1.))
                            .child(
                                Button::new("edit-selected-channel-btn")
                                    .ghost()
                                    .small()
                                    .icon(AppIcon::Gear)
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
                    d.child(
                        div().debug_selector(|| "toggle-channel-mute".to_owned()).flx("channel.channel-header-components.channel-notification-settings-button.channel-header-icon.click").child(
                            Button::new("toggle-channel-mute")
                                .ghost()
                                .small()
                                .icon(if channel_muted { AppIcon::BellSlash } else { AppIcon::Bell })
                                .selected(channel_muted)
                                .tooltip(label.clone())
                                .accessibility_label(label)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_channel_mute(cx))),
                        ),
                    )
                    .child(
                        div().debug_selector(|| "header-threads-btn".to_owned()).flx("threads.threads-button.channel-header-icon").child(
                            Button::new("header-threads-btn")
                                .ghost()
                                .small()
                                .icon(AppIcon::ChatsCircle)
                                .tooltip(l10n::tr("Threads"))
                                .accessibility_label(l10n::tr("Threads"))
                                .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx))),
                        ),
                    )
                    .child(
                        div().debug_selector(|| "header-pins-btn".to_owned()).flx("channel.channel-header-components.channel-pins-button.channel-header-icon").child(
                            Button::new("header-pins-btn")
                                .ghost()
                                .small()
                                .icon(AppIcon::PushPin)
                                .tooltip(l10n::tr("Pinned messages"))
                                .accessibility_label(l10n::tr("Pinned messages"))
                                .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx))),
                        ),
                    )
                    .child(
                        div().debug_selector(|| "header-favorite-btn".to_owned()).flx("channel.channel-header.icon-button.toggle-favorite").child(
                            Button::new("header-favorite-btn")
                                .ghost()
                                .small()
                                .icon(AppIcon::Star)
                                .tooltip(l10n::tr("Add to Favorites"))
                                .accessibility_label(l10n::tr("Add to Favorites")),
                        ),
                    )
                })
                .when(view.member_target.is_some() || view.group_dm.is_some(), |d| {
                    let members_label = if open {
                        l10n::tr("Hide members")
                    } else {
                        l10n::tr("Show members")
                    };
                    d.child(
                        div().debug_selector(|| "toggle-member-list".to_owned()).flx("channel.channel-header.channel-header-icon.toggle-members").child(
                            Button::new("toggle-member-list")
                                .ghost()
                                .small()
                                .icon(AppIcon::Users)
                                .selected(open)
                                .tooltip(members_label.clone())
                                .accessibility_label(members_label)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_member_list(cx))),
                        ),
                    )
                })
                .child({
                    let search_open = view.search.is_some();
                    div()
                        .id("toggle-search")
                        .role(Role::Button)
                        .aria_label(l10n::tr("Search"))
                        .debug_selector(|| "toggle-search".to_owned())
                        .w(px(theme.sizes.search_bar_width))
                        .h(px(theme.sizes.search_bar_height))
                        .px_2p5()
                        .rounded(px(theme.sizes.composer_radius))
                        .bg(theme.background_secondary)
                        .border_1()
                        .border_color(if search_open { theme.accent } else { theme.border })
                        .hover(|s| s.bg(theme.muted))
                        .cursor_pointer()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx)))
                        .flx("channel.message-search-bar.message-search-bar.input-container")
                        .child(Icon::new(AppIcon::MagnifyingGlass).xsmall().text_color(theme.muted_foreground))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .font_weight(FontWeight::NORMAL)
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("Search messages")),
                        )
                })
                .child(
                    div().debug_selector(|| "header-inbox-btn".to_owned()).flx("channel.channel-header.inbox-button").child(
                        Button::new("header-inbox-btn")
                            .ghost()
                            .small()
                            .icon(AppIcon::Tray)
                            .tooltip(l10n::tr("Inbox"))
                            .accessibility_label(l10n::tr("Inbox"))
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx))),
                    ),
                ),
        )
}
