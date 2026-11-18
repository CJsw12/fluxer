//! fluxer_client_i18n: the official Fluxer Lingui catalogs embedded at build time.
//!
//! Message ids are the Lingui source strings (the English `msgid`, which may contain ICU
//! syntax such as `{name}` or `{count, plural, one {# item} other {# items}}`). A message with a
//! Lingui `context` uses the key `context` + U+0004 + `msgid`; use [`I18n::t_ctx`].
//! Untranslated or missing messages fall back to the source string, like the web app falls back
//! to `en-US`.

use std::borrow::Cow;
use std::sync::OnceLock;

mod icu;

/// Decompressed table: u32 LE triples (key index, blob start, blob end) plus the text blob.
struct Decoded {
    entries: Vec<u8>,
    blob: String,
}

impl Decoded {
    /// Decodes the build-time `u32 idx_len + idx + blob` payload after zstd decompression.
    fn from_zst(zst: &[u8]) -> Option<Self> {
        let raw = zstd::stream::decode_all(zst).ok()?;
        let idx_len = u32_at(&raw, 0)?;
        let idx_end = 4usize.checked_add(idx_len)?;
        let entries = raw.get(4..idx_end)?.to_vec();
        let blob = String::from_utf8(raw.get(idx_end..)?.to_vec()).ok()?;
        Some(Self { entries, blob })
    }
}

struct LocaleTable {
    code: &'static str,
    zst: &'static [u8],
    /// Filled on first lookup of this locale only.
    cell: OnceLock<Option<Decoded>>,
}

include!(concat!(env!("OUT_DIR"), "/locales.rs"));

static KEYS_ZST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/keys.zst"));
static KEYS: OnceLock<Option<Decoded>> = OnceLock::new();

const DEFAULT_LOCALE: &str = "en-US";

fn u32_at(bytes: &[u8], i: usize) -> Option<usize> {
    let b = bytes.get(i * 4..i * 4 + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

fn key_index(key: &str) -> Option<usize> {
    let keys = KEYS.get_or_init(|| Decoded::from_zst(KEYS_ZST)).as_ref()?;
    let n = keys.entries.len() / 4;
    let (mut lo, mut hi) = (0, n.saturating_sub(1));
    while lo < hi {
        let mid = (lo + hi) / 2;
        let s = keys.blob.get(u32_at(&keys.entries, mid)?..u32_at(&keys.entries, mid + 1)?)?;
        match s.cmp(key) {
            std::cmp::Ordering::Equal => return Some(mid),
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

impl LocaleTable {
    fn decoded(&'static self) -> Option<&'static Decoded> {
        self.cell.get_or_init(|| Decoded::from_zst(self.zst)).as_ref()
    }

    #[cfg(test)]
    fn is_loaded(&self) -> bool {
        self.cell.get().is_some()
    }

    fn get(&'static self, key_idx: usize) -> Option<&'static str> {
        let d = self.decoded()?;
        let (mut lo, mut hi) = (0, d.entries.len() / 12);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let k = u32_at(&d.entries, mid * 3)?;
            match k.cmp(&key_idx) {
                std::cmp::Ordering::Equal => {
                    let s = u32_at(&d.entries, mid * 3 + 1)?;
                    let e = u32_at(&d.entries, mid * 3 + 2)?;
                    return d.blob.get(s..e);
                }
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
            }
        }
        None
    }
}

fn table(code: &str) -> Option<&'static LocaleTable> {
    LOCALES.iter().find(|t| t.code == code)
}

/// Locale codes of the embedded catalogs (for example `ko`, `en-US`, `pt-BR`).
pub fn available_locales() -> Vec<&'static str> {
    LOCALES.iter().map(|t| t.code).collect()
}

/// Maps an arbitrary BCP-47 / POSIX locale name to a supported catalog code (default `en-US`).
pub fn resolve_locale(input: &str) -> &'static str {
    let cleaned = input.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return DEFAULT_LOCALE;
    }
    if let Some(t) = LOCALES.iter().find(|t| t.code.eq_ignore_ascii_case(cleaned)) {
        return t.code;
    }
    let mut parts = cleaned.split('-');
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest: Vec<String> = parts.map(|p| p.to_ascii_lowercase()).collect();
    let has = |x: &str| rest.iter().any(|p| p == x);
    let pick = match lang.as_str() {
        "zh" => {
            if has("tw") || has("hk") || has("mo") || has("hant") {
                "zh-TW"
            } else {
                "zh-CN"
            }
        }
        "es" => {
            if rest.is_empty() || has("es") {
                "es-ES"
            } else {
                "es-419"
            }
        }
        "pt" => {
            if has("pt") {
                "pt-PT"
            } else {
                "pt-BR"
            }
        }
        "en" => {
            if has("gb") {
                "en-GB"
            } else {
                "en-US"
            }
        }
        "nb" | "nn" => "no",
        "sv" => "sv-SE",
        "tl" => "fil",
        other => {
            return LOCALES
                .iter()
                .find(|t| t.code == other)
                .map(|t| t.code)
                .unwrap_or(DEFAULT_LOCALE);
        }
    };
    table(pick).map(|t| t.code).unwrap_or(DEFAULT_LOCALE)
}

/// Best-effort system locale name such as `ko-KR` (Windows API, then `LC_ALL`/`LANG` env).
pub fn system_locale() -> Option<String> {
    #[cfg(windows)]
    {
        let mut buf = [0u16; 85];
        // SAFETY: buf is a valid writable buffer of the length passed (LOCALE_NAME_MAX_LENGTH).
        let n = unsafe {
            windows_sys::Win32::Globalization::GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32)
        };
        if n > 1 {
            return Some(String::from_utf16_lossy(&buf[..(n as usize - 1)]));
        }
    }
    ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|v| v.split(':').next().unwrap_or("").to_string())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
}

