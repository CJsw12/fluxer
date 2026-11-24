//! Device-local client preferences (`prefs.json`), kept next to the session
//! store. The file holds only theme, notification/sound, autostart/tray
//! switches and the locale; it never contains account tokens.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde_json::{Value, json};

use crate::theme::{
    DEFAULT_FONT_SIZE, DEFAULT_UI_ZOOM, DisplayPrefs, MAX_FONT_SIZE, MAX_UI_ZOOM, MIN_FONT_SIZE, MIN_UI_ZOOM, ThemeKind,
};
use crate::trusted_domains;

pub const FILE_NAME: &str = "prefs.json";
pub const MIN_VOICE_VOLUME: u16 = 0;
pub const MAX_VOICE_VOLUME: u16 = 200;
pub const DEFAULT_VOICE_VOLUME: u16 = 100;

/// Live-measured official layout resize bounds (`Logs/parity/m93_official/resize_spec.json`).
pub const SIDEBAR_WIDTH_MIN: u16 = 200;
pub const SIDEBAR_WIDTH_MAX: u16 = 480;
pub const SIDEBAR_WIDTH_DEFAULT: u16 = 320;
pub const VOICE_CALL_HEIGHT_MIN: u16 = 320;
pub const VOICE_CALL_HEIGHT_MAX: u16 = 1049;
pub const VOICE_CALL_HEIGHT_DEFAULT: u16 = 320;
pub const EMOJI_PICKER_WIDTH_MIN: u16 = 400;
pub const EMOJI_PICKER_WIDTH_MAX: u16 = 1200;
pub const EMOJI_PICKER_WIDTH_DEFAULT: u16 = 592;
pub const EMOJI_PICKER_WIDTH_STEP: u16 = 48;
pub const EMOJI_PICKER_HEIGHT_MIN: u16 = 360;
pub const EMOJI_PICKER_HEIGHT_MAX: u16 = 1040;
pub const EMOJI_PICKER_HEIGHT_DEFAULT: u16 = 690;

/// Snaps `raw` emoji picker width to official 48px grid track steps (`400 + 48 * k`), clamped within `max_w`.
pub fn snap_emoji_picker_width(raw: f32, max_w: f32) -> u16 {
    let min_w = EMOJI_PICKER_WIDTH_MIN as f32;
    let step = EMOJI_PICKER_WIDTH_STEP as f32;
    let cap = max_w.clamp(min_w, EMOJI_PICKER_WIDTH_MAX as f32);
    let max_steps = ((cap - min_w) / step).floor().max(0.0);
    let snapped_max = min_w + max_steps * step;
    let steps = ((raw - min_w) / step).round().max(0.0);
    (min_w + steps * step).clamp(min_w, snapped_max).round() as u16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReducedMotionPref {
    #[default]
    System,
    On,
    Off,
}

impl ReducedMotionPref {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    pub fn from_str(raw: &str) -> Self {
        match raw {
            "on" => Self::On,
            "off" => Self::Off,
            _ => Self::System,
        }
    }
}

fn normalize_device_name(raw: Option<&str>) -> Option<String> {
    raw.filter(|s| !s.trim().is_empty()).map(str::to_owned)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub theme: ThemeKind,
    pub notifications: bool,
    pub sounds: bool,
    pub locale: String,
    pub autostart: bool,
    pub close_to_tray: bool,
    pub font_size: u8,
    pub ui_zoom: u16,
    pub compact: bool,
    pub trusted_domains: Vec<String>,
    pub voice_input_device: Option<String>,
    pub voice_output_device: Option<String>,
    pub voice_input_volume: u16,
    pub voice_output_volume: u16,
    pub voice_push_to_talk: bool,
    pub voice_noise_suppression: bool,
    pub voice_processing_mode: String,
    pub voice_echo_cancellation: bool,
    pub voice_noise_suppression_backend: String,
    pub voice_auto_gain_control: bool,
    pub voice_vad_auto_threshold: bool,
    pub voice_vad_threshold: u8,
    pub voice_ptt_release_delay_ms: u16,
    pub camera_resolution: String,
    pub mirror_camera: bool,
    pub streaming_mode: String,
    pub video_resolution: String,
    pub video_fps: u8,
    pub video_codec: String,
    pub video_bitrate_kbps: u16,
    pub video_hardware_acceleration: bool,
    pub allow_guild_dms: bool,
    pub friend_request_access: String,
    pub spoiler_display: String,
    pub inline_media_previews: bool,
    pub render_embeds: bool,
    pub render_reactions: bool,
    pub convert_emoticons: bool,
    pub unread_badge_enabled: bool,
    pub time_format_24h: bool,
    pub reduced_motion: ReducedMotionPref,
    pub sidebar_width: u16,
    pub voice_call_height: u16,
    pub emoji_picker_width: u16,
    pub emoji_picker_height: u16,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemeKind::from_env(),
            notifications: true,
            sounds: true,
            locale: if cfg!(test) {
                "en-US".to_owned()
            } else {
                fluxer_client_i18n::I18n::from_system().locale().to_owned()
            },
            autostart: false,
            close_to_tray: true,
            font_size: DEFAULT_FONT_SIZE,
            ui_zoom: DEFAULT_UI_ZOOM,
            compact: false,
            trusted_domains: Vec::new(),
            voice_input_device: None,
            voice_output_device: None,
            voice_input_volume: DEFAULT_VOICE_VOLUME,
            voice_output_volume: DEFAULT_VOICE_VOLUME,
            voice_push_to_talk: false,
            voice_noise_suppression: true,
            voice_processing_mode: "voice".to_owned(),
            voice_echo_cancellation: true,
            voice_noise_suppression_backend: "standard".to_owned(),
            voice_auto_gain_control: true,
            voice_vad_auto_threshold: true,
            voice_vad_threshold: 50,
            voice_ptt_release_delay_ms: 250,
            camera_resolution: "720p".to_owned(),
            mirror_camera: false,
            streaming_mode: "screenshare".to_owned(),
            video_resolution: "1080p".to_owned(),
            video_fps: 60,
            video_codec: "auto".to_owned(),
            video_bitrate_kbps: 0,
            video_hardware_acceleration: true,
            allow_guild_dms: true,
            friend_request_access: "everyone".to_owned(),
            spoiler_display: "on_click".to_owned(),
            inline_media_previews: true,
            render_embeds: true,
            render_reactions: true,
            convert_emoticons: true,
            unread_badge_enabled: true,
            time_format_24h: false,
            reduced_motion: ReducedMotionPref::System,
            sidebar_width: SIDEBAR_WIDTH_DEFAULT,
            voice_call_height: VOICE_CALL_HEIGHT_DEFAULT,
            emoji_picker_width: EMOJI_PICKER_WIDTH_DEFAULT,
            emoji_picker_height: EMOJI_PICKER_HEIGHT_DEFAULT,
        }
    }
}

