//! Channel header above the message list: glyph and channel name, and on
//! guild channels the member list toggle (people icon, like the web
//! `ChannelHeader` "Show Member List" button).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::{Context, FontWeight, div, prelude::*};

use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::Place;

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let glyph = view.channel_glyph;
    let open = view.member_list_open;
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
        .child(div().flex_1().min_w_0().truncate().child(view.channel_title.clone()))
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
                        .selected(channel_muted)
                        .label(label)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_channel_mute(cx))),
                ),
            )
        })
        .child({
            let search_open = view.search.is_some();
            div().debug_selector(|| "toggle-search".to_owned()).child(
                Button::new("toggle-search")
                    .ghost()
                    .small()
                    .icon(IconName::Search)
                    .selected(search_open)
                    .tooltip(l10n::tr("Search"))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx))),
            )
        })
        .child({
            let enabled = view.notifier.enabled;
            Button::new("toggle-notifications")
                .ghost()
                .small()
                .icon(if enabled { IconName::Bell } else { IconName::BellOff })
                .tooltip(if enabled {
                    l10n::tr("Mute notifications")
                } else {
                    l10n::tr("Enable notifications")
                })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_notifications(cx)))
        })
        .when(view.member_target.is_some(), |d| {
            d.child(
                Button::new("toggle-member-list")
                    .ghost()
                    .small()
                    .icon(IconName::Users)
                    .selected(open)
                    .tooltip(if open {
                        l10n::tr("Hide members")
                    } else {
                        l10n::tr("Show members")
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_member_list(cx))),
            )
        })
}
