//! Pure data and helpers for the settings surface: the official locale list,
//! theme options and the effects the UI dispatches after a change.

use crate::prefs::ReducedMotionPref;
use crate::theme::ThemeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleOption {
    pub code: &'static str,
    pub native_name: &'static str,
    pub english_name: &'static str,
    pub flag: &'static str,
}

/// The 44 official locales (`SUPPORTED_LOCALES` in `LocaleUtils.ts`).
pub const LOCALES: &[LocaleOption] = &[
    LocaleOption { code: "ar", native_name: "العربية", english_name: "Arabic", flag: "" },
    LocaleOption { code: "be", native_name: "Беларуская", english_name: "Belarusian", flag: "" },
    LocaleOption { code: "bg", native_name: "Български", english_name: "Bulgarian", flag: "🇧🇬" },
    LocaleOption { code: "bn", native_name: "বাংলা", english_name: "Bangla", flag: "🇧🇩" },
    LocaleOption { code: "ca", native_name: "Català", english_name: "Catalan", flag: "" },
    LocaleOption { code: "cs", native_name: "Čeština", english_name: "Czech", flag: "🇨🇿" },
    LocaleOption { code: "da", native_name: "Dansk", english_name: "Danish", flag: "🇩🇰" },
    LocaleOption { code: "de", native_name: "Deutsch", english_name: "German", flag: "🇩🇪" },
    LocaleOption { code: "el", native_name: "Ελληνικά", english_name: "Greek", flag: "🇬🇷" },
    LocaleOption { code: "en-GB", native_name: "English", english_name: "English", flag: "🇬🇧" },
    LocaleOption { code: "en-US", native_name: "English (US)", english_name: "English (US)", flag: "🇺🇸" },
    LocaleOption { code: "es-ES", native_name: "Español (España)", english_name: "Spanish (Spain)", flag: "🇪🇸" },
    LocaleOption { code: "es-419", native_name: "Español (Latinoamérica)", english_name: "Spanish (Latin America)", flag: "" },
    LocaleOption { code: "fi", native_name: "Suomi", english_name: "Finnish", flag: "🇫🇮" },
    LocaleOption { code: "fil", native_name: "Filipino", english_name: "Filipino", flag: "🇵🇭" },
    LocaleOption { code: "fr", native_name: "Français", english_name: "French", flag: "🇫🇷" },
    LocaleOption { code: "he", native_name: "עברית", english_name: "Hebrew", flag: "🇮🇱" },
    LocaleOption { code: "hi", native_name: "हिन्दी", english_name: "Hindi", flag: "🇮🇳" },
    LocaleOption { code: "hr", native_name: "Hrvatski", english_name: "Croatian", flag: "🇭🇷" },
    LocaleOption { code: "hu", native_name: "Magyar", english_name: "Hungarian", flag: "🇭🇺" },
    LocaleOption { code: "id", native_name: "Bahasa Indonesia", english_name: "Indonesian", flag: "🇮🇩" },
    LocaleOption { code: "it", native_name: "Italiano", english_name: "Italian", flag: "🇮🇹" },
    LocaleOption { code: "ja", native_name: "日本語", english_name: "Japanese", flag: "🇯🇵" },
    LocaleOption { code: "ko", native_name: "한국어", english_name: "Korean", flag: "🇰🇷" },
    LocaleOption { code: "lt", native_name: "Lietuvių", english_name: "Lithuanian", flag: "🇱🇹" },
    LocaleOption { code: "lv", native_name: "Latviešu", english_name: "Latvian", flag: "🇱🇻" },
    LocaleOption { code: "nl", native_name: "Nederlands", english_name: "Dutch", flag: "🇳🇱" },
    LocaleOption { code: "no", native_name: "Norsk", english_name: "Norwegian", flag: "🇳🇴" },
    LocaleOption { code: "pl", native_name: "Polski", english_name: "Polish", flag: "🇵🇱" },
    LocaleOption { code: "pt-BR", native_name: "Português (Brasil)", english_name: "Portuguese (Brazil)", flag: "🇧🇷" },
    LocaleOption { code: "pt-PT", native_name: "Português (Portugal)", english_name: "Portuguese (Portugal)", flag: "🇵🇹" },
    LocaleOption { code: "ro", native_name: "Română", english_name: "Romanian", flag: "🇷🇴" },
    LocaleOption { code: "ru", native_name: "Русский", english_name: "Russian", flag: "🇷🇺" },
    LocaleOption { code: "sk", native_name: "Slovenčina", english_name: "Slovak", flag: "🇸🇰" },
    LocaleOption { code: "sl", native_name: "Slovenščina", english_name: "Slovenian", flag: "🇸🇮" },
    LocaleOption { code: "sq", native_name: "Shqip", english_name: "Albanian", flag: "🇦🇱" },
    LocaleOption { code: "sv-SE", native_name: "Svenska", english_name: "Swedish", flag: "🇸🇪" },
    LocaleOption { code: "th", native_name: "ไทย", english_name: "Thai", flag: "🇹🇭" },
    LocaleOption { code: "tok", native_name: "toki pona", english_name: "Toki Pona", flag: "" },
    LocaleOption { code: "tr", native_name: "Türkçe", english_name: "Turkish", flag: "🇹🇷" },
    LocaleOption { code: "uk", native_name: "Українська", english_name: "Ukrainian", flag: "🇺🇦" },
    LocaleOption { code: "vi", native_name: "Tiếng Việt", english_name: "Vietnamese", flag: "🇻🇳" },
    LocaleOption { code: "zh-CN", native_name: "中文 (简体)", english_name: "Chinese (simplified)", flag: "🇨🇳" },
    LocaleOption { code: "zh-TW", native_name: "中文 (繁體)", english_name: "Chinese (traditional)", flag: "🇹🇼" },
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

pub const FONT_SIZE_OPTIONS: &[u8] = &[12, 14, 16, 18, 20];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsEffect {
    ThemeChanged(ThemeKind),
    LocaleChanged(String),
    NotificationsChanged(bool),
    SoundsChanged(bool),
    AutostartChanged(bool),
    CloseToTrayChanged(bool),
    FontSizeChanged(u8),
    ZoomChanged(u16),
    CompactChanged(bool),
    VoiceInputDeviceChanged(Option<String>),
    VoiceOutputDeviceChanged(Option<String>),
    VoiceInputVolumeChanged(u16),
    VoiceOutputVolumeChanged(u16),
    VoicePushToTalkChanged(bool),
    VoiceNoiseSuppressionChanged(bool),
    VoiceProcessingModeChanged(String),
    VoiceEchoCancellationChanged(bool),
    VoiceNoiseSuppressionBackendChanged(String),
    VoiceAutoGainControlChanged(bool),
    VoiceVadAutoThresholdChanged(bool),
    VoiceVadThresholdChanged(u8),
    VoicePttReleaseDelayChanged(u16),
    ToggleMicTest,
    CameraResolutionChanged(String),
    MirrorCameraChanged(bool),
    StreamingModeChanged(String),
    VideoResolutionChanged(String),
    VideoFpsChanged(u8),
    VideoCodecChanged(String),
    VideoBitrateChanged(u16),
    VideoHardwareAccelerationChanged(bool),
    AllowGuildDmsChanged(bool),
    FriendRequestAccessChanged(String),
    SpoilerDisplayChanged(String),
    InlineMediaPreviewsChanged(bool),
    RenderEmbedsChanged(bool),
    RenderReactionsChanged(bool),
    ConvertEmoticonsChanged(bool),
    UnreadBadgeEnabledChanged(bool),
    TimeFormat24hChanged(bool),
    ReducedMotionChanged(ReducedMotionPref),
    ClearTrustedDomains,
    OpenAccountSwitcher,
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