impl Prefs {
    pub fn display(&self) -> DisplayPrefs {
        DisplayPrefs {
            font_size: self.font_size,
            ui_zoom: self.ui_zoom,
            compact: self.compact,
        }
        .clamped()
    }

    fn from_value(value: &Value) -> Self {
        let default = Self::default();
        let mut trusted_domains = Vec::new();
        if let Some(items) = value.get("trusted_domains").and_then(Value::as_array) {
            for item in items.iter().filter_map(Value::as_str) {
                if trusted_domains::to_comparable_host(item) == trusted_domains::TRUST_ALL_PATTERN {
                    trusted_domains.clear();
                    trusted_domains.push(trusted_domains::TRUST_ALL_PATTERN.to_owned());
                    break;
                }
                let _ = trusted_domains::add_trusted_domain(&mut trusted_domains, item);
            }
        }
        Self {
            theme: match value.get("theme").and_then(Value::as_str) {
                Some(raw) => ThemeKind::parse(raw).unwrap_or(ThemeKind::Dark),
                None => default.theme,
            },
            notifications: value
                .get("notifications")
                .and_then(Value::as_bool)
                .unwrap_or(default.notifications),
            sounds: value
                .get("sounds")
                .and_then(Value::as_bool)
                .unwrap_or(default.sounds),
            locale: value
                .get("locale")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or(default.locale),
            autostart: value
                .get("autostart")
                .and_then(Value::as_bool)
                .unwrap_or(default.autostart),
            close_to_tray: value
                .get("close_to_tray")
                .and_then(Value::as_bool)
                .unwrap_or(default.close_to_tray),
            font_size: value
                .get("font_size")
                .and_then(Value::as_u64)
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(default.font_size)
                .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE),
            ui_zoom: value
                .get("ui_zoom")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(default.ui_zoom)
                .clamp(MIN_UI_ZOOM, MAX_UI_ZOOM),
            compact: value
                .get("compact")
                .and_then(Value::as_bool)
                .unwrap_or(default.compact),
            trusted_domains,
            voice_input_device: normalize_device_name(
                value.get("voice_input_device").and_then(Value::as_str),
            ),
            voice_output_device: normalize_device_name(
                value.get("voice_output_device").and_then(Value::as_str),
            ),
            voice_input_volume: value
                .get("voice_input_volume")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(default.voice_input_volume)
                .clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME),
            voice_output_volume: value
                .get("voice_output_volume")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(default.voice_output_volume)
                .clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME),
            voice_push_to_talk: value
                .get("voice_push_to_talk")
                .and_then(Value::as_bool)
                .unwrap_or(default.voice_push_to_talk),
            voice_noise_suppression: value
                .get("voice_noise_suppression")
                .and_then(Value::as_bool)
                .unwrap_or(default.voice_noise_suppression),
            voice_processing_mode: value
                .get("voice_processing_mode")
                .and_then(Value::as_str)
                .filter(|s| matches!(*s, "voice" | "studio" | "custom"))
                .map(str::to_owned)
                .unwrap_or(default.voice_processing_mode),
            voice_echo_cancellation: value
                .get("voice_echo_cancellation")
                .and_then(Value::as_bool)
                .unwrap_or(default.voice_echo_cancellation),
            voice_noise_suppression_backend: value
                .get("voice_noise_suppression_backend")
                .and_then(Value::as_str)
                .filter(|s| matches!(*s, "off" | "standard" | "deepfilternet"))
                .map(str::to_owned)
                .unwrap_or(default.voice_noise_suppression_backend),
            voice_auto_gain_control: value
                .get("voice_auto_gain_control")
                .and_then(Value::as_bool)
                .unwrap_or(default.voice_auto_gain_control),
            voice_vad_auto_threshold: value
                .get("voice_vad_auto_threshold")
                .and_then(Value::as_bool)
                .unwrap_or(default.voice_vad_auto_threshold),
            voice_vad_threshold: value
                .get("voice_vad_threshold")
                .and_then(Value::as_u64)
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(default.voice_vad_threshold)
                .clamp(0, 100),
            voice_ptt_release_delay_ms: value
                .get("voice_ptt_release_delay_ms")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(default.voice_ptt_release_delay_ms)
                .clamp(20, 2000),
            camera_resolution: value
                .get("camera_resolution")
                .and_then(Value::as_str)
                .filter(|s| matches!(*s, "480p" | "720p" | "1080p"))
                .map(str::to_owned)
                .unwrap_or(default.camera_resolution),
            mirror_camera: value
                .get("mirror_camera")
                .and_then(Value::as_bool)
                .unwrap_or(default.mirror_camera),
            streaming_mode: value
                .get("streaming_mode")
                .and_then(Value::as_str)
                .filter(|s| matches!(*s, "screenshare" | "gaming" | "custom"))
                .map(str::to_owned)
                .unwrap_or(default.streaming_mode),
            video_resolution: value
                .get("video_resolution")
                .and_then(Value::as_str)
                .filter(|s| matches!(*s, "480p" | "720p" | "1080p" | "1440p" | "source"))
                .map(str::to_owned)
                .unwrap_or(default.video_resolution),
            video_fps: value
                .get("video_fps")
                .and_then(Value::as_u64)
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| matches!(*n, 15 | 30 | 60))
                .unwrap_or(default.video_fps),
            video_codec: value
                .get("video_codec")
                .and_then(Value::as_str)
                .filter(|s| matches!(*s, "auto" | "h265" | "h264" | "vp9" | "vp8" | "av1"))
                .map(str::to_owned)
                .unwrap_or(default.video_codec),
            video_bitrate_kbps: value
                .get("video_bitrate_kbps")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(default.video_bitrate_kbps)
                .min(50_000),
            video_hardware_acceleration: value
                .get("video_hardware_acceleration")
                .and_then(Value::as_bool)
                .unwrap_or(default.video_hardware_acceleration),
            allow_guild_dms: value
                .get("allow_guild_dms")
                .and_then(Value::as_bool)
                .unwrap_or(default.allow_guild_dms),
            friend_request_access: value
                .get("friend_request_access")
                .and_then(Value::as_str)
                .filter(|s| matches!(*s, "everyone" | "friends_of_friends" | "no_one"))
                .map(str::to_owned)
                .unwrap_or(default.friend_request_access),
            spoiler_display: value
                .get("spoiler_display")
                .and_then(Value::as_str)
                .filter(|s| matches!(*s, "on_click" | "moderator" | "always"))
                .map(str::to_owned)
                .unwrap_or(default.spoiler_display),
            inline_media_previews: value
                .get("inline_media_previews")
                .and_then(Value::as_bool)
                .unwrap_or(default.inline_media_previews),
            render_embeds: value
                .get("render_embeds")
                .and_then(Value::as_bool)
                .unwrap_or(default.render_embeds),
            render_reactions: value
                .get("render_reactions")
                .and_then(Value::as_bool)
                .unwrap_or(default.render_reactions),
            convert_emoticons: value
                .get("convert_emoticons")
                .and_then(Value::as_bool)
                .unwrap_or(default.convert_emoticons),
            unread_badge_enabled: value
                .get("unread_badge_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(default.unread_badge_enabled),
            time_format_24h: value
                .get("time_format_24h")
                .and_then(Value::as_bool)
                .unwrap_or(default.time_format_24h),
            reduced_motion: value
                .get("reduced_motion")
                .and_then(Value::as_str)
                .map(ReducedMotionPref::from_str)
                .unwrap_or(default.reduced_motion),
            sidebar_width: value
                .get("sidebar_width")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(default.sidebar_width)
                .clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX),
            voice_call_height: value
                .get("voice_call_height")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(default.voice_call_height)
                .clamp(VOICE_CALL_HEIGHT_MIN, VOICE_CALL_HEIGHT_MAX),
            emoji_picker_width: value
                .get("emoji_picker_width")
                .and_then(Value::as_u64)
                .map(|n| snap_emoji_picker_width(n as f32, EMOJI_PICKER_WIDTH_MAX as f32))
                .unwrap_or(default.emoji_picker_width),
            emoji_picker_height: value
                .get("emoji_picker_height")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(default.emoji_picker_height)
                .clamp(EMOJI_PICKER_HEIGHT_MIN, EMOJI_PICKER_HEIGHT_MAX),
        }
    }

    fn to_value(&self) -> Value {
        let mut map = serde_json::Map::with_capacity(45);
        map.insert("theme".into(), json!(self.theme.as_str()));
        map.insert("notifications".into(), json!(self.notifications));
        map.insert("sounds".into(), json!(self.sounds));
        map.insert("locale".into(), json!(self.locale));
        map.insert("autostart".into(), json!(self.autostart));
        map.insert("close_to_tray".into(), json!(self.close_to_tray));
        map.insert("font_size".into(), json!(self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)));
        map.insert("ui_zoom".into(), json!(self.ui_zoom.clamp(MIN_UI_ZOOM, MAX_UI_ZOOM)));
        map.insert("compact".into(), json!(self.compact));
        map.insert("trusted_domains".into(), json!(self.trusted_domains));
        map.insert("voice_input_device".into(), json!(normalize_device_name(self.voice_input_device.as_deref())));
        map.insert("voice_output_device".into(), json!(normalize_device_name(self.voice_output_device.as_deref())));
        map.insert("voice_input_volume".into(), json!(self.voice_input_volume.clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME)));
        map.insert("voice_output_volume".into(), json!(self.voice_output_volume.clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME)));
        map.insert("voice_push_to_talk".into(), json!(self.voice_push_to_talk));
        map.insert("voice_noise_suppression".into(), json!(self.voice_noise_suppression));
        map.insert("voice_processing_mode".into(), json!(self.voice_processing_mode));
        map.insert("voice_echo_cancellation".into(), json!(self.voice_echo_cancellation));
        map.insert("voice_noise_suppression_backend".into(), json!(self.voice_noise_suppression_backend));
        map.insert("voice_auto_gain_control".into(), json!(self.voice_auto_gain_control));
        map.insert("voice_vad_auto_threshold".into(), json!(self.voice_vad_auto_threshold));
        map.insert("voice_vad_threshold".into(), json!(self.voice_vad_threshold.clamp(0, 100)));
        map.insert("voice_ptt_release_delay_ms".into(), json!(self.voice_ptt_release_delay_ms.clamp(20, 2000)));
        map.insert("camera_resolution".into(), json!(self.camera_resolution));
        map.insert("mirror_camera".into(), json!(self.mirror_camera));
        map.insert("streaming_mode".into(), json!(self.streaming_mode));
        map.insert("video_resolution".into(), json!(self.video_resolution));
        map.insert("video_fps".into(), json!(self.video_fps));
        map.insert("video_codec".into(), json!(self.video_codec));
        map.insert("video_bitrate_kbps".into(), json!(self.video_bitrate_kbps));
        map.insert("video_hardware_acceleration".into(), json!(self.video_hardware_acceleration));
        map.insert("allow_guild_dms".into(), json!(self.allow_guild_dms));
        map.insert("friend_request_access".into(), json!(self.friend_request_access));
        map.insert("spoiler_display".into(), json!(self.spoiler_display));
        map.insert("inline_media_previews".into(), json!(self.inline_media_previews));
        map.insert("render_embeds".into(), json!(self.render_embeds));
        map.insert("render_reactions".into(), json!(self.render_reactions));
        map.insert("convert_emoticons".into(), json!(self.convert_emoticons));
        map.insert("unread_badge_enabled".into(), json!(self.unread_badge_enabled));
        map.insert("time_format_24h".into(), json!(self.time_format_24h));
        map.insert("reduced_motion".into(), json!(self.reduced_motion.as_str()));
        map.insert("sidebar_width".into(), json!(self.sidebar_width.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX)));
        map.insert("voice_call_height".into(), json!(self.voice_call_height.clamp(VOICE_CALL_HEIGHT_MIN, VOICE_CALL_HEIGHT_MAX)));
        map.insert(
            "emoji_picker_width".into(),
            json!(snap_emoji_picker_width(self.emoji_picker_width as f32, EMOJI_PICKER_WIDTH_MAX as f32)),
        );
        map.insert(
            "emoji_picker_height".into(),
            json!(self.emoji_picker_height.clamp(EMOJI_PICKER_HEIGHT_MIN, EMOJI_PICKER_HEIGHT_MAX)),
        );
        Value::Object(map)
    }
}

