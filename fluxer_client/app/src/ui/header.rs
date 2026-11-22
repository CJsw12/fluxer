//! Channel header above the message list: glyph and channel name, and on
//! guild channels the member list toggle (people icon, like the web
//! `ChannelHeader` "Show Member List" button).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Selectable as _, Sizable as _};
use gpui_kit::{Context, FontWeight, Role, div, prelude::*, px};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::Place;

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let glyph = view.channel_glyph;
    let open = if view.group_dm.is_some() { view.group_dm_panel_open } else { view.member_list_open };
    let channel_muted = view.selected_channel_muted();
    let is_dm = view.place == Place::Home;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .font_weight(FontWeight::BOLD)
        .when(!glyph.is_empty(), |d| d.child(div().text_color(theme.muted_foreground).text_lg().child(glyph)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().min_w_0().truncate().child(view.channel_title.clone()))
                .when_some(view.channel_topic.clone(), |d, topic| {
                    d.child(
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
        .when_some(view.selected_channel.filter(|_| view.can_manage_selected_channel), |d, ch_id| {
            d.child(
                div().debug_selector(|| "edit-selected-channel-btn".to_owned()).child(
                    Button::new("edit-selected-channel-btn")
                        .ghost()
                        .small()
                        .icon(AppIcon::Gear)
                        .tooltip(l10n::tr("Channel settings"))
                        .accessibility_label(l10n::tr("Channel settings"))
                        .on_click(cx.listener(move |this, _, window, cx| this.open_edit_channel(ch_id, window, cx))),
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
                div().debug_selector(|| "toggle-channel-mute".to_owned()).child(
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
        })
        .child({
            let enabled = view.notifier.enabled;
            let notif_label = if enabled {
                l10n::tr("Mute notifications")
            } else {
                l10n::tr("Enable notifications")
            };
            Button::new("toggle-notifications")
                .ghost()
                .small()
                .icon(if enabled { AppIcon::SpeakerHigh } else { AppIcon::SpeakerSlash })
                .tooltip(notif_label.clone())
                .accessibility_label(notif_label)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_notifications(cx)))
        })
        .when(view.member_target.is_some() || view.group_dm.is_some(), |d| {
            let members_label = if open {
                l10n::tr("Hide members")
            } else {
                l10n::tr("Show members")
            };
            d.child(
                div().debug_selector(|| "toggle-member-list".to_owned()).child(
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
                .w(px(180.))
                .h(px(28.))
                .px_2()
                .rounded(px(6.))
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
}
