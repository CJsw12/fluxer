//! Explore communities directory (`DiscoveryLayout`, `DiscoverySidebar`,
//! `DiscoveryNavbar`, `DiscoveryPage`, and `DiscoveryGuildPreviewModal`).

use fluxer_client_core::rest::{DiscoveryCategory, DiscoveryGuild};
use fluxer_client_core::state::Snowflake;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, Hsla, ObjectFit, Role, SharedString, div, img,
    prelude::*, px, rgb, rgba,
};

use crate::client_view::ClientView;
use crate::format::initials;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::channel_popouts::track_trigger;
use crate::ui::{l10n, overlay};
use crate::view_model::{guild_banner_url, guild_icon_url};

pub const PREVIEW_CONTEXT: &str = "DiscoveryPreviewModal";
pub const PAGE_SIZE: u32 = 36;
const CARD_WIDTH: f32 = 270.;

const DEFAULT_CATEGORIES: &[(u32, &str)] = &[
    (0, "Gaming"),
    (1, "Music"),
    (2, "Entertainment"),
    (3, "Education"),
    (4, "Science & Technology"),
    (5, "Content Creator"),
    (6, "Anime & Manga"),
    (7, "Movies & TV"),
    (8, "Other"),
];

pub const DISCOVERY_LANGUAGES: &[(&str, &str)] = &[
    ("ar", "Arabic"),
    ("bg", "Bulgarian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("de", "German"),
    ("el", "Greek"),
    ("en-US", "English"),
    ("es-ES", "Spanish (Spain)"),
    ("es-419", "Spanish (Latin America)"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hr", "Croatian"),
    ("hu", "Hungarian"),
    ("id", "Indonesian"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("lt", "Lithuanian"),
    ("nl", "Dutch"),
    ("no", "Norwegian"),
    ("pl", "Polish"),
    ("pt-BR", "Portuguese (Brazil)"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sv-SE", "Swedish"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("vi", "Vietnamese"),
    ("zh-CN", "Chinese (Simplified)"),
    ("zh-TW", "Chinese (Traditional)"),
];

const BANNER_TINT_HEX: [u32; 10] = [
    0xa99adf, 0x93b7e0, 0x8fc9c6, 0x9ac9a4, 0xbfcb92, 0xdfc194, 0xe0a894, 0xdd9caa, 0xcd9ac9, 0x9ba4dd,
];

fn banner_tint_for_id(id: &str) -> Hsla {
    let idx = id
        .parse::<u128>()
        .map(|n| (n % BANNER_TINT_HEX.len() as u128) as usize)
        .unwrap_or(0);
    rgb(BANNER_TINT_HEX[idx]).into()
}

pub fn default_categories() -> Vec<DiscoveryCategory> {
    DEFAULT_CATEGORIES
        .iter()
        .map(|(id, name)| DiscoveryCategory {
            id: *id,
            name: (*name).to_owned(),
        })
        .collect()
}

pub struct ExploreState {
    pub categories: Vec<DiscoveryCategory>,
    pub categories_fetched: bool,
    pub guilds: Vec<DiscoveryGuild>,
    pub total: u64,
    pub selected_category: Option<u32>,
    pub selected_language: Option<String>,
    pub submitted_query: String,
    pub search_input: Entity<InputState>,
    pub loading: bool,
    pub has_loaded: bool,
    pub error: Option<String>,
    pub search_seq: u64,
    pub join_seq: u64,
}

impl ExploreState {
    pub fn new(search_input: Entity<InputState>) -> Self {
        Self {
            categories: default_categories(),
            categories_fetched: false,
            guilds: Vec::new(),
            total: 0,
            selected_category: None,
            selected_language: None,
            submitted_query: String::new(),
            search_input,
            loading: false,
            has_loaded: false,
            error: None,
            search_seq: 0,
            join_seq: 0,
        }
    }
}

pub struct DiscoveryPreviewModal {
    pub guild: DiscoveryGuild,
    pub joining: bool,
    pub join_seq: Option<u64>,
    pub error: Option<String>,
}

pub fn render_sidebar(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_bg: Hsla = theme.accent;
    let inactive_fg: Hsla = theme.text_tertiary_muted;
    let badge_bg: Hsla = theme.background_tertiary;
    let badge_fg: Hsla = theme.text_tertiary;

    let header = div()
        .h(px(theme.sizes.header_height))
        .w_full()
        .flex_none()
        .bg(theme.background_secondary)
        .border_b_1()
        .border_color(theme.subtle_fill)
        .flx("app.discovery-guild-header.div")
        .child(
            div()
                .size_full()
                .px(px(16.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .flx("app.discovery-guild-header.div--2")
                .child(
                    div()
                        .text_size(px(20.))
                        .line_height(px(28.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.foreground)
                        .flx("app.discovery-guild-header.header-title")
                        .child(l10n::tr("Explore")),
                ),
        );

    let communities_item = div()
        .id("explore-nav-communities")
        .role(Role::Button)
        .aria_label(l10n::tr("Communities"))
        .debug_selector(|| "explore-nav-communities".to_owned())
        .w_full()
        .min_h(px(44.))
        .p(px(8.))
        .rounded(px(6.))
        .bg(active_bg)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .text_size(px(16.))
        .line_height(px(24.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.foreground)
        .cursor_pointer()
        .on_click(cx.listener(|this, _, _, cx| this.explore_reset_to_all(cx)))
        .flx("discovery.discovery.discovery-sidebar.nav-item")
        .child(
            div()
                .flex_none()
                .size(px(20.))
                .flx("discovery.discovery.discovery-sidebar.nav-item-icon")
                .child(Icon::new(AppIcon::UsersThreeFill).with_size(px(20.)).text_color(theme.foreground)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .flx("discovery.discovery.discovery-sidebar.nav-item-label")
                .child(l10n::tr("Communities")),
        );

    let coming_soon_row = |id: &'static str, icon: AppIcon, label: String| {
        div()
            .id(id)
            .w_full()
            .min_h(px(44.))
            .p(px(8.))
            .rounded(px(6.))
            .bg(gpui_kit::transparent_black())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .text_size(px(16.))
            .line_height(px(24.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(inactive_fg)
            .flx("discovery.discovery.discovery-sidebar.nav-item")
            .child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .flx("discovery.discovery.discovery-sidebar.nav-item-icon")
                    .child(Icon::new(icon).with_size(px(20.)).text_color(inactive_fg)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .flx("discovery.discovery.discovery-sidebar.nav-item-label")
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .bg(badge_bg)
                    .text_color(badge_fg)
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .flx("discovery.discovery.discovery-sidebar.coming-soon-badge")
                    .child(l10n::tr("Coming soon")),
            )
    };

    let sidebar_body = div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .flx("discovery.discovery.discovery-sidebar.container")
        .child(
            div()
                .id("explore-sidebar-scroller")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .flx("discovery.discovery.discovery-sidebar.scroller")
                .child(
                    div()
                        .w_full()
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flx("discovery.discovery.discovery-sidebar.nav-list")
                        .child(communities_item)
                        .child(coming_soon_row("explore-nav-apps", AppIcon::AppWindowBold, l10n::tr("Apps")))
                        .child(coming_soon_row("explore-nav-themes", AppIcon::PaletteBold, l10n::tr("Themes"))),
                ),
        );

    div()
        .debug_selector(|| "explore-sidebar".to_owned())
        .flex()
        .flex_col()
        .w(px(view.effective_sidebar_width()))
        .h_full()
        .flex_none()
        .bg(theme.background_secondary)
        .child(header)
        .child(sidebar_body)
        .into_any_element()
}

pub fn render_page(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let dark = theme.background.l < 0.5;
    let active_tab_bg: Hsla = theme.accent;
    let inactive_tab_fg: Hsla = theme.text_tertiary;
    let pill_bg: Hsla = if dark { rgb(0x242229).into() } else { theme.background };
    let pill_border: Hsla = if dark { rgba(0xc9c5d326).into() } else { theme.border };
    let lang_fg: Hsla = if view.explore.selected_language.is_some() {
        theme.brand
    } else {
        theme.muted_foreground
    };
    let search_active = !view.explore.submitted_query.is_empty();
    let draft_non_empty = !view.explore.search_input.read(cx).value().is_empty();

    let left_surface = if search_active {
        div()
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .overflow_hidden()
            .flx("discovery.discovery.discovery-transition.div")
            .child(
                div()
                    .id("explore-search-back")
                    .role(Role::Button)
                    .aria_label(l10n::tr("Go back"))
                    .debug_selector(|| "explore-search-back".to_owned())
                    .size(px(32.))
                    .flex_none()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover_bg))
                    .on_click(cx.listener(|this, _, window, cx| this.clear_explore_search(window, cx)))
                    .flx("discovery.discovery.discovery-navbar.back-button")
                    .child(Icon::new(AppIcon::ArrowLeftBold).with_size(px(18.)).text_color(theme.foreground)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .flx("discovery.discovery.discovery-navbar.search-title")
                    .child(l10n::tr_args(
                        "Search results for \"{query}\"",
                        &[("query", view.explore.submitted_query.as_str())],
                    )),
            )
    } else {
        let mut tabs = div()
            .id("explore-category-tabs")
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .overflow_x_scroll()
            .flx("discovery.discovery.discovery-navbar.category-tabs");

        tabs = tabs.child(render_category_tab(
            None,
            l10n::tr("All"),
            view.explore.selected_category.is_none(),
            active_tab_bg,
            inactive_tab_fg,
            theme,
            cx,
        ));
        for cat in &view.explore.categories {
            tabs = tabs.child(render_category_tab(
                Some(cat.id),
                cat.name.clone(),
                view.explore.selected_category == Some(cat.id),
                active_tab_bg,
                inactive_tab_fg,
                theme,
                cx,
            ));
        }
        div()
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .overflow_hidden()
            .flx("discovery.discovery.discovery-transition.div")
            .child(tabs)
    };

    let navbar = div()
        .h(px(theme.sizes.header_height))
        .flex_none()
        .w_full()
        .px(px(16.))
        .gap(px(16.))
        .bg(theme.background_secondary_lighter)
        .border_b_1()
        .border_color(theme.subtle_fill)
        .flex()
        .flex_row()
        .items_center()
        .flx("discovery.discovery.discovery-navbar.navbar")
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(32.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .overflow_hidden()
                .flx("discovery.discovery.discovery-navbar.left-section")
                .child(left_surface),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .flx("discovery.discovery.discovery-navbar.search-area")
                .child(
                    div()
                        .id("explore-search-form")
                        .w(px(theme.sizes.search_bar_width))
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .flx("discovery.discovery.discovery-navbar.search-form.submit")
                        .child(
                            div()
                                .size_full()
                                .px(px(10.))
                                .rounded(px(12.))
                                .bg(pill_bg)
                                .border_1()
                                .border_color(pill_border)
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .flx("discovery.discovery.discovery-navbar.div")
                                .child(
                                    div()
                                        .flex_none()
                                        .size(px(16.))
                                        .flx("discovery.discovery.discovery-navbar.search-icon")
                                        .child(
                                            Icon::new(AppIcon::MagnifyingGlass)
                                                .with_size(px(16.))
                                                .text_color(inactive_tab_fg),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .h(px(34.))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .line_height(px(22.))
                                        .text_color(theme.foreground)
                                        .debug_selector(|| "explore-search-input".to_owned())
                                        .flx("discovery.discovery.discovery-navbar.search-input.set-draft-query")
                                        .child(
                                            Input::new(&view.explore.search_input)
                                                .appearance(false)
                                                .text_size(px(14.))
                                                .line_height(px(22.)),
                                        ),
                                )
                                .when(draft_non_empty || search_active, |d| {
                                    d.child(
                                        div().debug_selector(|| "explore-search-clear".to_owned()).child(
                                            Button::new("explore-search-clear")
                                                .ghost()
                                                .small()
                                                .icon(AppIcon::X)
                                                .accessibility_label(l10n::tr("Clear search"))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.clear_explore_search(window, cx);
                                                })),
                                        ),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .id("explore-language-btn")
                        .role(Role::Button)
                        .aria_label(l10n::tr("Filter by language"))
                        .debug_selector(|| "explore-language-btn".to_owned())
                        .relative()
                        .size(px(32.))
                        .rounded_full()
                        .bg(gpui_kit::transparent_black())
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.hover_bg))
                        .on_click(cx.listener(|this, ev: &gpui_kit::ClickEvent, window, cx| {
                            this.toggle_explore_language_popout(ev.position(), window, cx);
                        }))
                        .flx("discovery.discovery.discovery-language-button.channel-header-icon.click")
                        .child(track_trigger("explore-language-btn"))
                        .child(Icon::new(AppIcon::Translate).with_size(px(20.)).text_color(lang_fg)),
                ),
        );

    let media = view.backend.media();
    let results_inner: AnyElement = if view.explore.loading && view.explore.guilds.is_empty() {
        div()
            .w_full()
            .py(px(48.))
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(inactive_tab_fg)
            .child(l10n::tr("Loading communities…"))
            .into_any_element()
    } else if !view.explore.guilds.is_empty() {
        let mut grid = div()
            .debug_selector(|| "explore-grid".to_owned())
            .w_full()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(16.))
            .flx("discovery.discovery.discovery-page.virtual-grid");
        for (idx, guild) in view.explore.guilds.iter().enumerate() {
            grid = grid.child(render_guild_card(idx, guild, media, dark, theme, cx));
        }
        let has_more = (view.explore.guilds.len() as u64) < view.explore.total;
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(grid)
            .when(has_more, |d| {
                d.child(
                    div().w_full().py(px(8.)).flex().justify_center().child(
                        div().debug_selector(|| "explore-load-more".to_owned()).child(
                            Button::new("explore-load-more")
                                .ghost()
                                .small()
                                .disabled(view.explore.loading)
                                .label(if view.explore.loading {
                                    l10n::tr("Loading…")
                                } else {
                                    l10n::tr("Load more")
                                })
                                .on_click(cx.listener(|this, _, _, cx| this.explore_load_more(cx))),
                        ),
                    ),
                )
            })
            .into_any_element()
    } else {
        render_empty_state(view, inactive_tab_fg, theme, cx)
    };

    let body = div()
        .id("explore-scroller")
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_y_scroll()
        .flx("discovery.discovery.discovery-page.scroller")
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .flx("discovery.discovery.discovery-page.scroll-body")
                .child(
                    div()
                        .w_full()
                        .pt(px(20.))
                        .px(px(16.))
                        .pb(px(32.))
                        .flx("discovery.discovery.discovery-page.content")
                        .child(
                            div()
                                .w_full()
                                .relative()
                                .flx("discovery.discovery.discovery-page.search-layout")
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(-1.))
                                        .top(px(-1.))
                                        .w(px(1.))
                                        .h(px(1.))
                                        .overflow_hidden()
                                        .flx("discovery.discovery.discovery-page.sr-only"),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .flx("discovery.discovery.discovery-page.results-column")
                                        .child(
                                            div()
                                                .w_full()
                                                .flx("discovery.discovery.discovery-transition.div")
                                                .child(results_inner),
                                        ),
                                ),
                        ),
                ),
        );

    div()
        .id("explore-page")
        .debug_selector(|| "explore-page".to_owned())
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(theme.background_secondary)
        .flx("app.discovery-layout.guild-main-content")
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .h_full()
                .bg(theme.background_secondary_lighter)
                .flx("discovery.discovery.discovery-page.container")
                .child(navbar)
                .child(body),
        )
        .into_any_element()
}

fn render_category_tab(
    category_id: Option<u32>,
    label: String,
    active: bool,
    active_bg: Hsla,
    inactive_fg: Hsla,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> impl IntoElement {
    let key = match category_id {
        Some(id) => id.to_string(),
        None => "all".to_owned(),
    };
    let el_id = SharedString::from(format!("explore-cat-{key}"));
    let dbg = el_id.to_string();
    let fg = if active { theme.foreground } else { inactive_fg };
    let bg = if active {
        active_bg
    } else {
        gpui_kit::transparent_black()
    };
    div()
        .id(el_id)
        .role(Role::Tab)
        .aria_selected(active)
        .debug_selector(move || dbg.clone())
        .h(px(32.))
        .px(px(12.))
        .rounded(px(6.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(14.))
        .line_height(px(21.))
        .font_weight(FontWeight::MEDIUM)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.bg(theme.hover_bg).text_color(theme.foreground))
        .on_click(cx.listener(move |this, _, _, cx| this.explore_select_category(category_id, cx)))
        .flx("discovery.discovery.discovery-navbar.category-tab.category-select.button")
        .child(label)
}

fn render_guild_card(
    idx: usize,
    guild: &DiscoveryGuild,
    media: Option<&str>,
    dark: bool,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> impl IntoElement {
    let parsed_id = Snowflake::parse(&guild.id);
    let icon_url = media.and_then(|m| parsed_id.and_then(|gid| guild_icon_url(m, gid, guild.icon.as_deref())));
    let banner_url = media.and_then(|m| parsed_id.and_then(|gid| guild_banner_url(m, gid, guild.banner.as_deref())));
    let card_border: Hsla = if dark { rgba(0xc9c5d326).into() } else { theme.border };
    let muted_fg: Hsla = theme.text_tertiary;
    let desc_fg: Hsla = theme.muted_foreground;
    let tag_bg: Hsla = theme.background_tertiary;
    let gid_dbg = guild.id.clone();
    let guild_for_click = guild.clone();

    let banner = div()
        .w_full()
        .h(px(120.))
        .flex_none()
        .relative()
        .overflow_hidden()
        .bg(banner_tint_for_id(&guild.id))
        .flx("discovery.discovery.discovery-guild-card.banner")
        .when_some(banner_url, |d, url| {
            d.child(
                img(url)
                    .id(("explore-banner", idx))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
        });

    let icon_box = div()
        .absolute()
        .left(px(16.))
        .top(px(92.))
        .size(px(56.))
        .rounded(px(14.))
        .border_3()
        .border_color(theme.background_secondary)
        .bg(theme.guild_list_foreground)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center()
        .flx("discovery.discovery.discovery-guild-card.icon")
        .child(match icon_url {
            Some(url) => img(url)
                .id(("explore-icon", idx))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .text_base()
                .font_weight(FontWeight::BOLD)
                .text_color(theme.foreground)
                .child(initials(&guild.name))
                .into_any_element(),
        });

    let description = guild
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| l10n::tr("No description."));

    let tags_row = (!guild.custom_tags.is_empty()).then(|| {
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(4.))
            .children(guild.custom_tags.iter().take(3).cloned().map(|t| {
                div()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .bg(tag_bg)
                    .text_size(px(11.))
                    .text_color(muted_fg)
                    .child(t)
            }))
    });

    let member_label = if guild.member_count == 1 {
        format!("1 {}", l10n::tr("member"))
    } else {
        format!("{} {}", guild.member_count, l10n::tr("members"))
    };

    div()
        .id(("explore-guild-card", idx))
        .debug_selector(move || format!("explore-card-{gid_dbg}"))
        .relative()
        .w(px(CARD_WIDTH))
        .min_h(px(280.))
        .rounded(px(12.))
        .border_1()
        .border_color(card_border)
        .bg(theme.background_secondary)
        .overflow_hidden()
        .flex()
        .flex_col()
        .cursor_pointer()
        .hover(|s| s.bg(theme.hover_bg))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_discovery_preview(guild_for_click.clone(), window, cx);
        }))
        .flx("discovery.discovery.discovery-guild-card.card-button.activate")
        .child(banner)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pt(px(36.))
                .px(px(16.))
                .pb(px(12.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .flx("discovery.discovery.discovery-guild-card.body")
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .text_color(theme.foreground)
                        .flx("discovery.discovery.discovery-guild-card.name-text")
                        .child(guild.name.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h(px(42.))
                        .max_h(px(64.))
                        .overflow_hidden()
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .text_color(desc_fg)
                        .flx("discovery.discovery.discovery-guild-card.description")
                        .child(description),
                )
                .children(tags_row),
        )
        .child(
            div()
                .flex_none()
                .px(px(16.))
                .py(px(12.))
                .border_t_1()
                .border_color(card_border)
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .flx("discovery.discovery.discovery-guild-card.footer")
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .child(div().size(px(8.)).rounded_full().bg(theme.status_online))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted_fg)
                                .child(format!("{} {}", guild.online_count, l10n::tr("online"))),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .child(div().size(px(8.)).rounded_full().bg(muted_fg))
                        .child(div().text_xs().text_color(muted_fg).child(member_label)),
                ),
        )
        .child(icon_box)
}

fn render_empty_state(
    view: &ClientView,
    desc_fg: Hsla,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let icon_fg: Hsla = theme.muted_foreground;
    let (icon, heading, desc, has_retry) = if view.explore.error.is_some() {
        (
            AppIcon::WarningCircle,
            l10n::tr("Couldn't load communities"),
            l10n::tr("Check your connection, then try again."),
            true,
        )
    } else if !view.explore.submitted_query.is_empty() {
        (
            AppIcon::MagnifyingGlass,
            l10n::tr("No communities match."),
            l10n::tr("Try another search term, or pick a different category or language."),
            false,
        )
    } else {
        (
            AppIcon::Compass,
            l10n::tr("Nothing to explore yet"),
            l10n::tr("No communities are listed right now. Check back a little later."),
            false,
        )
    };

    div()
        .w_full()
        .py(px(48.))
        .px(px(24.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .flx("discovery.discovery.discovery-empty-state.container")
        .child(
            div()
                .flex_none()
                .size(px(56.))
                .flex()
                .items_center()
                .justify_center()
                .flx("discovery.discovery.discovery-empty-state.icon")
                .child(Icon::new(icon).with_size(px(56.)).text_color(icon_fg)),
        )
        .child(
            div()
                .max_w(px(416.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .flx("discovery.discovery.discovery-empty-state.text")
                .child(
                    div()
                        .text_size(px(18.))
                        .line_height(px(25.2))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .flx("discovery.discovery.discovery-empty-state.heading")
                        .child(heading),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(desc_fg)
                        .flx("discovery.discovery.discovery-empty-state.description")
                        .child(desc),
                ),
        )
        .when(has_retry, |d| {
            d.child(
                div()
                    .debug_selector(|| "explore-retry-btn".to_owned())
                    .flx("discovery.discovery.discovery-empty-state.button.retry")
                    .child(
                        Button::new("explore-retry-btn")
                            .primary()
                            .small()
                            .label(l10n::tr("Try again"))
                            .on_click(cx.listener(|this, _, _, cx| this.explore_retry(cx))),
                    ),
            )
        })
        .into_any_element()
}

pub fn render_preview_modal(
    view: &ClientView,
    modal: &DiscoveryPreviewModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let guild = &modal.guild;
    let parsed_id = Snowflake::parse(&guild.id);
    let already_member = parsed_id.is_some_and(|gid| view.backend.state().guilds.contains_key(&gid));
    let icon_url = view
        .backend
        .media()
        .and_then(|m| parsed_id.and_then(|gid| guild_icon_url(m, gid, guild.icon.as_deref())));
    let description = guild
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| l10n::tr("No description."));

    let cta_label = if already_member {
        l10n::tr("Open community")
    } else if modal.joining {
        l10n::tr("Joining…")
    } else {
        l10n::tr("Join community")
    };

    let content = div()
        .id("discovery-preview-modal")
        .key_context(PREVIEW_CONTEXT)
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .size(px(56.))
                        .flex_none()
                        .rounded(px(14.))
                        .bg(theme.guild_list_foreground)
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(match icon_url {
                            Some(url) => img(url)
                                .id("explore-preview-icon")
                                .size_full()
                                .object_fit(ObjectFit::Cover)
                                .into_any_element(),
                            None => div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.foreground)
                                .child(initials(&guild.name))
                                .into_any_element(),
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("Public community")),
                        )
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .truncate()
                                .child(guild.name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "{} {} · {} {}",
                                    guild.online_count,
                                    l10n::tr("online"),
                                    guild.member_count,
                                    l10n::tr("members")
                                )),
                        ),
                ),
        )
        .child(div().text_sm().text_color(theme.muted_foreground).child(description))
        .when(!guild.custom_tags.is_empty(), |d| {
            d.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1p5()
                    .children(guild.custom_tags.iter().cloned().map(|tag| {
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(px(4.))
                            .bg(theme.background)
                            .border_1()
                            .border_color(theme.border)
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tag)
                    })),
            )
        })
        .when_some(modal.error.clone(), |d, err| {
            d.child(div().text_xs().text_color(theme.danger).child(err))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("explore-preview-cancel")
                        .ghost()
                        .small()
                        .disabled(modal.joining)
                        .label(l10n::tr("Close"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                )
                .child(
                    div()
                        .debug_selector(|| "explore-preview-join-btn".to_owned())
                        .flx("discovery.discovery-guild-preview-modal.button.join")
                        .child(
                            Button::new("explore-preview-join-btn")
                                .primary()
                                .small()
                                .disabled(modal.joining)
                                .label(cta_label)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_join_discovery_guild(window, cx);
                                })),
                        ),
                ),
        );

    let weak = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = weak.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
