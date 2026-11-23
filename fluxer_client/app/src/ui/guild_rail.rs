//! Guild rail (`GuildsSidebar.tsx` + `GuildListIndicator.ts` + `GuildListDMItem.tsx`
//! + `GuildListItemPresentation.tsx` + `GuildsLayout.module.css`):
//! - Top `Direct messages` (`Home`) button and `Favorites` button, each wrapped in `rail_item`
//!   with the left white indicator bar (`40px` selected, `20px` hovered, `8px` unread) and
//!   hover morphing (`24px` circle -> `12px` rounded square, `theme.brand` background,
//!   `theme.on_fill` foreground).
//! - Horizontal separator (`rail-separator`, 32x2px).
//! - One `rail_item` per guild (`GuildIcon` image or initials, with left white indicator bar
//!   and `24px -> 12px` hover morphing).
//! - Bottom `Discover` (`Globe`) and `Join a community` (`Plus`) buttons with hover morphing.

use fluxer_client_core::state::Unread;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, Context, Div, ElementId, FontWeight, ObjectFit, Role, SharedString, Stateful, div, img, prelude::*, px,
};

use super::{badge, l10n};
use crate::client_view::{ClientView, RailHoverTarget};
use crate::icons::AppIcon;
use crate::theme::Theme;
use crate::view_model::Place;

/// Exact port of `resolveGuildListIndicatorHeight` from
/// `fluxer_app/src/features/app/components/layout/sidebar_nav/GuildListIndicator.ts`.
pub(crate) fn resolve_rail_indicator_height(selected: bool, hovered: bool, unread: bool) -> Option<f32> {
    if selected {
        Some(40.)
    } else if hovered {
        Some(20.)
    } else if unread {
        Some(8.)
    } else {
        None
    }
}

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
        .bg(theme.background_secondary)
        .overflow_y_scroll()
        .border_r_1()
        .border_color(theme.border);

    let home_selected = view.place == Place::Home && !view.favorites_selected;
    let home_hovered = view.hovered_rail_item == Some(RailHoverTarget::Home);
    let home_active = home_selected || home_hovered;
    let home_fg = if home_active {
        theme.on_fill
    } else {
        theme.foreground
    };
    rail = rail.child(rail_item(
        "home",
        home_selected,
        home_hovered,
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
            home_hovered,
            true,
            view.home_unread,
            theme,
        )
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            this.set_rail_hover(RailHoverTarget::Home, *hovered, cx);
        }))
        .on_click(cx.listener(|this, _, _, cx| this.select_place(Place::Home, cx))),
    ));

    // Official `FavoritesGuildButton` wrapped in the same `rail_item` with left white indicator bar
    let fav_selected = view.place == Place::Home && view.favorites_selected;
    let fav_hovered = view.hovered_rail_item == Some(RailHoverTarget::Favorites);
    let fav_active = fav_selected || fav_hovered;
    let fav_fg = if fav_active {
        theme.on_fill
    } else {
        theme.foreground
    };
    rail = rail
        .child(rail_item(
            "favorites",
            fav_selected,
            fav_hovered,
            Unread::default(),
            theme,
            rail_button_content(
                "rail-favorites-btn",
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(AppIcon::Star).small().text_color(fav_fg))
                    .into_any_element(),
                l10n::tr("Favorites"),
                fav_selected,
                fav_hovered,
                true,
                Unread::default(),
                theme,
            )
            .debug_selector(|| "rail-favorites-btn".to_owned())
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_rail_hover(RailHoverTarget::Favorites, *hovered, cx);
            }))
            .on_click(cx.listener(|this, _, _, cx| this.select_favorites(cx))),
        ))
        .child(
            div()
                .id("rail-separator")
                .debug_selector(|| "rail-separator".to_owned())
                .w(px(32.))
                .h(px(2.))
                .rounded(px(1.))
                .bg(theme.border),
        );

    for g in &view.guilds {
        let id = g.id;
        let selected = view.place == Place::Guild(id);
        let hovered = view.hovered_rail_item == Some(RailHoverTarget::Guild(id));
        let active = selected || hovered;
        let initials_fg = if active {
            theme.on_fill
        } else {
            theme.foreground
        };
        let has_image = g.icon_url.is_some();
        let content: AnyElement = match &g.icon_url {
            Some(url) => img(url.clone())
                .id(("guild-icon", id.0))
                .debug_selector(move || format!("guild-rail-icon-{}", id.0))
                .size_full()
                .rounded(px(if active { 12. } else { theme.sizes.rail_button / 2. }))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div().text_color(initials_fg).child(g.initials.clone()).into_any_element(),
        };
        rail = rail.child(rail_item(
            format!("guild-{}", id.0),
            selected,
            hovered,
            g.unread,
            theme,
            rail_button_content(
                ("guild", id.0),
                content,
                g.name.clone(),
                selected,
                hovered,
                !has_image,
                g.unread,
                theme,
            )
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_rail_hover(RailHoverTarget::Guild(id), *hovered, cx);
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.select_place(Place::Guild(id), cx))),
        ));
    }

    let discover_hovered = view.hovered_rail_item == Some(RailHoverTarget::Discover);
    let discover_fg = if discover_hovered {
        theme.on_fill
    } else {
        theme.foreground
    };
    let join_hovered = view.hovered_rail_item == Some(RailHoverTarget::Join);

    rail.child(
        div()
            .id("rail-discover-btn")
            .role(Role::Button)
            .aria_label(l10n::tr("Discover"))
            .debug_selector(|| "rail-discover-btn".to_owned())
            .size(px(theme.sizes.rail_button))
            .rounded(px(if discover_hovered {
                12.
            } else {
                theme.sizes.rail_button / 2.
            }))
            .border_1()
            .border_color(if discover_hovered { theme.brand } else { theme.border })
            .bg(if discover_hovered {
                theme.brand
            } else {
                theme.background_secondary
            })
            .text_color(discover_fg)
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_rail_hover(RailHoverTarget::Discover, *hovered, cx);
            }))
            .on_click(cx.listener(|this, _, window, cx| this.open_join_guild(window, cx)))
            .child(Icon::new(AppIcon::Compass).small().text_color(discover_fg)),
    )
    .child(
        div()
            .id("join-guild-wrapper")
            .debug_selector(|| "join-guild".to_owned())
            .size(px(theme.sizes.rail_button))
            .rounded(px(if join_hovered {
                12.
            } else {
                theme.sizes.rail_button / 2.
            }))
            .border_1()
            .border_color(if join_hovered {
                theme.foreground
            } else {
                theme.border
            })
            .bg(if join_hovered {
                theme.muted
            } else {
                theme.background_secondary
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_rail_hover(RailHoverTarget::Join, *hovered, cx);
            }))
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

