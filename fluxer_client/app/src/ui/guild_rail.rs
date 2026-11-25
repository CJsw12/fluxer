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
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Hsla, Window, rgb, rgba, ElementId, FontWeight, ObjectFit, Role, SharedString, Stateful, div, img, prelude::*, px,
};

use super::{badge, l10n, rail_tooltip};
use crate::client_view::{ClientView, RailHoverTarget};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
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

const RAIL_ICON: f32 = 28.;
/// 30% of the 44px guild icon.
const GUILD_RADIUS: f32 = 13.2;

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let mut rail = div()
        .id("guild-rail")
        .role(Role::Navigation)
        .aria_label(l10n::tr("Communities"))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.))
        .py(px(4.))
        .w(px(theme.sizes.rail_width))
        .h_full()
        .flex_none()
        .bg(theme.background_secondary)
        .overflow_y_scroll()
        .flx("app.guilds-layout.guild-list.guild-list-scroller-wrapper");

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
        48.,
        "app.sidebar-nav.fluxer-button.fluxer-button.select",
        "app.sidebar-nav.fluxer-button.guild-indicator",
        "app.sidebar-nav.fluxer-button.guild-indicator-bar",
        cx.listener(|this, _, _, cx| this.select_place(Place::Home, cx)),
        cx.listener(|this, hovered: &bool, _, cx| {
            this.set_rail_hover(RailHoverTarget::Home, *hovered, cx);
        }),
        rail_button_content(
            "home",
            div()
                .debug_selector(|| "rail-home-icon".to_owned())
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex_none()
                        .size(px(RAIL_ICON))
                        .flx("app.sidebar-nav.fluxer-button.message-bubble-icon")
                        .child(Icon::new(AppIcon::ChatCircleFill).with_size(px(RAIL_ICON)).text_color(home_fg)),
                )
                .into_any_element(),
            l10n::tr("Direct messages"),
            home_selected,
            home_hovered,
            true,
            view.home_unread,
            theme,
            theme.background_tertiary,
        )
        .flx("app.sidebar-nav.fluxer-button.fluxer-button-icon"),
        home_hovered.then(|| rail_tooltip::text(l10n::tr("Direct messages"), theme, rail_tooltip::TOOLTIP_LEFT)),
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
            48.,
            "app.sidebar-nav.favorites-button.fluxer-button.select",
            "app.sidebar-nav.favorites-button.guild-indicator",
            "app.sidebar-nav.favorites-button.guild-indicator-bar",
            cx.listener(|this, _, _, cx| this.select_favorites(cx)),
            cx.listener(|this, hovered: &bool, _, cx| {
                this.set_rail_hover(RailHoverTarget::Favorites, *hovered, cx);
            }),
            rail_button_content(
                "rail-favorites-btn",
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .flex_none()
                            .size(px(RAIL_ICON))
                            .flx("app.sidebar-nav.favorites-button.favorites-icon")
                            .child(Icon::new(AppIcon::Star).with_size(px(RAIL_ICON)).text_color(fav_fg)),
                    )
                    .into_any_element(),
                l10n::tr("Favorites"),
                fav_selected,
                fav_hovered,
                true,
                Unread::default(),
                theme,
                theme.background_tertiary,
            )
            .debug_selector(|| "rail-favorites-btn".to_owned())
            .flx("app.sidebar-nav.favorites-button.fluxer-button-icon"),
            fav_hovered.then(|| rail_tooltip::text(l10n::tr("Favorites"), theme, rail_tooltip::TOOLTIP_LEFT)),
        ))
        .child(
            div()
                .id("rail-separator")
                .debug_selector(|| "rail-separator".to_owned())
                .w(px(32.))
                .h(px(2.))
                .mt(px(3.)).mb(px(-1.))
                .flex_none()
                .rounded(px(1.))
                .bg(theme.hover_bg)
                .flx("app.guilds-layout.guild-list-divider.guild-divider"),
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
        let in_voice = g.has_voice_activity;
        let content: AnyElement = match &g.icon_url {
            Some(url) => img(url.clone())
                .id(("guild-icon", id.0))
                .debug_selector(move || format!("guild-rail-icon-{}", id.0))
                .size_full()
                .rounded(px(if active { GUILD_RADIUS } else { theme.sizes.rail_button / 2. }))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => {
                // `GuildAcronymUtils.ts`: font sizes by acronym length, scaled from a 48px icon.
                const SIZES: [f32; 6] = [18., 18., 16., 14., 12., 10.];
                let len = g.initials.chars().count().clamp(1, SIZES.len());
                let font = (SIZES[len - 1] * theme.sizes.rail_button / 48.).floor().max(8.);
                div()
                    .text_size(px(font))
                    .line_height(px(font))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(initials_fg)
                    .flx("guild.guild-acronym.text")
                    .child(g.initials.clone())
                    .into_any_element()
            }
        };
        rail = rail.child(rail_item(
            format!("guild-{}", id.0),
            selected,
            hovered,
            g.unread,
            theme,
            54.,
            "app.sidebar-nav.guild-list-item-presentation.guild-list-item.click",
            "app.sidebar-nav.guild-list-item-presentation.guild-indicator",
            "app.sidebar-nav.guild-list-item-presentation.guild-indicator-bar",
            cx.listener(move |this, _, _, cx| this.select_place(Place::Guild(id), cx)),
            cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_rail_hover(RailHoverTarget::Guild(id), *hovered, cx);
            }),
            rail_button_content(
                ("guild", id.0),
                content,
                g.name.clone(),
                selected,
                hovered,
                !has_image,
                g.unread,
                theme,
                theme.guild_list_foreground,
            )
            .flx("app.sidebar-nav.guild-list-item-presentation.guild-icon")
            .when(in_voice, |d| d.child(rail_tooltip::voice_badge(theme))),
            hovered.then(|| rail_tooltip::guild(g.name.clone(), g.counts, theme)),
        ));
    }

    let discover_selected = view.place == Place::Explore;
    let discover_hovered = view.hovered_rail_item == Some(RailHoverTarget::Discover);
    let discover_active = discover_selected || discover_hovered;
    let discover_fg = if discover_selected { theme.on_fill } else { theme.foreground };
    let join_hovered = view.hovered_rail_item == Some(RailHoverTarget::Join);

    rail.child(slot(2., 68., 44., "app.sidebar-nav.discovery-button.button.click",
        div()
            .id("rail-discover-btn")
            .role(Role::Button)
            .aria_label(l10n::tr("Discover"))
            .aria_selected(discover_selected)
            .debug_selector(|| "rail-discover-btn".to_owned())
            .size(px(theme.sizes.rail_button))
            .rounded(px(if discover_active {
                GUILD_RADIUS
            } else {
                theme.sizes.rail_button / 2.
            }))
            .border_2()
            .when(!discover_selected, |d| d.border_dashed())
            .border_color(if discover_active { Hsla::from(rgb(0xf4f4f6)) } else { Hsla::from(rgba(0xc9c5d326)) })
            .bg(if discover_selected { theme.brand } else { gpui_kit::transparent_black() })
            .text_color(discover_fg)
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_rail_hover(RailHoverTarget::Discover, *hovered, cx);
            }))
            .on_click(cx.listener(|this, _, _, cx| this.open_explore(cx)))
            .flx("app.sidebar-nav.discovery-button.div--2")
            .child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .flx("app.sidebar-nav.discovery-button.icon-text")
                    .child(Icon::new(AppIcon::Compass).with_size(px(20.)).text_color(discover_fg)),
            ),
        discover_hovered.then(|| rail_tooltip::text(l10n::tr("Explore communities"), theme, 76.)),
    ))
    .child(slot(4., 68., 44., "app.sidebar-nav.add-guild-button.button.add-guild",
        div()
            .id("join-guild-wrapper")
            .role(Role::Button)
            .aria_label(l10n::tr("Join a community"))
            .debug_selector(|| "join-guild".to_owned())
            .size(px(theme.sizes.rail_button))
            .rounded(px(if join_hovered {
                GUILD_RADIUS
            } else {
                theme.sizes.rail_button / 2.
            }))
            .border_2()
            .border_dashed()
            .border_color(if join_hovered { Hsla::from(rgb(0xf4f4f6)) } else { Hsla::from(rgba(0xc9c5d326)) })
            .bg(gpui_kit::transparent_black())
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_rail_hover(RailHoverTarget::Join, *hovered, cx);
            }))
            .on_click(cx.listener(|this, _, window, cx| this.open_join_guild(window, cx)))
            .flx("app.sidebar-nav.add-guild-button.div--2")
            .child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .flx("app.sidebar-nav.add-guild-button.icon-text")
                    .child(Icon::new(AppIcon::Plus).with_size(px(20.)).text_color(if join_hovered { theme.foreground } else { theme.muted_foreground })),
            ),
        join_hovered.then(|| rail_tooltip::create_or_join(l10n::tr("Create or join a community"), theme, 76.)),
    ))
}

