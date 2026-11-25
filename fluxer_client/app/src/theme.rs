//! Colors and sizes of the client layout in one place.
//!
//! Colors are ported from the official app's generated color system
//! (`fluxer_app/scripts/GenerateColorSystem.ts` with the families and scales
//! of `fluxer_app/src/features/theme/utils/ColorSystemPalette.ts`, saturation
//! factor 1). Every surface/text token is an HSL value derived from a family
//! (hue, saturation) and a lightness scale, exactly as the CSS generator does.
//!
//! The official theme ids are `dark`, `dark_legacy`, `coal`, `light` and
//! `system` (`packages/constants/src/UserConstants.ts`). Here `Darker` is the
//! official `dark_legacy` palette and `Black` is the official `coal` palette.

use gpui_kit::component::{Theme as KitTheme, ThemeMode};
use gpui_kit::{App, Global, Hsla, Window, WindowAppearance, hsla, rgb};

/// App-wide selected theme kind (`Theme::current` reads this global).
#[derive(Debug, Clone, Copy)]
pub struct Selected(pub ThemeKind);

impl Global for Selected {}

/// The active [`ThemeKind`], falling back to `FLUXER_THEME` / Dark.
pub fn kind(cx: &App) -> ThemeKind {
    cx.try_global::<Selected>().map_or_else(ThemeKind::from_env, |s| s.0)
}

/// Switches the active theme kind at runtime and redraws windows.
pub fn set_kind(kind: ThemeKind, cx: &mut App) {
    cx.set_global(Selected(kind));
    cx.refresh_windows();
}

pub const MIN_FONT_SIZE: u8 = 12;
pub const MAX_FONT_SIZE: u8 = 24;
pub const DEFAULT_FONT_SIZE: u8 = 14;
pub const MIN_UI_ZOOM: u16 = 75;
pub const MAX_UI_ZOOM: u16 = 150;
pub const DEFAULT_UI_ZOOM: u16 = 100;

/// User display preferences affecting typography, spacing density, and UI zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayPrefs {
    pub font_size: u8,
    pub ui_zoom: u16,
    pub compact: bool,
}

impl Default for DisplayPrefs {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
            ui_zoom: DEFAULT_UI_ZOOM,
            compact: false,
        }
    }
}

