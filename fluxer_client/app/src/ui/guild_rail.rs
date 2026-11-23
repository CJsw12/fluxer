//! Guild rail: the DM button (`HomeGuildButton`), then one button per guild (`GuildIcon`).

use fluxer_client_core::state::Unread;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, Context, Div, ElementId, FontWeight, ObjectFit, Role, SharedString, Stateful, div, img, prelude::*, px,
};

use super::{badge, l10n};
use crate::client_view::ClientView;
use crate::icons::AppIcon;
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
    let home_fg = if home_selected {
        theme.on_fill
    } else {
        theme.foreground
    };
    rail = rail.child(rail_item(
        home_selected,
        view.home_unread,
        theme,
        rail_button_content(
            "home",
            div()
                .debug_selector(|| "rail-home-icon".to_owned())
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(AppIcon::ChatsCircle).text_color(home_fg))
                .into_any_element(),
            l10n::tr("Direct messages"),
            home_selected,
            true,
            view.home_unread,
            theme,
        )
        .on_click(cx.listener(|this, _, _, cx| this.select_place(Place::Home, cx))),
    ));
    // Official `FavoritesGuildButton` (Star icon) + horizontal `.guildSeparator`
    rail = rail
        .child(
            div()
                .id("rail-favorites-btn")
                .role(Role::Button)
                .aria_label(l10n::tr("Favorites"))
                .debug_selector(|| "rail-favorites-btn".to_owned())
                .size(px(theme.sizes.rail_button))
                .rounded_full()
                .bg(theme.muted)
                .text_color(theme.foreground)
                .cursor_pointer()
                .hover(|s| s.rounded(px(12.)).bg(theme.accent))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| this.select_place(Place::Home, cx)))
                .child(Icon::new(AppIcon::PushPin).small().text_color(theme.foreground)),
        )
        .child(
            div()
                .id("rail-separator")
                .debug_selector(|| "rail-separator".to_owned())
                .w(px(32.))
                .h(px(1.))
                .bg(theme.border),
        );
    for g in &view.guilds {
        let id = g.id;
        let selected = view.place == Place::Guild(id);
        let initials_fg = if selected {
            theme.on_fill
        } else {
            theme.foreground
        };
        let content: AnyElement = match &g.icon_url {
            Some(url) => img(url.clone())
                .id(("guild-icon", id.0))
                .debug_selector(move || format!("guild-rail-icon-{}", id.0))
                .size_full()
                .rounded(px(if selected { 12. } else { theme.sizes.rail_button / 2. }))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div().text_color(initials_fg).child(g.initials.clone()).into_any_element(),
        };
        rail = rail.child(rail_item(
            selected,
            g.unread,
            theme,
            rail_button_content(
                ("guild", id.0),
                content,
                g.name.clone(),
                selected,
                true,
                g.unread,
                theme,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.select_place(Place::Guild(id), cx))),
        ));
    }
    rail.child(
        div()
            .id("rail-discover-btn")
            .role(Role::Button)
            .aria_label(l10n::tr("Discover"))
            .debug_selector(|| "rail-discover-btn".to_owned())
            .size(px(theme.sizes.rail_button))
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background_secondary)
            .text_color(theme.foreground)
            .cursor_pointer()
            .hover(|s| s.rounded(px(12.)).bg(theme.muted))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, window, cx| this.open_join_guild(window, cx)))
            .child(Icon::new(AppIcon::Globe).small().text_color(theme.foreground)),
    )
    .child(
        div()
            .debug_selector(|| "join-guild".to_owned())
            .size(px(theme.sizes.rail_button))
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background_secondary)
            .flex()
            .items_center()
            .justify_center()
            .child(
                Button::new("join-guild")
                    .ghost()
                    .small()
                    .icon(AppIcon::Plus)
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

fn rail_button_content(
    id: impl Into<ElementId>,
    content: AnyElement,
    aria_label: impl Into<SharedString>,
    selected: bool,
    brand_when_selected: bool,
    unread: Unread,
    theme: &Theme,
) -> Stateful<Div> {
    let size = theme.sizes.rail_button;
    let bg = if selected {
        if brand_when_selected {
            theme.brand
        } else {
            theme.accent
        }
    } else {
        theme.muted
    };
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
        .bg(bg)
        .text_color(theme.foreground)
        .when(unread.unread, |d| d.font_weight(FontWeight::BOLD))
        .child(content)
        .when(unread.mention_count > 0, |d| {
            d.child(div().absolute().right(px(-4.)).bottom(px(-4.)).child(badge(unread.mention_count, theme)))
        })
}