/// Rail entry: the button with the left-edge white indicator bar
/// (`GuildListIndicator.ts`: 40px selected, 20px hovered, 8px unread).
fn rail_item(
    pill_key: impl Into<SharedString>,
    selected: bool,
    hovered: bool,
    unread: Unread,
    theme: &Theme,
    button: impl IntoElement,
) -> impl IntoElement {
    let pill = resolve_rail_indicator_height(selected, hovered, unread.unread);
    let button_size = theme.sizes.rail_button;
    let fg = theme.foreground;
    let pill_key_str = pill_key.into().to_string();
    div()
        .relative()
        .w_full()
        .flex()
        .justify_center()
        .child(button)
        .children(pill.map(|h| {
            let sel = format!("rail-pill-{pill_key_str}");
            div()
                .debug_selector(move || sel.clone())
                .absolute()
                .left_0()
                .top(px((button_size - h) / 2.))
                .w(px(4.))
                .h(px(h))
                .rounded_r(px(4.))
                .bg(fg)
        }))
}

fn rail_button_content(
    id: impl Into<ElementId>,
    content: AnyElement,
    aria_label: impl Into<SharedString>,
    selected: bool,
    hovered: bool,
    brand_when_active: bool,
    unread: Unread,
    theme: &Theme,
) -> Stateful<Div> {
    let size = theme.sizes.rail_button;
    let active = selected || hovered;
    let bg = if active {
        if brand_when_active {
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
        .cursor_pointer()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(if active { 12. } else { size / 2. }))
        .bg(bg)
        .text_color(if active { theme.on_fill } else { theme.foreground })
        .when(unread.unread, |d| d.font_weight(FontWeight::BOLD))
        .child(content)
        .when(unread.mention_count > 0, |d| {
            d.child(div().absolute().right(px(-4.)).bottom(px(-4.)).child(badge(unread.mention_count, theme)))
        })
}

#[cfg(test)]
mod tests {
    use super::resolve_rail_indicator_height;

    #[test]
    fn indicator_height_matches_official_guild_list_indicator_ts() {
        assert_eq!(resolve_rail_indicator_height(true, false, false), Some(40.));
        assert_eq!(resolve_rail_indicator_height(true, true, true), Some(40.));
        assert_eq!(resolve_rail_indicator_height(false, true, false), Some(20.));
        assert_eq!(resolve_rail_indicator_height(false, true, true), Some(20.));
        assert_eq!(resolve_rail_indicator_height(false, false, true), Some(8.));
        assert_eq!(resolve_rail_indicator_height(false, false, false), None);
    }
}