impl DisplayPrefs {
    pub fn clamped(self) -> Self {
        Self {
            font_size: self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE),
            ui_zoom: self.ui_zoom.clamp(MIN_UI_ZOOM, MAX_UI_ZOOM),
            compact: self.compact,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayGlobal(pub DisplayPrefs);

impl Global for DisplayGlobal {}

pub fn display(cx: &App) -> DisplayPrefs {
    cx.try_global::<DisplayGlobal>().map_or_else(DisplayPrefs::default, |g| g.0)
}

pub fn set_display(prefs: DisplayPrefs, cx: &mut App) {
    cx.set_global(DisplayGlobal(prefs.clamped()));
    cx.refresh_windows();
}

/// Updates the global [`DisplayPrefs`] without triggering `refresh_windows`
/// (used inside `ClientView::refresh` which already finishes with `cx.notify`).
pub fn sync_display_global(prefs: DisplayPrefs, cx: &mut App) -> bool {
    let clamped = prefs.clamped();
    if display(cx) != clamped {
        cx.set_global(DisplayGlobal(clamped));
        true
    } else {
        false
    }
}

/// Which palette to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Light,
    /// Official `dark` (the default).
    Dark,
    /// Official `dark_legacy` (original neutral gray palette).
    Darker,
    /// Official `coal` (pitch-black surfaces).
    Black,
    /// Follow the OS dark-mode setting.
    System,
}

impl ThemeKind {
    /// Parses `light|dark|darker|black|system` plus the official ids and
    /// common aliases (`dark_legacy`, `legacy`, `coal`, `amoled`).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "darker" | "dark_legacy" | "legacy" => Some(Self::Darker),
            "black" | "coal" | "amoled" => Some(Self::Black),
            "system" | "auto" => Some(Self::System),
            _ => None,
        }
    }

    /// Official settings string (`dark`, `dark_legacy`, `coal`, `light`, `system`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Darker => "dark_legacy",
            Self::Black => "coal",
            Self::System => "system",
        }
    }

    /// Kind chosen by `FLUXER_THEME`; Dark when unset or invalid. Tests
    /// default to Light so headless color assertions stay stable.
    pub fn from_env() -> Self {
        let fallback = if cfg!(test) { Self::Light } else { Self::Dark };
        std::env::var("FLUXER_THEME")
            .ok()
            .and_then(|value| Self::parse(&value))
            .unwrap_or(fallback)
    }

    /// Resolves `System` against the OS appearance; other kinds are unchanged.
    pub fn resolve(self, appearance: WindowAppearance) -> Self {
        match self {
            Self::System => match appearance {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
                WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
            },
            other => other,
        }
    }

    fn is_dark(self) -> bool {
        !matches!(self, Self::Light)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// `--background-primary` (message area).
    pub background: Hsla,
    /// `--background-secondary` (channel list, user panel).
    pub background_secondary: Hsla,
    /// `--background-secondary-lighter` (`OutlineFrame` main content frame:
    /// channel header, chat area, member list, right panels).
    pub background_secondary_lighter: Hsla,
    /// `--background-tertiary` (guild rail).
    pub background_tertiary: Hsla,
    /// `--guild-list-foreground` (surface stop 0.38 dark/coal, 0.35 light).
    pub guild_list_foreground: Hsla,
    /// `--text-chat` (message body, member names).
    pub text_chat: Hsla,
    /// `--text-secondary`.
    pub text_secondary: Hsla,
    /// `--text-tertiary`.
    pub text_tertiary: Hsla,
    /// `--text-tertiary-muted` (unselected channel names, categories).
    pub text_tertiary_muted: Hsla,
    /// `--text-primary`.
    pub foreground: Hsla,
    /// Unselected rail buttons.
    pub muted: Hsla,
    /// `--background-modifier-hover` (dark: `rgba(255,255,255,0.05)`, light: `rgba(23,25,28,0.05)`).
    pub hover_bg: Hsla,
    /// User area & icon control button hover tint (`--text-primary-muted` at 10% alpha).
    pub control_hover_bg: Hsla,
    /// Subtle button fill such as voice panel media buttons (dark: `rgba(255,255,255,0.035)`).
    pub subtle_fill: Hsla,
    /// `--text-primary-muted`.
    pub muted_foreground: Hsla,
    /// Selected rail button / channel row (`--background-modifier-selected`).
    pub accent: Hsla,
    /// `--border-color`.
    pub border: Hsla,
    /// `--background-modifier-accent` (subtle surface/control border).
    pub background_modifier_accent: Hsla,
    /// Divider lines; the same token as `border`.
    pub divider: Hsla,
    /// `--brand-primary`.
    pub brand: Hsla,
    /// `--text-link`.
    pub text_link: Hsla,
    /// Unread channel pill (`--text-primary`).
    pub unread_pill: Hsla,
    /// Errors, mention badges, the "NEW" divider (`--status-danger`).
    pub danger: Hsla,
    /// Text on filled shapes (badges, "NEW", avatar initials).
    pub on_fill: Hsla,
    /// System message icons (`GuildJoinMessage.module.css`: rgb(34 197 94)).
    pub join_icon: Hsla,
    pub leave_icon: Hsla,
    /// Image preview while it loads.
    pub image_placeholder: Hsla,
    /// `--markup-mention-fill`.
    pub mention_bg: Hsla,
    /// `--markup-mention-text`.
    pub mention_fg: Hsla,
    /// User panel status dot.
    pub status_online: Hsla,
    /// `--status-idle`.
    pub status_connecting: Hsla,
    pub status_idle: Hsla,
    pub status_dnd: Hsla,
    pub status_offline: Hsla,
    pub sizes: Sizes,
}