/// Translator for one locale with an `en-US` (source string) fallback.
#[derive(Debug, Clone, Copy)]
pub struct I18n {
    locale: &'static str,
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(DEFAULT_LOCALE)
    }
}

impl I18n {
    pub fn new(locale: &str) -> Self {
        Self { locale: resolve_locale(locale) }
    }

    /// Uses the OS locale, falling back to `en-US`.
    pub fn from_system() -> Self {
        Self::new(&system_locale().unwrap_or_default())
    }

    pub fn locale(&self) -> &'static str {
        self.locale
    }

    fn lookup(&self, key: &str) -> Option<&'static str> {
        let idx = key_index(key)?;
        table(self.locale)
            .and_then(|t| t.get(idx))
            .or_else(|| table(DEFAULT_LOCALE).and_then(|t| t.get(idx)))
    }

    fn render<'a>(&self, source: &'a str, key: &str, args: &[(&str, &str)]) -> Cow<'a, str> {
        match self.lookup(key) {
            Some(msg) => Cow::Owned(icu::format(msg, args, self.locale).into_owned()),
            None => icu::format(source, args, "en-US"),
        }
    }

    /// Translates a source string; placeholders stay as `{name}`.
    pub fn t<'a>(&self, id: &'a str) -> Cow<'a, str> {
        self.render(id, id, &[])
    }

    /// Translates and substitutes ICU arguments (`{name}`, plural `{count, plural, ...}`).
    pub fn t_args<'a>(&self, id: &'a str, args: &[(&str, &str)]) -> Cow<'a, str> {
        self.render(id, id, args)
    }

    /// Translates a message that has a Lingui `context`.
    pub fn t_ctx<'a>(&self, ctx: &str, id: &'a str, args: &[(&str, &str)]) -> Cow<'a, str> {
        self.render(id, &format!("{ctx}\u{4}{id}"), args)
    }
}

/// `t!(i18n, "Log out")` or `t!(i18n, "Hello {name}", name = user)`.
#[macro_export]
macro_rules! t {
    ($i18n:expr, $id:expr) => {
        $i18n.t($id)
    };
    ($i18n:expr, $id:expr, $($name:ident = $val:expr),+ $(,)?) => {
        $i18n.t_args($id, &[$((stringify!($name), &$val.to_string() as &str)),+])
    };
}

#[cfg(test)]
mod tests;
