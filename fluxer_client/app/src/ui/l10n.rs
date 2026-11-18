//! App-wide translator for the official Fluxer catalogs: one lazy global
//! [`I18n`], chosen from `FLUXER_LOCALE` or the system locale. Tests always
//! start in `en-US`, so the existing English assertions stay stable.

use std::sync::{OnceLock, RwLock};

use fluxer_client_i18n::I18n;

static TRANSLATOR: OnceLock<RwLock<I18n>> = OnceLock::new();

fn initial() -> I18n {
    #[cfg(test)]
    {
        return I18n::new("en-US");
    }
    #[cfg(not(test))]
    {
        match std::env::var("FLUXER_LOCALE") {
            Ok(code) if !code.trim().is_empty() => I18n::new(code.trim()),
            _ => I18n::from_system(),
        }
    }
}

fn translator() -> &'static RwLock<I18n> {
    TRANSLATOR.get_or_init(|| RwLock::new(initial()))
}

/// The active translator (a `Copy` snapshot).
pub fn current() -> I18n {
    translator().read().map(|guard| *guard).unwrap_or_default()
}

/// Translates `id` (a Lingui msgid) into the active locale.
pub fn tr(id: &str) -> String {
    current().t(id).into_owned()
}

/// Translates `id` and fills its ICU placeholders.
pub fn tr_args(id: &str, args: &[(&str, &str)]) -> String {
    current().t_args(id, args).into_owned()
}

/// Keeps the previous English wording for `en-US` while other locales use
/// the catalog. Only for messages whose English text is asserted verbatim
/// by tests that predate the catalogs.
pub fn tr_or_en(id: &str, english: &str) -> String {
    let i18n = current();
    if i18n.locale() == "en-US" { english.to_owned() } else { i18n.t(id).into_owned() }
}

/// [`tr_or_en`] with ICU arguments for the non-English catalogs.
pub fn tr_args_or_en(id: &str, english: &str, args: &[(&str, &str)]) -> String {
    let i18n = current();
    if i18n.locale() == "en-US" { english.to_owned() } else { i18n.t_args(id, args).into_owned() }
}

/// Switches the app-wide locale (for example `ko` or `en-US`).
#[allow(dead_code)]
pub fn set_locale(code: &str) {
    if let Ok(mut guard) = translator().write() {
        *guard = I18n::new(code);
    }
}