/// Layout sizes in pixels synced 1:1 with official `fluxer_app` CSS tokens
/// (`globals.css`, `MessageLayoutSpec.ts`, `SidebarWidth.ts`, `SettingsModalLayout.module.css`
/// at `1rem = 16px`, 100% zoom).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeSizes {
    /// Base text size of the client layout.
    pub text: f32,
    /// `--layout-guild-list-width: 4.5rem` (72px).
    pub rail_width: f32,
    /// `--guild-icon-size: 2.75rem` (44px).
    pub rail_button: f32,
    /// `SIDEBAR_WIDTH_DEFAULT` / `--layout-sidebar-width: 20rem` (320px).
    pub channel_list_width: f32,
    /// `--layout-header-height: 3.5rem` (56px).
    pub header_height: f32,
    /// `--message-search-bar-width: 15.25rem` (244px).
    pub search_bar_width: f32,
    /// `--message-search-bar-height: 2.25rem` (36px).
    pub search_bar_height: f32,
    /// `--layout-member-list-width: 16.5rem` (264px).
    pub member_list_width: f32,
    /// `MessageLayoutSpec.cozy.avatarSize: 2.5rem` (40px).
    pub avatar: f32,
    /// Message avatar column (`cozy.avatarSize` 2.5rem + `gutter` 0.75rem = 52px).
    pub gutter: f32,
    /// `MessageLayoutSpec.spacingY / containerPaddingY: 0.125rem` (2px).
    pub message_padding_y: f32,
    /// Space above the first message of a group (16px cozy, 4px compact).
    pub group_gap: f32,
    /// `--footer-box-height: 3.625rem` (58px).
    pub composer_height: f32,
    /// `--footer-box-radius: var(--radius-lg)` (8px).
    pub composer_radius: f32,
    /// `SettingsModalLayout.desktopSidebar` min `15.75rem` (252px).
    pub settings_sidebar_width: f32,
    /// Settings modal fixed top header height (56px).
    pub settings_header_height: f32,
    /// Settings modal body max width (740px).
    pub settings_content_max_width: f32,
    /// Settings modal dialog max width (1398px).
    pub settings_modal_max_width: f32,
    /// `--native-titlebar-height: 32px`.
    pub titlebar_height: f32,
    /// Icon of the "Welcome to #name" header.
    pub channel_start_icon: f32,
    /// `--user-area-content-height: 2rem` (32px).
    pub user_panel_avatar: f32,
    pub status_dot: f32,
    pub compact: bool,
}

/// Compatibility alias for [`ThemeSizes`].
pub type Sizes = ThemeSizes;

impl ThemeSizes {
    pub const DEFAULT: ThemeSizes = ThemeSizes {
        text: 14.,
        rail_width: 72.,
        rail_button: 44.,
        channel_list_width: 320.,
        header_height: 56.,
        search_bar_width: 244.,
        search_bar_height: 36.,
        member_list_width: 264.,
        avatar: 40.,
        gutter: 52.,
        message_padding_y: 2.,
        group_gap: 16.,
        composer_height: 58.,
        composer_radius: 8.,
        settings_sidebar_width: 252.,
        settings_header_height: 56.,
        settings_content_max_width: 740.,
        settings_modal_max_width: 1398.,
        titlebar_height: 32.,
        channel_start_icon: 68.,
        user_panel_avatar: 32.,
        status_dot: 10.,
        compact: false,
    };

    /// Derives pixel dimensions from user font size, zoom percentage, and density mode.
    pub fn for_display(display: DisplayPrefs) -> Self {
        let display = display.clamped();
        let zoom = f32::from(display.ui_zoom) / 100.0;
        let scale = |v: f32| (v * zoom).round();
        let (avatar, gutter, group_gap, message_padding_y) = if display.compact {
            (0., 0., scale(4.), scale(1.))
        } else {
            (
                scale(Self::DEFAULT.avatar),
                scale(Self::DEFAULT.gutter),
                scale(Self::DEFAULT.group_gap),
                scale(Self::DEFAULT.message_padding_y),
            )
        };
        Self {
            text: scale(f32::from(display.font_size)),
            rail_width: scale(Self::DEFAULT.rail_width),
            rail_button: scale(Self::DEFAULT.rail_button),
            channel_list_width: scale(Self::DEFAULT.channel_list_width),
            header_height: scale(Self::DEFAULT.header_height),
            search_bar_width: scale(Self::DEFAULT.search_bar_width),
            search_bar_height: scale(Self::DEFAULT.search_bar_height),
            member_list_width: scale(Self::DEFAULT.member_list_width),
            avatar,
            gutter,
            message_padding_y,
            group_gap,
            composer_height: scale(Self::DEFAULT.composer_height),
            composer_radius: scale(Self::DEFAULT.composer_radius),
            settings_sidebar_width: scale(Self::DEFAULT.settings_sidebar_width),
            settings_header_height: scale(Self::DEFAULT.settings_header_height),
            settings_content_max_width: scale(Self::DEFAULT.settings_content_max_width),
            settings_modal_max_width: scale(Self::DEFAULT.settings_modal_max_width),
            titlebar_height: Self::DEFAULT.titlebar_height,
            channel_start_icon: scale(Self::DEFAULT.channel_start_icon),
            user_panel_avatar: scale(Self::DEFAULT.user_panel_avatar),
            status_dot: scale(Self::DEFAULT.status_dot),
            compact: display.compact,
        }
    }
}

