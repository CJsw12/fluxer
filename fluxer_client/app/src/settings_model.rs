//! Pure data and helpers for the settings surface: the official locale list,
//! theme options and the effects the UI dispatches after a change.

use crate::theme::ThemeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleOption {
    pub code: &'static str,
    pub native_name: &'static str,
}

/// The 44 official locales (`SUPPORTED_LOCALES` in `LocaleUtils.ts`).
pub const LOCALES: &[LocaleOption] = &[
    LocaleOption { code: "ar", native_name: "العربية" },
    LocaleOption { code: "be", native_name: "Беларуская" },
    LocaleOption { code: "bg", native_name: "Български" },
    LocaleOption { code: "bn", native_name: "বাংলা" },
    LocaleOption { code: "ca", native_name: "Català" },
    LocaleOption { code: "cs", native_name: "Čeština" },
    LocaleOption { code: "da", native_name: "Dansk" },
    LocaleOption { code: "de", native_name: "Deutsch" },
    LocaleOption { code: "el", native_name: "Ελληνικά" },
    LocaleOption { code: "en-GB", native_name: "English" },
    LocaleOption { code: "en-US", native_name: "English (US)" },
    LocaleOption { code: "es-ES", native_name: "Español (España)" },
    LocaleOption { code: "es-419", native_name: "Español (Latinoamérica)" },
    LocaleOption { code: "fi", native_name: "Suomi" },
    LocaleOption { code: "fil", native_name: "Filipino" },
    LocaleOption { code: "fr", native_name: "Français" },
    LocaleOption { code: "he", native_name: "עברית" },
    LocaleOption { code: "hi", native_name: "हिन्दी" },
    LocaleOption { code: "hr", native_name: "Hrvatski" },
    LocaleOption { code: "hu", native_name: "Magyar" },
    LocaleOption { code: "id", native_name: "Bahasa Indonesia" },
    LocaleOption { code: "it", native_name: "Italiano" },
    LocaleOption { code: "ja", native_name: "日本語" },
    LocaleOption { code: "ko", native_name: "한국어" },
    LocaleOption { code: "lt", native_name: "Lietuvių" },
    LocaleOption { code: "lv", native_name: "Latviešu" },
    LocaleOption { code: "nl", native_name: "Nederlands" },
    LocaleOption { code: "no", native_name: "Norsk" },
    LocaleOption { code: "pl", native_name: "Polski" },
    LocaleOption { code: "pt-BR", native_name: "Português (Brasil)" },
    LocaleOption { code: "pt-PT", native_name: "Português (Portugal)" },
    LocaleOption { code: "ro", native_name: "Română" },
    LocaleOption { code: "ru", native_name: "Русский" },
    LocaleOption { code: "sk", native_name: "Slovenčina" },
    LocaleOption { code: "sl", native_name: "Slovenščina" },
    LocaleOption { code: "sq", native_name: "Shqip" },
    LocaleOption { code: "sv-SE", native_name: "Svenska" },
    LocaleOption { code: "th", native_name: "ไทย" },
    LocaleOption { code: "tok", native_name: "toki pona" },
    LocaleOption { code: "tr", native_name: "Türkçe" },
    LocaleOption { code: "uk", native_name: "Українська" },
    LocaleOption { code: "vi", native_name: "Tiếng Việt" },
    LocaleOption { code: "zh-CN", native_name: "中文 (简体)" },
    LocaleOption { code: "zh-TW", native_name: "中文 (繁體)" },
];

/// The native name of `code`, or `code` itself for unsupported codes.
#[cfg_attr(not(test), allow(dead_code))]
pub fn locale_native_name(code: &str) -> &str {
    match LOCALES.iter().find(|locale| locale.code == code) {
        Some(locale) => locale.native_name,
        None => code,
    }
}

pub const THEME_OPTIONS: &[(ThemeKind, &str)] = &[
    (ThemeKind::Dark, "Dark"),
    (ThemeKind::Light, "Light"),
    (ThemeKind::System, "System"),
    (ThemeKind::Darker, "Dark (Legacy)"),
    (ThemeKind::Black, "Coal"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsEffect {
    ThemeChanged(ThemeKind),
    LocaleChanged(String),
    NotificationsChanged(bool),
    SoundsChanged(bool),
    AutostartChanged(bool),
    CloseToTrayChanged(bool),
    LogOutRequested,
}

/// The official `theme` id sent in the user-settings patch.
pub fn theme_patch_value(kind: ThemeKind) -> &'static str {
    kind.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_all_official_locales_once() {
        assert_eq!(LOCALES.len(), 44);
        let mut codes: Vec<&str> = LOCALES.iter().map(|locale| locale.code).collect();
        let mut available = fluxer_client_i18n::available_locales();
        for code in &available {
            assert!(codes.contains(code), "{code} is missing from LOCALES");
        }
        for code in &codes {
            assert!(available.contains(code), "{code} is not an embedded catalog");
        }
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 44, "duplicate locale codes");
        available.sort_unstable();
        assert_eq!(codes, available);
    }

    #[test]
    fn native_names_and_unknown_fallback() {
        assert_eq!(locale_native_name("ko"), "한국어");
        assert_eq!(locale_native_name("unknown"), "unknown");
    }

    #[test]
    fn theme_patch_values_preserve_the_official_ids() {
        assert_eq!(theme_patch_value(ThemeKind::Light), "light");
        assert_eq!(theme_patch_value(ThemeKind::Dark), "dark");
        assert_eq!(theme_patch_value(ThemeKind::Darker), "dark_legacy");
        assert_eq!(theme_patch_value(ThemeKind::Black), "coal");
        assert_eq!(theme_patch_value(ThemeKind::System), "system");
    }
}