/// Full-width slot that carries the official click-area tag; hover and click live here.
fn slot(mt: f32, w: f32, h: f32, flx: &'static str, child: impl IntoElement, tooltip: Option<AnyElement>) -> impl IntoElement {
    div()
        .relative()
        .w(px(w))
        .h(px(h))
        .flex_none()
        .mt(px(mt))
        .flex()
        .items_center()
        .justify_center()
        .child(child)
        .children(tooltip)
        .flx(flx)
}

/// Rail entry: the button with the left-edge white indicator bar
/// (`GuildListIndicator.ts`: 40px selected, 20px hovered, 8px unread). The slot
/// wrapper spans the rail width and owns the click/hover handlers and the official tag.
#[allow(clippy::too_many_arguments)]
fn rail_item(
    pill_key: impl Into<SharedString>,
    selected: bool,
    hovered: bool,
    unread: Unread,
    theme: &Theme,
    slot_height: f32,
    flx: &'static str,
    indicator_wrap_flx: &'static str,
    indicator_flx: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
    button: impl IntoElement,
    tooltip: Option<AnyElement>,
) -> impl IntoElement {
    let pill = resolve_rail_indicator_height(selected, hovered, unread.unread);
    let fg = theme.foreground;
    let pill_key_str: String = pill_key.into().to_string();
    div()
        .id(SharedString::from(format!("rail-slot-{pill_key_str}")))
        .relative()
        .w_full()
        .h(px(slot_height))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_hover(on_hover)
        .on_click(on_click)
        .child(button)
        .children(tooltip)
        .children(pill.map(|h| {
            let sel = format!("rail-pill-{pill_key_str}");
            div()
                .absolute()
                .left(px(-2.))
                .top(px((slot_height - 44.) / 2.))
                .w(px(8.))
                .h(px(44.))
                .flx(indicator_wrap_flx)
                .child(
                    div()
                        .debug_selector(move || sel.clone())
                        .absolute()
                        .left(px(1.))
                        .top(px((44. - h) / 2.))
                        .w(px(6.))
                        .h(px(h))
                        .rounded_r(px(6.))
                        .bg(fg)
                        .flx(indicator_flx),
                )
        }))
        .flx(flx)
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
    inactive_bg: Hsla,
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
        inactive_bg
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
        .rounded(px(if active { GUILD_RADIUS } else { size / 2. }))
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