/// One HSL token: hue in degrees, saturation and lightness in percent.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hsl {
    h: f32,
    s: f32,
    l: f32,
    a: f32,
}

impl Hsl {
    const fn new(h: f32, s: f32, l: f32) -> Self {
        Self { h, s, l, a: 1. }
    }

    const fn alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    fn color(self) -> Hsla {
        hsla(self.h / 360., self.s / 100., self.l / 100., self.a)
    }
}

#[derive(Clone, Copy)]
enum Curve {
    EaseIn,
    EaseOut,
    EaseInOut,
}

/// `scaleStopLightness`: lightness at `position` of a scale `range`.
fn scale_lightness(range: (f32, f32), curve: Curve, position: f32) -> f32 {
    let t = position.clamp(0., 1.);
    let eased = match curve {
        Curve::EaseIn => t * t,
        Curve::EaseOut => 1. - (1. - t) * (1. - t),
        Curve::EaseInOut if t < 0.5 => 2. * t * t,
        Curve::EaseInOut => 1. - 2. * (1. - t) * (1. - t),
    };
    let lightness = range.0 + (range.1 - range.0) * eased;
    (lightness * 1000.).round() / 1000.
}

/// Source values of one official palette.
struct Palette {
    /// Surface and text family as (hue, saturation).
    family: (f32, f32),
    surface_range: (f32, f32),
    surface_curve: Curve,
    stops: SurfaceStops,
    text_range: (f32, f32),
    text_curve: Curve,
    /// Coal aliases `--background-secondary` to `--background-primary`.
    secondary_is_primary: bool,
    selected: Hsl,
    border: Hsl,
    modifier_accent: Hsl,
    link: Hsl,
    online: Hsl,
    idle: Hsl,
    dnd: Hsl,
    offline: Hsl,
}

/// Surface scale stop positions of `--background-primary`, `-secondary` and
/// `-tertiary` (`COLOR_SCALES.*Surface`). The light scale runs dark to bright,
/// so its primary sits at the top.
type SurfaceStops = (f32, f32, f32);
const DARK_STOPS: SurfaceStops = (0., 0.16, 0.4);
const LIGHT_STOPS: SurfaceStops = (1., 0.74, 0.42);

/// First family of the official `--font-sans` stack on Windows.
#[cfg(windows)]
const OFFICIAL_WINDOWS_SANS: &str = "Segoe UI Variable Text";

const STATUS_DANGER: Hsl = Hsl::new(1., 77., 55.);
const BRAND_PRIMARY: Hsl = Hsl::new(242., 70., 55.);

fn dark_palette() -> Palette {
    Palette {
        family: (258., 10.),
        surface_range: (5., 24.),
        surface_curve: Curve::EaseOut,
        stops: DARK_STOPS,
        text_range: (60., 96.),
        text_curve: Curve::EaseInOut,
        secondary_is_primary: false,
        selected: Hsl::new(258., 10., 100.).alpha(0.1),
        border: Hsl::new(258., 10., 50.).alpha(0.2),
        modifier_accent: Hsl::new(258., 13., 80.).alpha(0.15),
        link: Hsl::new(198., 92., 70.),
        online: Hsl::new(152., 72., 40.),
        idle: Hsl::new(45., 93., 50.),
        dnd: Hsl::new(0., 84., 60.),
        offline: Hsl::new(218., 11., 65.),
    }
}

fn coal_palette() -> Palette {
    Palette {
        surface_range: (1., 12.),
        secondary_is_primary: true,
        selected: Hsl::new(258., 10., 100.).alpha(0.08),
        modifier_accent: Hsl::new(258., 10., 65.).alpha(0.18),
        ..dark_palette()
    }
}

fn legacy_palette() -> Palette {
    Palette {
        family: (220., 13.),
        surface_range: (5., 26.),
        text_range: (52., 96.),
        selected: Hsl::new(220., 13., 100.).alpha(0.1),
        border: Hsl::new(220., 13., 50.).alpha(0.2),
        modifier_accent: Hsl::new(220., 13., 80.).alpha(0.15),
        link: Hsl::new(210., 100., 70.),
        online: Hsl::new(142., 76., 40.),
        ..dark_palette()
    }
}

