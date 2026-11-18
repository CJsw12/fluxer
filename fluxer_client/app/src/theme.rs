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
use gpui_kit::{App, Hsla, Window, WindowAppearance, hsla, rgb};

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
    /// `--background-tertiary` (guild rail).
    pub background_tertiary: Hsla,
    /// `--text-primary`.
    pub foreground: Hsla,
    /// Unselected rail buttons / hover fill.
    pub muted: Hsla,
    /// `--text-primary-muted`.
    pub muted_foreground: Hsla,
    /// Selected rail button / channel row (`--background-modifier-selected`).
    pub accent: Hsla,
    /// `--border-color`.
    pub border: Hsla,
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

/// Layout sizes in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// Base text size of the client layout.
    pub text: f32,
    pub rail_width: f32,
    pub rail_button: f32,
    pub channel_list_width: f32,
    /// Message avatar (`Message.module.css`: 2.5rem).
    pub avatar: f32,
    /// Avatar column width in the message list (avatar + gap).
    pub gutter: f32,
    /// Space above the first message of a group.
    pub group_gap: f32,
    /// Icon of the "Welcome to #name" header.
    pub channel_start_icon: f32,
    pub user_panel_avatar: f32,
    pub status_dot: f32,
}

impl Sizes {
    pub const DEFAULT: Sizes = Sizes {
        text: 14.,
        rail_width: 64.,
        rail_button: 44.,
        channel_list_width: 220.,
        avatar: 40.,
        gutter: 56.,
        group_gap: 16.,
        channel_start_icon: 68.,
        user_panel_avatar: 32.,
        status_dot: 10.,
    };
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
    /// The theme in use: `FLUXER_THEME`, Dark by default.
    pub fn current(cx: &App) -> Self {
        Self::for_kind(ThemeKind::from_env(), cx)
    }

    pub fn for_kind(kind: ThemeKind, cx: &App) -> Self {
        Self::from_kind(kind.resolve(cx.window_appearance()))
    }

    pub fn light(_cx: &App) -> Self {
        Self::from_kind(ThemeKind::Light)
    }

    pub fn dark(_cx: &App) -> Self {
        Self::from_kind(ThemeKind::Dark)
    }

    pub fn darker(_cx: &App) -> Self {
        Self::from_kind(ThemeKind::Darker)
    }

    pub fn black(_cx: &App) -> Self {
        Self::from_kind(ThemeKind::Black)
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
        Self {
            background: primary.color(),
            background_secondary: secondary.color(),
            background_tertiary: p.surface(p.stops.2).color(),
            foreground: text_primary.color(),
            muted: secondary.color(),
            muted_foreground: p.text(light, 0.55).color(),
            accent: p.selected.color(),
            border: p.border.color(),
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
/// `kind`. Call at startup and whenever the user changes the theme.
pub fn apply(kind: ThemeKind, window: &mut Window, cx: &mut App) {
    let resolved = kind.resolve(window.appearance());
    let mode = if resolved.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    KitTheme::change(mode, Some(window), cx);
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
}