#[derive(Debug, Default)]
pub struct PrefsStore {
    pub data: Prefs,
    path: Option<PathBuf>,
}

impl PrefsStore {
    /// Loads `path` (a missing or corrupt file falls back to the defaults)
    /// and keeps saving there. The locale is normalized to a supported code.
    pub fn open(path: PathBuf) -> Self {
        let mut data = match read(&path) {
            Ok(data) => data,
            Err(err) => {
                if path.exists() {
                    tracing::warn!(%err, "preferences not loaded");
                }
                Prefs::default()
            }
        };
        data.locale = fluxer_client_i18n::resolve_locale(&data.locale).to_owned();
        Self { data, path: Some(path) }
    }

    /// Atomically writes the preferences (a temporary file then a rename).
    pub fn save(&self) -> Result<()> {
        match &self.path {
            Some(path) => write(path, &self.data),
            None => Ok(()),
        }
    }

    pub fn set_theme(&mut self, theme: ThemeKind) {
        self.data.theme = theme;
        self.persist();
    }

    pub fn set_notifications(&mut self, enabled: bool) {
        self.data.notifications = enabled;
        self.persist();
    }

    pub fn set_sounds(&mut self, enabled: bool) {
        self.data.sounds = enabled;
        self.persist();
    }

    pub fn set_locale(&mut self, locale: &str) {
        self.data.locale = fluxer_client_i18n::resolve_locale(locale).to_owned();
        self.persist();
    }