fn light_palette() -> Palette {
    Palette {
        family: (220., 10.),
        surface_range: (86., 98.5),
        surface_curve: Curve::EaseIn,
        stops: LIGHT_STOPS,
        text_range: (15., 54.),
        text_curve: Curve::EaseOut,
        secondary_is_primary: false,
        selected: Hsl::new(220., 10., 10.).alpha(0.1),
        border: Hsl::new(220., 10., 40.).alpha(0.15),
        modifier_accent: Hsl::new(220., 10., 40.).alpha(0.22),
        link: Hsl::new(198., 92., 45.),
        online: Hsl::new(152., 70., 40.),
        idle: Hsl::new(45., 90., 45.),
        dnd: Hsl::new(359., 70., 50.),
        offline: Hsl::new(210., 10., 55.),
    }
}

impl Palette {
    fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Light => light_palette(),
            ThemeKind::Darker => legacy_palette(),
            ThemeKind::Black => coal_palette(),
            // `System` is resolved by the caller; Dark is the safe default.
            ThemeKind::Dark | ThemeKind::System => dark_palette(),
        }
    }

    fn surface(&self, position: f32) -> Hsl {
        let l = scale_lightness(self.surface_range, self.surface_curve, position);
        Hsl::new(self.family.0, self.family.1, l)
    }

    /// Text token by strength: 1 is `--text-primary`, 0.55 is
    /// `--text-primary-muted`. The dark text scale runs dim to bright, the
    /// light scale runs bright (dark text) to dim, so light inverts it.
    fn text(&self, light: bool, strength: f32) -> Hsl {
        let position = if light { 1. - strength } else { strength };
        let l = scale_lightness(self.text_range, self.text_curve, position);
        Hsl::new(self.family.0, self.family.1, l)
    }
}

impl Theme {
    /// The theme in use: the `Selected` global, else `FLUXER_THEME`, else Dark.
    pub fn current(cx: &App) -> Self {
        Self::for_kind(kind(cx), cx)
    }

