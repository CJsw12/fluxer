//! Device-local client preferences (`prefs.json`), kept next to the session
//! store. The file holds only the theme, notification/sound switches and the
//! locale; it never contains account tokens.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde_json::{Value, json};

use crate::theme::ThemeKind;

pub const FILE_NAME: &str = "prefs.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub theme: ThemeKind,
    pub notifications: bool,
    pub sounds: bool,
    pub locale: String,
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
        }
    }
}

impl Prefs {
    fn from_value(value: &Value) -> Self {
        let default = Self::default();
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
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "theme": self.theme.as_str(),
            "notifications": self.notifications,
            "sounds": self.sounds,
            "locale": self.locale,
        })
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
        assert_eq!(keys.len(), 4);
        for key in ["theme", "notifications", "sounds", "locale"] {
            assert!(keys.contains(&key), "{key}");
        }
        for forbidden in ["token", "password", "secret"] {
            assert!(!text.to_ascii_lowercase().contains(forbidden), "{forbidden}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