    pub fn set_autostart(&mut self, enabled: bool) {
        self.data.autostart = enabled;
        self.persist();
    }

    pub fn set_close_to_tray(&mut self, enabled: bool) {
        self.data.close_to_tray = enabled;
        self.persist();
    }

    pub fn set_font_size(&mut self, font_size: u8) {
        self.data.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.persist();
    }

    pub fn set_ui_zoom(&mut self, ui_zoom: u16) {
        self.data.ui_zoom = ui_zoom.clamp(MIN_UI_ZOOM, MAX_UI_ZOOM);
        self.persist();
    }

    pub fn set_compact(&mut self, compact: bool) {
        self.data.compact = compact;
        self.persist();
    }

    pub fn set_voice_input_device(&mut self, device: Option<String>) {
        self.data.voice_input_device = normalize_device_name(device.as_deref());
        self.persist();
    }

    pub fn set_voice_output_device(&mut self, device: Option<String>) {
        self.data.voice_output_device = normalize_device_name(device.as_deref());
        self.persist();
    }

    pub fn set_voice_input_volume(&mut self, volume: u16) {
        self.data.voice_input_volume = volume.clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME);
        self.persist();
    }

    pub fn set_voice_output_volume(&mut self, volume: u16) {
        self.data.voice_output_volume = volume.clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME);
        self.persist();
    }

    pub fn set_voice_push_to_talk(&mut self, enabled: bool) {
        self.data.voice_push_to_talk = enabled;
        self.persist();
    }

    pub fn set_voice_noise_suppression(&mut self, enabled: bool) {
        self.data.voice_noise_suppression = enabled;
        self.persist();
    }

    pub fn set_sidebar_width(&mut self, width: u16) {
        self.data.sidebar_width = width.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
        self.persist();
    }

    pub fn set_voice_call_height(&mut self, height: u16) {
        self.data.voice_call_height = height.clamp(VOICE_CALL_HEIGHT_MIN, VOICE_CALL_HEIGHT_MAX);
        self.persist();
    }

    pub fn set_emoji_picker_size(&mut self, width: u16, height: u16) {
        self.data.emoji_picker_width = snap_emoji_picker_width(width as f32, EMOJI_PICKER_WIDTH_MAX as f32);
        self.data.emoji_picker_height = height.clamp(EMOJI_PICKER_HEIGHT_MIN, EMOJI_PICKER_HEIGHT_MAX);
        self.persist();
    }

    pub fn set_allow_guild_dms(&mut self, enabled: bool) {
        self.data.allow_guild_dms = enabled;
        self.persist();
    }

    pub fn set_inline_media_previews(&mut self, enabled: bool) {
        self.data.inline_media_previews = enabled;
        self.persist();
    }

    pub fn set_voice_processing_mode(&mut self, mode: &str) {
        if matches!(mode, "voice" | "studio" | "custom") {
            self.data.voice_processing_mode = mode.to_owned();
            if mode == "studio" {
                self.data.voice_noise_suppression = false;
                self.data.voice_echo_cancellation = false;
                self.data.voice_auto_gain_control = false;
            } else if mode == "voice" {
                self.data.voice_noise_suppression = true;
                self.data.voice_echo_cancellation = true;
                self.data.voice_auto_gain_control = true;
            }
            self.persist();
        }
    }

    pub fn set_voice_echo_cancellation(&mut self, enabled: bool) {
        self.data.voice_echo_cancellation = enabled;
        self.persist();
    }

    pub fn set_voice_noise_suppression_backend(&mut self, backend: &str) {
        if matches!(backend, "off" | "standard" | "deepfilternet") {
            self.data.voice_noise_suppression_backend = backend.to_owned();
            self.data.voice_noise_suppression = backend != "off";
            self.persist();
        }
    }

    pub fn set_voice_auto_gain_control(&mut self, enabled: bool) {
        self.data.voice_auto_gain_control = enabled;
        self.persist();
    }

    pub fn set_voice_vad_auto_threshold(&mut self, enabled: bool) {
        self.data.voice_vad_auto_threshold = enabled;
        self.persist();
    }

    pub fn set_voice_vad_threshold(&mut self, threshold: u8) {
        self.data.voice_vad_threshold = threshold.clamp(0, 100);
        self.persist();
    }

    pub fn set_voice_ptt_release_delay_ms(&mut self, delay_ms: u16) {
        self.data.voice_ptt_release_delay_ms = delay_ms.clamp(20, 2000);
        self.persist();
    }

    pub fn set_camera_resolution(&mut self, res: &str) {
        if matches!(res, "480p" | "720p" | "1080p") {
            self.data.camera_resolution = res.to_owned();
            self.persist();
        }
    }

    pub fn set_mirror_camera(&mut self, mirror: bool) {
        self.data.mirror_camera = mirror;
        self.persist();
    }

    pub fn set_streaming_mode(&mut self, mode: &str) {
        if matches!(mode, "screenshare" | "gaming" | "custom") {
            self.data.streaming_mode = mode.to_owned();
            if mode == "screenshare" {
                self.data.video_resolution = "1080p".to_owned();
                self.data.video_fps = 30;
            } else if mode == "gaming" {
                self.data.video_resolution = "1080p".to_owned();
                self.data.video_fps = 60;
            }
            self.persist();
        }
    }

    pub fn set_video_resolution(&mut self, res: &str) {
        if matches!(res, "480p" | "720p" | "1080p" | "1440p" | "source") {
            self.data.video_resolution = res.to_owned();
            self.data.streaming_mode = "custom".to_owned();
            self.persist();
        }
    }

    pub fn set_video_fps(&mut self, fps: u8) {
        if matches!(fps, 15 | 30 | 60) {
            self.data.video_fps = fps;
            self.data.streaming_mode = "custom".to_owned();
            self.persist();
        }
    }

    pub fn set_video_codec(&mut self, codec: &str) {
        if matches!(codec, "auto" | "h265" | "h264" | "vp9" | "vp8" | "av1") {
            self.data.video_codec = codec.to_owned();
            self.persist();
        }
    }

    pub fn set_video_bitrate_kbps(&mut self, kbps: u16) {
        self.data.video_bitrate_kbps = kbps.min(50_000);
        self.persist();
    }

    pub fn set_video_hardware_acceleration(&mut self, enabled: bool) {
        self.data.video_hardware_acceleration = enabled;
        self.persist();
    }

    pub fn set_friend_request_access(&mut self, access: &str) {
        if matches!(access, "everyone" | "friends_of_friends" | "no_one") {
            self.data.friend_request_access = access.to_owned();
            self.persist();
        }
    }

    pub fn set_spoiler_display(&mut self, mode: &str) {
        if matches!(mode, "on_click" | "moderator" | "always") {
            self.data.spoiler_display = mode.to_owned();
            self.persist();
        }
    }

    pub fn set_render_embeds(&mut self, enabled: bool) {
        self.data.render_embeds = enabled;
        self.persist();
    }

    pub fn set_render_reactions(&mut self, enabled: bool) {
        self.data.render_reactions = enabled;
        self.persist();
    }

    pub fn set_convert_emoticons(&mut self, enabled: bool) {
        self.data.convert_emoticons = enabled;
        self.persist();
    }

    pub fn set_unread_badge_enabled(&mut self, enabled: bool) {
        self.data.unread_badge_enabled = enabled;
        self.persist();
    }

    pub fn set_time_format_24h(&mut self, enabled: bool) {
        self.data.time_format_24h = enabled;
        self.persist();
    }

    pub fn set_reduced_motion(&mut self, pref: ReducedMotionPref) {
        self.data.reduced_motion = pref;
        self.persist();
    }

    pub fn add_trusted_domain(&mut self, host: &str) -> bool {
        if trusted_domains::add_trusted_domain(&mut self.data.trusted_domains, host) {
            self.persist();
            true
        } else {
            false
        }
    }

    pub fn clear_trusted_domains(&mut self) {
        if !self.data.trusted_domains.is_empty() {
            self.data.trusted_domains.clear();
            self.persist();
        }
    }

    pub fn set_trusted_domains(&mut self, domains: Vec<String>) {
        if self.data.trusted_domains != domains {
            self.data.trusted_domains = domains;
            self.persist();
        }
    }

    /// A failed save is only logged: prefs stay valid in memory.
    fn persist(&self) {
        if let Err(err) = self.save() {
            tracing::warn!(%err, "preferences not saved");
        }
    }
}