    pub fn for_kind(kind: ThemeKind, cx: &App) -> Self {
        let mut theme = Self::from_kind(kind.resolve(cx.window_appearance()));
        theme.sizes = Sizes::for_display(display(cx));
        theme
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn light(_cx: &App) -> Self {
        Self::from_kind(ThemeKind::Light)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn dark(_cx: &App) -> Self {
        Self::from_kind(ThemeKind::Dark)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn darker(_cx: &App) -> Self {
        Self::from_kind(ThemeKind::Darker)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn black(_cx: &App) -> Self {
        Self::from_kind(ThemeKind::Black)
    }

    /// Returns the static color preview for `kind` (for settings theme swatch cards).
    pub fn preview(kind: ThemeKind) -> Self {
        Self::from_kind(kind)
    }

    /// Builds the palette of an already resolved kind (`System` acts as Dark).
    fn from_kind(kind: ThemeKind) -> Self {
        let p = Palette::for_kind(kind);
        let light = !kind.is_dark();
        let primary = p.surface(p.stops.0);
        let secondary = if p.secondary_is_primary {
            primary
        } else {
            p.surface(p.stops.1)
        };
        let text_primary = p.text(light, 1.);
        // `--background-secondary-lighter`: position 0.22 dark, 0.83 light;
        // coal aliases it to `--background-primary`.
        let lighter = if p.secondary_is_primary {
            primary
        } else {
            p.surface(if light { 0.83 } else { 0.22 })
        };
        // Text stop positions of `ColorSystemPalette` as strengths (the light
        // scale position is `1 - strength`).
        let (chat, secondary_text, tertiary, tertiary_muted) = if light {
            (0.92, 0.72, 0.4, 0.15)
        } else {
            (0.82, 0.72, 0.38, 0.2)
        };
        Self {
            background: primary.color(),
            background_secondary: secondary.color(),
            background_secondary_lighter: lighter.color(),
            background_tertiary: p.surface(p.stops.2).color(),
            guild_list_foreground: p.surface(if light { 0.35 } else { 0.38 }).color(),
            text_chat: p.text(light, chat).color(),
            text_secondary: p.text(light, secondary_text).color(),
            text_tertiary: p.text(light, tertiary).color(),
            text_tertiary_muted: p.text(light, tertiary_muted).color(),
            foreground: text_primary.color(),
            muted: secondary.color(),
            hover_bg: if light {
                Hsl::new(220., 10., 10.).alpha(0.05).color()
            } else {
                hsla(0., 0., 1., 0.05)
            },
            control_hover_bg: p.text(light, 0.55).alpha(0.10).color(),
            subtle_fill: if light {
                hsla(0., 0., 0., 0.04)
            } else {
                hsla(0., 0., 1., 0.035)
            },
            muted_foreground: p.text(light, 0.55).color(),
            accent: p.selected.color(),
            border: p.border.color(),
            background_modifier_accent: p.modifier_accent.color(),
            divider: p.border.color(),
            brand: BRAND_PRIMARY.color(),
            text_link: p.link.color(),
            unread_pill: text_primary.color(),
            danger: STATUS_DANGER.color(),
            on_fill: gpui_kit::white(),
            join_icon: rgb(0x22c55e).into(),
            leave_icon: rgb(0xef4444).into(),
            image_placeholder: hsla(0., 0., 0.5, 0.15),
            mention_bg: p.link.alpha(0.2).color(),
            mention_fg: p.link.color(),
            status_online: p.online.color(),
            status_connecting: p.idle.color(),
            status_idle: p.idle.color(),
            status_dnd: p.dnd.color(),
            status_offline: p.offline.color(),
            sizes: Sizes::DEFAULT,
        }
    }
}

/// Makes the gpui-kit global theme (inputs, buttons, scrollbars) follow
/// `kind` and Fluxer's official brand primary palette (`#5865F2`).
/// Call at startup and whenever the user changes the theme.
pub fn apply(kind: ThemeKind, window: &mut Window, cx: &mut App) {
    let resolved = kind.resolve(window.appearance());
    let mode = if resolved.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    KitTheme::change(mode, Some(window), cx);
    let brand = BRAND_PRIMARY.color();
    let brand_hover = Hsl::new(BRAND_PRIMARY.h, BRAND_PRIMARY.s, (BRAND_PRIMARY.l - 6.0).max(0.0)).color();
    let brand_active = Hsl::new(BRAND_PRIMARY.h, BRAND_PRIMARY.s, (BRAND_PRIMARY.l - 12.0).max(0.0)).color();
    let kit = KitTheme::global_mut(cx);
    // Official desktop `--font-sans` starts with "Segoe UI Variable Text" on Windows 11 (measured
    // via the official DOM); the GPUI default system font is plain Segoe UI with narrower metrics.
    #[cfg(windows)]
    {
        kit.font_family = OFFICIAL_WINDOWS_SANS.into();
    }
    kit.colors.primary = brand;
    kit.colors.primary_hover = brand_hover;
    kit.colors.primary_active = brand_active;
    kit.colors.primary_foreground = gpui_kit::white();
    kit.colors.button_primary = brand;
    kit.colors.button_primary_hover = brand_hover;
    kit.colors.button_primary_active = brand_active;
    kit.colors.button_primary_foreground = gpui_kit::white();
    kit.colors.ring = brand;
    kit.tokens.primary = brand.into();
    kit.tokens.primary_hover = brand_hover.into();
    kit.tokens.primary_active = brand_active.into();
    kit.tokens.primary_foreground = gpui_kit::white().into();
    kit.tokens.button_primary = brand.into();
    kit.tokens.button_primary_hover = brand_hover.into();
    kit.tokens.button_primary_active = brand_active.into();
    kit.tokens.button_primary_foreground = gpui_kit::white().into();
    kit.tokens.ring = brand.into();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kind_names_and_aliases() {
        assert_eq!(ThemeKind::parse("Light"), Some(ThemeKind::Light));
        assert_eq!(ThemeKind::parse(" dark "), Some(ThemeKind::Dark));
        assert_eq!(ThemeKind::parse("darker"), Some(ThemeKind::Darker));
        assert_eq!(ThemeKind::parse("dark_legacy"), Some(ThemeKind::Darker));
        assert_eq!(ThemeKind::parse("black"), Some(ThemeKind::Black));
        assert_eq!(ThemeKind::parse("coal"), Some(ThemeKind::Black));
        assert_eq!(ThemeKind::parse("system"), Some(ThemeKind::System));
        assert_eq!(ThemeKind::parse("pink"), None);
    }

    #[test]
    fn system_follows_appearance() {
        assert_eq!(
            ThemeKind::System.resolve(WindowAppearance::Dark),
            ThemeKind::Dark
        );
        assert_eq!(
            ThemeKind::System.resolve(WindowAppearance::VibrantLight),
            ThemeKind::Light
        );
        assert_eq!(
            ThemeKind::Black.resolve(WindowAppearance::Light),
            ThemeKind::Black
        );
    }

    #[test]
    fn scale_matches_the_official_generator() {
        assert_eq!(scale_lightness((5., 24.), Curve::EaseOut, 0.), 5.);
        assert_eq!(scale_lightness((86., 98.5), Curve::EaseIn, 1.), 98.5);
        // darkSurface --background-secondary: 5 + 19 * (1 - 0.84^2) = 10.5936
        assert_eq!(scale_lightness((5., 24.), Curve::EaseOut, 0.16), 10.594);
        // darkText --text-primary is the top of the range.
        assert_eq!(scale_lightness((60., 96.), Curve::EaseInOut, 1.), 96.);
    }

    #[test]
    fn dark_tokens_use_the_official_hsl_values() {
        let p = dark_palette();
        assert_eq!(p.surface(p.stops.0), Hsl::new(258., 10., 5.));
        assert_eq!(p.text(false, 1.), Hsl::new(258., 10., 96.));
        let theme = Theme::from_kind(ThemeKind::Dark);
        assert_eq!(theme.background, hsla(258. / 360., 0.10, 0.05, 1.));
        assert_eq!(theme.brand, hsla(242. / 360., 0.70, 0.55, 1.));
        assert_eq!(theme.status_online, hsla(152. / 360., 0.72, 0.40, 1.));
    }

    #[test]
    fn lighter_surface_and_text_tokens_match_the_manifest() {
        let near = |a: Hsla, h: f32, sat: f32, l: f32| {
            assert!((a.h - h / 360.).abs() < 1e-4 && (a.s - sat / 100.).abs() < 1e-4 && (a.l - l / 100.).abs() < 6e-4, "{a:?} vs {l}");
        };
        let d = Theme::from_kind(ThemeKind::Dark);
        near(d.background_secondary_lighter, 258., 10., 12.44);
        near(d.text_chat, 258., 10., 93.67);
        near(d.text_secondary, 258., 10., 90.36);
        near(d.text_tertiary, 258., 10., 70.4);
        near(d.text_tertiary_muted, 258., 10., 62.88);
        near(d.muted_foreground, 258., 10., 81.42);
        // ThemeVariableManifest.ts `--guild-list-foreground`.
        near(d.guild_list_foreground, 258., 10., 16.7);
        let l = Theme::from_kind(ThemeKind::Light);
        near(l.background_secondary_lighter, 220., 10., 94.61);
        near(l.text_chat, 220., 10., 20.99);
        near(l.text_secondary, 220., 10., 33.78);
        near(l.text_tertiary, 220., 10., 47.76);
        near(l.text_tertiary_muted, 220., 10., 53.12);
        near(l.guild_list_foreground, 220., 10., 87.53);
        let c = Theme::from_kind(ThemeKind::Black);
        assert_eq!(c.background_secondary_lighter, c.background);
    }

    #[test]
    fn light_text_is_dark_and_surface_is_bright() {
        let theme = Theme::from_kind(ThemeKind::Light);
        assert_eq!(theme.background, hsla(220. / 360., 0.10, 0.985, 1.));
        assert_eq!(theme.foreground, hsla(220. / 360., 0.10, 0.15, 1.));
        assert_eq!(theme.status_dnd, hsla(359. / 360., 0.70, 0.50, 1.));
    }

    #[test]
    fn coal_aliases_secondary_to_primary() {
        let theme = Theme::from_kind(ThemeKind::Black);
        assert_eq!(theme.background, theme.background_secondary);
        assert_eq!(theme.background, hsla(258. / 360., 0.10, 0.01, 1.));
    }

    #[test]
    fn legacy_uses_the_gray_family_and_link() {
        let theme = Theme::from_kind(ThemeKind::Darker);
        assert_eq!(theme.background, hsla(220. / 360., 0.13, 0.05, 1.));
        assert_eq!(theme.text_link, hsla(210. / 360., 1.0, 0.70, 1.));
    }

    #[test]
    fn every_kind_has_distinct_backgrounds_and_readable_text() {
        let themes = [
            Theme::from_kind(ThemeKind::Light),
            Theme::from_kind(ThemeKind::Dark),
            Theme::from_kind(ThemeKind::Darker),
            Theme::from_kind(ThemeKind::Black),
        ];
        for (i, a) in themes.iter().enumerate() {
            assert_ne!(a.background, a.foreground);
            for b in &themes[i + 1..] {
                assert_ne!(a.background, b.background);
            }
        }
    }

    #[test]
    fn sizes_scale_with_zoom_and_compact_density() {
        assert_eq!(ThemeSizes::for_display(DisplayPrefs::default()), ThemeSizes::DEFAULT);

        let zoomed = ThemeSizes::for_display(DisplayPrefs {
            font_size: 16,
            ui_zoom: 125,
            compact: false,
        });
        assert_eq!(zoomed.text, 20.);
        assert_eq!(zoomed.rail_width, 90.);
        assert_eq!(zoomed.channel_list_width, 400.);
        assert_eq!(zoomed.header_height, 70.);
        assert_eq!(zoomed.member_list_width, 330.);
        assert_eq!(zoomed.avatar, 50.);
        assert_eq!(zoomed.gutter, 65.);
        assert_eq!(zoomed.group_gap, 20.);
        assert!(!zoomed.compact);

        let compact = ThemeSizes::for_display(DisplayPrefs {
            font_size: 14,
            ui_zoom: 100,
            compact: true,
        });
        assert!(compact.compact);
        assert_eq!(compact.avatar, 0.);
        assert_eq!(compact.gutter, 0.);
        assert_eq!(compact.group_gap, 4.);
        assert_eq!(compact.message_padding_y, 1.);
    }

    #[test]
    fn theme_sizes_match_official_fluxer_app_layout_spec() {
        // Official rem values from fluxer_app/src/app/globals.css, MessageLayoutSpec.ts,
        // SidebarWidth.ts, and SettingsModalLayout.module.css at 1rem = 16px.
        const REM: f32 = 16.0;
        let s = ThemeSizes::DEFAULT;
        assert_eq!(s.rail_width, 4.5 * REM);
        assert_eq!(s.rail_button, 2.75 * REM);
        assert_eq!(s.channel_list_width, 20.0 * REM);
        assert_eq!(s.header_height, 3.5 * REM);
        assert_eq!(s.search_bar_width, 15.25 * REM);
        assert_eq!(s.search_bar_height, 2.25 * REM);
        assert_eq!(s.member_list_width, 16.5 * REM);
        assert_eq!(s.avatar, 2.5 * REM);
        assert_eq!(s.gutter, (2.5 + 0.75) * REM);
        assert_eq!(s.message_padding_y, 0.125 * REM);
        assert_eq!(s.composer_height, 3.625 * REM);
        assert_eq!(s.composer_radius, 0.5 * REM);
        assert_eq!(s.settings_sidebar_width, 15.75 * REM);
        assert_eq!(s.titlebar_height, 32.0);
        assert_eq!(s.user_panel_avatar, 2.0 * REM);

        // Verify default (1280x800) and maximized (1920x1040) windows retain ample center pane width.
        let chrome_with_members = s.rail_width + s.channel_list_width + s.member_list_width;
        assert!(1280.0 - chrome_with_members >= 600.0, "1280x800 center width = {}", 1280.0 - chrome_with_members);
        assert!(1920.0 - chrome_with_members >= 1200.0, "1920x1040 center width = {}", 1920.0 - chrome_with_members);
        assert!(800.0 - s.header_height - s.composer_height >= 640.0);
        assert!(1040.0 - s.header_height - s.composer_height >= 880.0);
    }

    #[test]
    fn hover_and_control_tokens_are_distinct_from_secondary() {
        for kind in [ThemeKind::Dark, ThemeKind::Light, ThemeKind::Darker, ThemeKind::Black] {
            let t = Theme::from_kind(kind);
            assert_ne!(t.hover_bg, t.background_secondary);
            assert!(t.hover_bg.a > 0.0);
            assert!(t.control_hover_bg.a > 0.0);
            assert!(t.subtle_fill.a > 0.0);
        }
    }
}
