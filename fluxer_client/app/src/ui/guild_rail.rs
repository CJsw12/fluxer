//! Guild rail: the DM button, then one button per guild.

use fluxer_client_core::state::Unread;
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{Context, Div, ElementId, FontWeight, Role, SharedString, Stateful, div, prelude::*, px};

use super::{badge, l10n};
use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::view_model::Place;

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let mut rail = div()
        .id("guild-rail")
        .role(Role::Navigation)
        .aria_label(l10n::tr("Communities"))
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .py_2()
        .w(px(theme.sizes.rail_width))
        .h_full()
        .flex_none()
        .overflow_y_scroll()
        .border_r_1()
        .border_color(theme.border);
    let home_selected = view.place == Place::Home;
    rail = rail.child(rail_item(
        home_selected,
        view.home_unread,
        theme,
        rail_button(
            "home",
            l10n::tr("DM").into(),
            l10n::tr("Direct messages"),
            home_selected,
            view.home_unread,
            theme,
        )
        .on_click(cx.listener(|this, _, _, cx| this.select_place(Place::Home, cx))),
    ));
    for g in &view.guilds {
        let id = g.id;
        let selected = view.place == Place::Guild(id);
        rail = rail.child(rail_item(
            selected,
            g.unread,
            theme,
            rail_button(("guild", id.0), g.initials.clone(), g.name.clone(), selected, g.unread, theme)
                .on_click(cx.listener(move |this, _, _, cx| this.select_place(Place::Guild(id), cx))),
        ));
    }
    rail.child(
        div().debug_selector(|| "join-guild".to_owned()).child(
            Button::new("join-guild")
                .ghost()
                .small()
                .icon(IconName::Plus)
                .tooltip(l10n::tr("Join a community"))
                .accessibility_label(l10n::tr("Join a community"))
                .on_click(cx.listener(|this, _, window, cx| this.open_join_guild(window, cx))),
        ),
    )
}

/// Rail entry: the button with the left-edge pill
/// (`GuildListItem`: 8px unread, 40px selected).
fn rail_item(selected: bool, unread: Unread, theme: &Theme, button: impl IntoElement) -> impl IntoElement {
    let pill = if selected {
        Some(40.)
    } else if unread.unread {
        Some(8.)
    } else {
        None
    };
    let button_size = theme.sizes.rail_button;
    let fg = theme.foreground;
    div().relative().w_full().flex().justify_center().child(button).children(pill.map(|h| {
        div().absolute().left_0().top(px((button_size - h) / 2.)).w(px(4.)).h(px(h)).rounded_r(px(4.)).bg(fg)
    }))
}

fn rail_button(
    id: impl Into<ElementId>,
    label: SharedString,
    aria_label: impl Into<SharedString>,
    selected: bool,
    unread: Unread,
    theme: &Theme,
) -> Stateful<Div> {
    let size = theme.sizes.rail_button;
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(aria_label)
        .aria_selected(selected)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(if selected { 12. } else { size / 2. }))
        .bg(if selected { theme.accent } else { theme.muted })
        .text_color(theme.foreground)
        .when(unread.unread, |d| d.font_weight(FontWeight::BOLD))
        .child(label)
        .when(unread.mention_count > 0, |d| {
            d.child(div().absolute().right(px(-4.)).bottom(px(-4.)).child(badge(unread.mention_count, theme)))
        })
}