fn read(path: &Path) -> Result<Prefs> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)?;
    Ok(Prefs::from_value(&value))
}

/// Write to a temporary file, then rename over the old one.
fn write(path: &Path, data: &Prefs) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&data.to_value())?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fluxer-prefs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn round_trips_every_field_and_theme() {
        let dir = temp_dir("round-trip");
        let path = dir.join(FILE_NAME);
        for theme in [
            ThemeKind::Light,
            ThemeKind::Dark,
            ThemeKind::Darker,
            ThemeKind::Black,
            ThemeKind::System,
        ] {
            let mut store = PrefsStore::open(path.clone());
            store.data = Prefs {
                theme,
                notifications: theme != ThemeKind::Light,
                sounds: theme == ThemeKind::Black,
                locale: "ko".to_owned(),
                autostart: theme == ThemeKind::Darker,
                close_to_tray: theme != ThemeKind::System,
                font_size: if theme == ThemeKind::Light { 16 } else { 14 },
                ui_zoom: if theme == ThemeKind::Black { 120 } else { 100 },
                compact: theme == ThemeKind::Darker,
                trusted_domains: if theme == ThemeKind::System {
                    vec!["example.org".to_owned()]
                } else {
                    Vec::new()
                },
                voice_input_device: if theme == ThemeKind::Light {
                    Some("Built-in Mic".to_owned())
                } else {
                    None
                },
                voice_output_device: if theme == ThemeKind::Darker {
                    Some("Studio Headphones".to_owned())
                } else {
                    None
                },
                voice_input_volume: if theme == ThemeKind::Light { 150 } else { 80 },
                voice_output_volume: if theme == ThemeKind::Black { 200 } else { 40 },
                voice_push_to_talk: theme == ThemeKind::Light,
                voice_noise_suppression: theme != ThemeKind::Black,
                voice_processing_mode: if theme == ThemeKind::Light {
                    "custom".to_owned()
                } else {
                    "voice".to_owned()
                },
                voice_echo_cancellation: theme != ThemeKind::Darker,
                voice_noise_suppression_backend: if theme == ThemeKind::Black {
                    "deepfilternet".to_owned()
                } else {
                    "standard".to_owned()
                },
                voice_auto_gain_control: theme != ThemeKind::Light,
                voice_vad_auto_threshold: theme != ThemeKind::Black,
                voice_vad_threshold: if theme == ThemeKind::Light { 35 } else { 50 },
                voice_ptt_release_delay_ms: if theme == ThemeKind::Darker { 500 } else { 250 },
                camera_resolution: if theme == ThemeKind::Black {
                    "1080p".to_owned()
                } else {
                    "720p".to_owned()
                },
                mirror_camera: theme == ThemeKind::Light,
                streaming_mode: if theme == ThemeKind::Black {
                    "gaming".to_owned()
                } else {
                    "screenshare".to_owned()
                },
                video_resolution: if theme == ThemeKind::Light {
                    "1440p".to_owned()
                } else {
                    "1080p".to_owned()
                },
                video_fps: if theme == ThemeKind::Darker { 30 } else { 60 },
                video_codec: if theme == ThemeKind::Black {
                    "h265".to_owned()
                } else {
                    "auto".to_owned()
                },
                video_bitrate_kbps: if theme == ThemeKind::Light { 8000 } else { 0 },
                video_hardware_acceleration: theme != ThemeKind::Darker,
                allow_guild_dms: theme != ThemeKind::Darker,
                friend_request_access: if theme == ThemeKind::Light {
                    "friends_of_friends".to_owned()
                } else {
                    "everyone".to_owned()
                },
                spoiler_display: if theme == ThemeKind::Black {
                    "always".to_owned()
                } else {
                    "on_click".to_owned()
                },
                inline_media_previews: theme != ThemeKind::Light,
                render_embeds: theme != ThemeKind::Darker,
                render_reactions: theme != ThemeKind::Black,
                convert_emoticons: theme != ThemeKind::Light,
                unread_badge_enabled: theme != ThemeKind::Darker,
                time_format_24h: theme == ThemeKind::Black,
                reduced_motion: match theme {
                    ThemeKind::Light => ReducedMotionPref::On,
                    ThemeKind::Dark => ReducedMotionPref::Off,
                    _ => ReducedMotionPref::System,
                },
                sidebar_width: if theme == ThemeKind::Light { 260 } else { 320 },
                voice_call_height: if theme == ThemeKind::Darker { 420 } else { 320 },
                emoji_picker_width: if theme == ThemeKind::Black { 640 } else { 592 },
                emoji_picker_height: if theme == ThemeKind::Light { 500 } else { 690 },
            };
            store.save().expect("save");
            let loaded = PrefsStore::open(path.clone());
            assert_eq!(loaded.data, store.data, "{theme:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_json_falls_back_to_defaults() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, b"not json").expect("write");
        assert_eq!(PrefsStore::open(path).data, Prefs::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_theme_falls_back_to_dark() {
        let dir = temp_dir("unknown-theme");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            br#"{"theme":"pink","notifications":false,"sounds":false,"locale":"ko"}"#,
        )
        .expect("write");
        let store = PrefsStore::open(path);
        assert_eq!(store.data.theme, ThemeKind::Dark);
        assert!(!store.data.notifications);
        assert!(!store.data.sounds);
        assert_eq!(store.data.locale, "ko");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creating_directories_on_save() {
        let dir = temp_dir("directories");
        let path = dir.join("nested").join("deeper").join(FILE_NAME);
        let mut store = PrefsStore::open(path.clone());
        store.set_theme(ThemeKind::System);
        assert!(path.is_file(), "save creates the parent directories");
        let loaded = PrefsStore::open(path);
        assert_eq!(loaded.data.theme, ThemeKind::System);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_four_key_json_uses_tray_and_autostart_defaults() {
        let dir = temp_dir("legacy-four-keys");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            br#"{"theme":"light","notifications":false,"sounds":true,"locale":"ko"}"#,
        )
        .expect("write");
        let store = PrefsStore::open(path);
        assert_eq!(store.data.theme, ThemeKind::Light);
        assert!(!store.data.notifications);
        assert!(store.data.sounds);
        assert_eq!(store.data.locale, "ko");
        assert!(!store.data.autostart);
        assert!(store.data.close_to_tray);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serialized_json_has_no_token_keys() {
        let dir = temp_dir("no-tokens");
        let path = dir.join(FILE_NAME);
        let store = PrefsStore::open(path.clone());
        store.save().expect("save");
        let text = std::fs::read_to_string(&path).expect("read");
        let value: Value = serde_json::from_str(&text).expect("json");
        let keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.len(), 45);
        for key in [
            "theme",
            "notifications",
            "sounds",
            "locale",
            "autostart",
            "close_to_tray",
            "font_size",
            "ui_zoom",
            "compact",
            "trusted_domains",
            "voice_input_device",
            "voice_output_device",
            "voice_input_volume",
            "voice_output_volume",
            "voice_push_to_talk",
            "voice_noise_suppression",
            "voice_processing_mode",
            "voice_echo_cancellation",
            "voice_noise_suppression_backend",
            "voice_auto_gain_control",
            "voice_vad_auto_threshold",
            "voice_vad_threshold",
            "voice_ptt_release_delay_ms",
            "camera_resolution",
            "mirror_camera",
            "streaming_mode",
            "video_resolution",
            "video_fps",
            "video_codec",
            "video_bitrate_kbps",
            "video_hardware_acceleration",
            "allow_guild_dms",
            "friend_request_access",
            "spoiler_display",
            "inline_media_previews",
            "render_embeds",
            "render_reactions",
            "convert_emoticons",
            "unread_badge_enabled",
            "time_format_24h",
            "reduced_motion",
            "sidebar_width",
            "voice_call_height",
            "emoji_picker_width",
            "emoji_picker_height",
        ] {
            assert!(keys.contains(&key), "{key}");
        }
        for forbidden in ["token", "password", "secret"] {
            assert!(!text.to_ascii_lowercase().contains(forbidden), "{forbidden}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clamps_display_prefs_and_round_trips_trusted_domains() {
        let dir = temp_dir("clamp-display");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            br#"{"font_size":4,"ui_zoom":999,"compact":true,"trusted_domains":["Docs.RS.","bad host","docs.rs"]}"#,
        )
        .expect("write");
        let mut store = PrefsStore::open(path.clone());
        assert_eq!(store.data.font_size, MIN_FONT_SIZE);
        assert_eq!(store.data.ui_zoom, MAX_UI_ZOOM);
        assert!(store.data.compact);
        assert_eq!(store.data.trusted_domains, vec!["docs.rs"]);

        assert!(store.add_trusted_domain("example.com"));
        assert!(!store.add_trusted_domain("EXAMPLE.COM"));
        let reloaded = PrefsStore::open(path.clone());
        assert_eq!(reloaded.data.trusted_domains, vec!["docs.rs", "example.com"]);
        store.clear_trusted_domains();
        assert!(PrefsStore::open(path).data.trusted_domains.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clamps_voice_volume_and_normalizes_empty_device() {
        let dir = temp_dir("voice-clamp");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            br#"{"voice_input_device":"","voice_output_device":"   ","voice_input_volume":999,"voice_output_volume":-10}"#,
        )
        .expect("write");
        let mut store = PrefsStore::open(path.clone());
        assert_eq!(store.data.voice_input_device, None);
        assert_eq!(store.data.voice_output_device, None);
        assert_eq!(store.data.voice_input_volume, MAX_VOICE_VOLUME);
        assert_eq!(store.data.voice_output_volume, DEFAULT_VOICE_VOLUME);

        store.set_voice_input_device(Some("".to_owned()));
        store.set_voice_output_device(Some("USB Headset".to_owned()));
        store.set_voice_input_volume(999);
        store.set_voice_output_volume(0);
        let reloaded = PrefsStore::open(path);
        assert_eq!(reloaded.data.voice_input_device, None);
        assert_eq!(reloaded.data.voice_output_device.as_deref(), Some("USB Headset"));
        assert_eq!(reloaded.data.voice_input_volume, MAX_VOICE_VOLUME);
        assert_eq!(reloaded.data.voice_output_volume, MIN_VOICE_VOLUME);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reduced_motion_pref_parses_and_falls_back_to_system() {
        assert_eq!(ReducedMotionPref::from_str("system"), ReducedMotionPref::System);
        assert_eq!(ReducedMotionPref::from_str("on"), ReducedMotionPref::On);
        assert_eq!(ReducedMotionPref::from_str("off"), ReducedMotionPref::Off);
        assert_eq!(ReducedMotionPref::from_str("unknown"), ReducedMotionPref::System);
        assert_eq!(ReducedMotionPref::from_str(""), ReducedMotionPref::System);

        assert_eq!(ReducedMotionPref::System.as_str(), "system");
        assert_eq!(ReducedMotionPref::On.as_str(), "on");
        assert_eq!(ReducedMotionPref::Off.as_str(), "off");

        let dir = temp_dir("reduced-motion-fallback");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, br#"{"reduced_motion":"bogus"}"#).expect("write");
        let mut store = PrefsStore::open(path.clone());
        assert_eq!(store.data.reduced_motion, ReducedMotionPref::System);
        store.set_reduced_motion(ReducedMotionPref::On);
        assert_eq!(PrefsStore::open(path).data.reduced_motion, ReducedMotionPref::On);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clamps_and_round_trips_resize_prefs() {
        assert_eq!(snap_emoji_picker_width(200.0, 994.0), 400);
        assert_eq!(snap_emoji_picker_width(592.0, 994.0), 592);
        assert_eq!(snap_emoji_picker_width(644.0, 994.0), 640);
        assert_eq!(snap_emoji_picker_width(730.0, 994.0), 736);
        assert_eq!(snap_emoji_picker_width(1200.0, 994.0), 976);

        let dir = temp_dir("resize-clamp");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            br#"{"sidebar_width":50,"voice_call_height":9999,"emoji_picker_width":650,"emoji_picker_height":100}"#,
        )
        .expect("write");
        let mut store = PrefsStore::open(path.clone());
        assert_eq!(store.data.sidebar_width, SIDEBAR_WIDTH_MIN);
        assert_eq!(store.data.voice_call_height, VOICE_CALL_HEIGHT_MAX);
        assert_eq!(store.data.emoji_picker_width, 640);
        assert_eq!(store.data.emoji_picker_height, EMOJI_PICKER_HEIGHT_MIN);

        store.set_sidebar_width(480);
        store.set_voice_call_height(590);
        store.set_emoji_picker_size(736, 686);
        let reloaded = PrefsStore::open(path);
        assert_eq!(reloaded.data.sidebar_width, 480);
        assert_eq!(reloaded.data.voice_call_height, 590);
        assert_eq!(reloaded.data.emoji_picker_width, 736);
        assert_eq!(reloaded.data.emoji_picker_height, 686);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
