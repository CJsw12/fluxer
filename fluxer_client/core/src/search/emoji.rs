//! Emoji data, search and frecency, plus the composer trigger parsers
//! (`SlashCommandUtils.ts:5-7`).
//!
//! TS: `UnicodeEmojis.ts` (data), `EmojiSearchIndex.ts` (six match tiers),
//! `UsageFrecency.ts` (7-day half-life), `AutocompleteOptionBuilders.ts`
//! (10 results, empty query = top 5 frecent).
//!
//! Not ported: n-gram prefilter (a linear scan over ~2000 emoji is fast
//! enough), `name::skin-tone-N` query names, emoji role restrictions and
//! animated gating (`GuildEmoji` carries neither).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::state::{ClientState, GuildEmoji, Snowflake, bits, has};

const EMOJI_JSON: &str = include_str!("../../data/emojis.json");

/// Result cap for `:shortcode` autocomplete (`AutocompleteOptionBuilders.ts`).
pub const AUTOCOMPLETE_LIMIT: usize = 10;
/// Empty `:` query shows this many frecent emoji.
pub const FRECENT_LIMIT: usize = 5;
/// `UsageFrecency.ts` constants.
pub const MAX_TRACKED_KEYS: usize = 200;
const MAX_USAGE_COUNT: u32 = 100_000;
const MAX_KEY_LEN: usize = 256;
const HALF_LIFE_MS: f64 = 7.0 * 86_400_000.0;

/// Picker categories in display order (`emojis.json` `categories`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EmojiCategory {
    People,
    Nature,
    Food,
    Activity,
    Travel,
    Objects,
    Symbols,
    Flags,
}

impl EmojiCategory {
    pub const ALL: [EmojiCategory; 8] = [
        Self::People,
        Self::Nature,
        Self::Food,
        Self::Activity,
        Self::Travel,
        Self::Objects,
        Self::Symbols,
        Self::Flags,
    ];

    /// Key in `emojis.json`.
    pub fn key(self) -> &'static str {
        match self {
            Self::People => "people",
            Self::Nature => "nature",
            Self::Food => "food",
            Self::Activity => "activity",
            Self::Travel => "travel",
            Self::Objects => "objects",
            Self::Symbols => "symbols",
            Self::Flags => "flags",
        }
    }
}

/// One unicode emoji.
#[derive(Debug, Clone, PartialEq)]
pub struct UnicodeEmoji {
    /// First name, e.g. `grinning`.
    pub name: String,
    /// All shortcodes (first is `name`).
    pub names: Vec<String>,
    pub keywords: Vec<String>,
    pub surrogates: String,
    pub category: EmojiCategory,
    /// Skin-tone variants in tone order 1..=5 (light .. dark); empty if none.
    pub skins: Vec<String>,
    /// Position in the table (stable tiebreak).
    pub order: usize,
}

impl UnicodeEmoji {
    /// Surrogates for skin tone `tone` (1..=5), else the base emoji.
    pub fn with_skin_tone(&self, tone: usize) -> &str {
        tone.checked_sub(1)
            .and_then(|i| self.skins.get(i))
            .map_or(self.surrogates.as_str(), String::as_str)
    }
}

#[derive(Deserialize)]
struct RawSkin {
    surrogates: String,
}

#[derive(Deserialize)]
struct RawEmoji {
    #[serde(default)]
    names: Vec<String>,
    surrogates: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    skins: Vec<RawSkin>,
}

#[derive(Deserialize)]
struct RawData {
    #[serde(default)]
    shortcuts: BTreeMap<String, String>,
    #[serde(default)]
    categories: BTreeMap<String, Vec<RawEmoji>>,
}

struct EmojiData {
    emojis: Vec<UnicodeEmoji>,
    shortcuts: BTreeMap<String, String>,
}

fn data() -> &'static EmojiData {
    static DATA: OnceLock<EmojiData> = OnceLock::new();
    DATA.get_or_init(|| {
        let raw: RawData = serde_json::from_str(EMOJI_JSON).unwrap_or_else(|err| {
            tracing::error!(%err, "embedded emojis.json is invalid");
            RawData { shortcuts: BTreeMap::new(), categories: BTreeMap::new() }
        });
        let mut emojis = Vec::new();
        for category in EmojiCategory::ALL {
            for e in raw.categories.get(category.key()).into_iter().flatten() {
                let Some(name) = e.names.first() else { continue };
                emojis.push(UnicodeEmoji {
                    name: name.clone(),
                    names: e.names.clone(),
                    keywords: e.keywords.clone(),
                    surrogates: e.surrogates.clone(),
                    category,
                    skins: e.skins.iter().map(|s| s.surrogates.clone()).collect(),
                    order: emojis.len(),
                });
            }
        }
        EmojiData { emojis, shortcuts: raw.shortcuts }
    })
}

/// Every unicode emoji, picker order (parsed on first use).
pub fn unicode_emojis() -> &'static [UnicodeEmoji] {
    &data().emojis
}

/// Unicode emoji of one picker category.
pub fn unicode_emojis_in(category: EmojiCategory) -> impl Iterator<Item = &'static UnicodeEmoji> {
    unicode_emojis().iter().filter(move |e| e.category == category)
}

/// Emoticon (`<3`, `:)`) to emoji name (`heart`), `shortcuts` in the data.
pub fn emoticon_name(emoticon: &str) -> Option<&'static str> {
    data().shortcuts.get(emoticon).map(String::as_str)
}

/// Unicode emoji by any shortcode (exact, case-insensitive).
pub fn unicode_by_name(name: &str) -> Option<&'static UnicodeEmoji> {
    let lower = name.to_ascii_lowercase();
    unicode_emojis().iter().find(|e| e.names.iter().any(|n| n.to_ascii_lowercase() == lower))
}

// ---- frecency ---------------------------------------------------------

/// Usage of one emoji key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEntry {
    pub count: u32,
    pub last_used_ms: u64,
}

/// Persisted emoji usage (`UsageFrecency.ts`). Keys are
/// [`EmojiHit::frecency_key`]. The app serializes this struct as it likes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmojiFrecency {
    pub entries: BTreeMap<String, UsageEntry>,
}

/// `log2(count + 1) * 0.5^(age / 7 days)`.
fn frecency_score(entry: &UsageEntry, now_ms: u64) -> f64 {
    let age = now_ms.saturating_sub(entry.last_used_ms) as f64;
    f64::from(entry.count + 1).log2() * 0.5_f64.powf(age / HALF_LIFE_MS)
}

impl EmojiFrecency {
    pub fn score(&self, key: &str, now_ms: u64) -> f64 {
        self.entries.get(key).map_or(0.0, |e| frecency_score(e, now_ms))
    }

    /// Count a use of `key` at `now_ms`. Invalid keys are ignored; beyond
    /// [`MAX_TRACKED_KEYS`] the lowest-scored entries are dropped.
    pub fn record(&mut self, key: &str, now_ms: u64) {
        if key.is_empty() || key.len() > MAX_KEY_LEN {
            return;
        }
        let entry = self.entries.entry(key.to_owned()).or_insert(UsageEntry { count: 0, last_used_ms: now_ms });
        entry.count = (entry.count + 1).min(MAX_USAGE_COUNT);
        entry.last_used_ms = now_ms;
        self.prune(now_ms);
    }

    /// Keys best first (score, then recency, count, key).
    pub fn ranked_keys(&self, now_ms: u64) -> Vec<&str> {
        let mut ranked: Vec<(&String, &UsageEntry, f64)> =
            self.entries.iter().map(|(k, e)| (k, e, frecency_score(e, now_ms))).collect();
        ranked.sort_by(|a, b| {
            b.2.total_cmp(&a.2)
                .then(b.1.last_used_ms.cmp(&a.1.last_used_ms))
                .then(b.1.count.cmp(&a.1.count))
                .then(a.0.cmp(b.0))
        });
        ranked.into_iter().map(|(k, _, _)| k.as_str()).collect()
    }

    /// Drop invalid entries and keep the best [`MAX_TRACKED_KEYS`]
    /// (`sanitizeUsageMap`); call after loading from disk.
    pub fn prune(&mut self, now_ms: u64) {
        self.entries.retain(|k, e| !k.is_empty() && k.len() <= MAX_KEY_LEN && e.count >= 1);
        for e in self.entries.values_mut() {
            e.count = e.count.min(MAX_USAGE_COUNT);
            e.last_used_ms = e.last_used_ms.min(now_ms);
        }
        if self.entries.len() > MAX_TRACKED_KEYS {
            let keep: Vec<String> = self.ranked_keys(now_ms).into_iter().take(MAX_TRACKED_KEYS).map(str::to_owned).collect();
            self.entries.retain(|k, _| keep.contains(k));
        }
    }
}

// ---- search -------------------------------------------------------------

/// A search result.
#[derive(Debug, Clone, PartialEq)]
pub enum EmojiHit<'a> {
    Unicode(&'a UnicodeEmoji),
    Custom { guild_id: Snowflake, emoji: &'a GuildEmoji },
}

impl EmojiHit<'_> {
    /// Text inserted into the composer: the glyph, or `<:name:id>` /
    /// `<a:name:id>` for custom emoji.
    pub fn insert_text(&self) -> String {
        match self {
            Self::Unicode(e) => e.surrogates.clone(),
            Self::Custom { emoji, .. } => {
                let a = if emoji.animated { "a" } else { "" };
                format!("<{a}:{}:{}>", emoji.name, emoji.id)
            }
        }
    }

    /// Key in [`EmojiFrecency`]: the glyph, or `custom:<id>`.
    pub fn frecency_key(&self) -> String {
        match self {
            Self::Unicode(e) => e.surrogates.clone(),
            Self::Custom { emoji, .. } => format!("custom:{}", emoji.id),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Unicode(e) => &e.name,
            Self::Custom { emoji, .. } => &emoji.name,
        }
    }
}

/// `MATCH_TIER`; lower is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchTier {
    ExactName,
    NameStartsWith,
    NameBoundary,
    NameContains,
    KeywordStartsWith,
    KeywordContains,
}

fn compact(s: &str) -> String {
    s.chars().filter(|c| *c != '_' && *c != '-').collect()
}

/// A name boundary is the start/end, `_`, or an ASCII uppercase letter.
fn is_boundary(c: Option<char>) -> bool {
    c.is_none_or(|c| c == '_' || c.is_ascii_uppercase())
}

fn has_boundary_match(name: &str, lower_name: &str, query: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    let lower: Vec<char> = lower_name.chars().collect();
    let q: Vec<char> = query.chars().collect();
    if q.is_empty() || q.len() > lower.len() {
        return false;
    }
    (0..=lower.len() - q.len()).any(|i| {
        if lower[i..i + q.len()] != q[..] || !is_boundary(i.checked_sub(1).and_then(|j| chars.get(j)).copied()) {
            return false;
        }
        let end = i + q.len();
        is_boundary(chars.get(end).copied()) || (lower.get(end) == Some(&'s') && is_boundary(chars.get(end + 1).copied()))
    })
}

/// `getEmojiMatchTier`. `query` is lowercased (ASCII).
pub fn match_tier(names: &[&str], keywords: &[&str], query: &str) -> Option<MatchTier> {
    let lower_names: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    let compact_query = compact(query);
    if lower_names.iter().any(|n| n == query) {
        return Some(MatchTier::ExactName);
    }
    let compact_names: Vec<String> = lower_names.iter().map(|n| compact(n)).collect();
    if lower_names.iter().zip(&compact_names).any(|(n, c)| {
        n.starts_with(query) || (!compact_query.is_empty() && c.starts_with(&compact_query))
    }) {
        return Some(MatchTier::NameStartsWith);
    }
    if names.iter().zip(&lower_names).any(|(n, l)| has_boundary_match(n, l, query)) {
        return Some(MatchTier::NameBoundary);
    }
    if lower_names.iter().zip(&compact_names).any(|(n, c)| {
        n.contains(query) || (!compact_query.is_empty() && c.contains(&compact_query))
    }) {
        return Some(MatchTier::NameContains);
    }
    let mut contains = false;
    for k in keywords {
        let k = k.to_ascii_lowercase();
        if k.starts_with(query) {
            return Some(MatchTier::KeywordStartsWith);
        }
        contains |= k.contains(query);
    }
    contains.then_some(MatchTier::KeywordContains)
}

/// Custom emoji the user may use in `channel_id`: the channel's own guild
/// always; other guilds need USE_EXTERNAL_EMOJIS there. In DMs or with no
/// channel every guild's emoji are allowed (TS `filterEmojisForAutocomplete`
/// differs for no-channel contexts: unverified).
fn usable_custom<'a>(
    state: &'a ClientState,
    channel_id: Option<Snowflake>,
) -> Vec<(Snowflake, &'a GuildEmoji)> {
    let channel = channel_id.and_then(|c| state.channels.get(&c));
    let own_guild = channel.and_then(|c| c.guild_id);
    let external_ok = match channel {
        Some(c) if c.guild_id.is_some() => has(state.channel_permissions(c.id), bits::USE_EXTERNAL_EMOJIS),
        _ => true,
    };
    state
        .emojis
        .iter()
        .filter(|(guild, _)| Some(**guild) == own_guild || external_ok)
        .flat_map(|(guild, list)| list.iter().map(move |e| (*guild, e)))
        .collect()
}

/// Search unicode and usable custom emoji. Order: tier, frecency (higher
/// first), name, table order. Empty `query` returns the most frecent
/// [`FRECENT_LIMIT`] (capped by `count`).
pub fn search_emojis<'a>(
    state: &'a ClientState,
    channel_id: Option<Snowflake>,
    query: &str,
    count: usize,
    frecency: &EmojiFrecency,
    now_ms: u64,
) -> Vec<EmojiHit<'a>> {
    let customs = usable_custom(state, channel_id);
    let query = query.to_ascii_lowercase();
    if query.is_empty() {
        return frecent_hits(&customs, frecency, now_ms, count.min(FRECENT_LIMIT));
    }

    let mut scored: Vec<(MatchTier, f64, String, usize, EmojiHit<'a>)> = Vec::new();
    for e in unicode_emojis() {
        let names: Vec<&str> = e.names.iter().map(String::as_str).collect();
        let keywords: Vec<&str> = e.keywords.iter().map(String::as_str).collect();
        if let Some(tier) = match_tier(&names, &keywords, &query) {
            let hit = EmojiHit::Unicode(e);
            let f = frecency.score(&hit.frecency_key(), now_ms);
            scored.push((tier, f, e.name.clone(), e.order, hit));
        }
    }
    let base = unicode_emojis().len();
    for (i, (guild_id, emoji)) in customs.iter().enumerate() {
        if let Some(tier) = match_tier(&[emoji.name.as_str()], &[], &query) {
            let hit = EmojiHit::Custom { guild_id: *guild_id, emoji };
            let f = frecency.score(&hit.frecency_key(), now_ms);
            scored.push((tier, f, emoji.name.clone(), base + i, hit));
        }
    }
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.total_cmp(&a.1))
            .then_with(|| a.2.to_lowercase().cmp(&b.2.to_lowercase()))
            .then(a.3.cmp(&b.3))
    });
    scored.into_iter().take(count).map(|s| s.4).collect()
}

fn frecent_hits<'a>(
    customs: &[(Snowflake, &'a GuildEmoji)],
    frecency: &EmojiFrecency,
    now_ms: u64,
    count: usize,
) -> Vec<EmojiHit<'a>> {
    frecency
        .ranked_keys(now_ms)
        .into_iter()
        .filter_map(|key| {
            if let Some(id) = key.strip_prefix("custom:") {
                let id = Snowflake::parse(id)?;
                customs.iter().find(|(_, e)| e.id == id).map(|(g, e)| EmojiHit::Custom { guild_id: *g, emoji: e })
            } else {
                unicode_emojis().iter().find(|e| e.surrogates == key).map(EmojiHit::Unicode)
            }
        })
        .take(count)
        .collect()
}

// ---- composer triggers --------------------------------------------------

/// The last whitespace-delimited word of the text before the cursor. The
/// TS triggers are `(^|\s)X(...)$`, i.e. the trigger char must start the
/// word.
fn last_word(text_before_cursor: &str) -> &str {
    text_before_cursor.rsplit(char::is_whitespace).next().unwrap_or("")
}

/// `:shortcode` trigger (`/(^|\s):([a-z0-9_+~-]{2,})$/i`): the query
/// without the colon. The replacement range is the colon plus the query:
/// `text.len() - query.len() - 1 .. text.len()`.
pub fn emoji_query(text_before_cursor: &str) -> Option<&str> {
    let query = last_word(text_before_cursor).strip_prefix(':')?;
    let valid = query.chars().count() >= 2
        && query.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '~' | '-'));
    valid.then_some(query)
}

/// `@mention` trigger (`/(^|\s)@(\S*)$/`): query after the `@` (may be empty).
pub fn mention_query(text_before_cursor: &str) -> Option<&str> {
    last_word(text_before_cursor).strip_prefix('@')
}

/// `#channel` trigger (`/(^|\s)#(\S*)$/`): query after the `#` (may be empty).
pub fn channel_query(text_before_cursor: &str) -> Option<&str> {
    last_word(text_before_cursor).strip_prefix('#')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::fixture::*;

    #[test]
    fn data_parses() {
        let all = unicode_emojis();
        assert_eq!(all.len(), 1940);
        assert!(all.iter().all(|e| !e.name.is_empty() && !e.surrogates.is_empty()));
        assert_eq!(unicode_by_name("heart").map(|e| e.category), Some(EmojiCategory::People));
        assert_eq!(emoticon_name("<3"), Some("heart"));
        let wave = unicode_by_name("wave");
        assert!(wave.is_some_and(|e| e.skins.len() == 5));
        let wave = wave.map(|e| (e.surrogates.as_str(), e.with_skin_tone(3), e.with_skin_tone(0)));
        assert!(wave.is_some_and(|(base, t3, t0)| base != t3 && base == t0));
    }

    #[test]
    fn tiers() {
        assert_eq!(match_tier(&["smile"], &[], "smile"), Some(MatchTier::ExactName));
        assert_eq!(match_tier(&["smile_cat"], &[], "smi"), Some(MatchTier::NameStartsWith));
        assert_eq!(match_tier(&["smile_cat"], &[], "smilecat"), Some(MatchTier::NameStartsWith), "compact");
        assert_eq!(match_tier(&["big_smile"], &[], "smile"), Some(MatchTier::NameBoundary));
        assert_eq!(match_tier(&["big_smiles"], &[], "smile"), Some(MatchTier::NameBoundary), "plural s");
        assert_eq!(match_tier(&["xsmilex"], &[], "smile"), Some(MatchTier::NameContains));
        assert_eq!(match_tier(&["grin"], &["happy face"], "happ"), Some(MatchTier::KeywordStartsWith));
        assert_eq!(match_tier(&["grin"], &["very happy"], "happ"), Some(MatchTier::KeywordContains));
        assert_eq!(match_tier(&["grin"], &["joy"], "zzz"), None);
    }

    #[test]
    fn search_orders_by_tier_and_frecency() {
        let s = fixture_state();
        let none = EmojiFrecency::default();
        let hits = search_emojis(&s, Some(sf(GENERAL)), "heart", 10, &none, 0);
        assert_eq!(hits.first().map(EmojiHit::name), Some("heart"), "exact first");
        assert!(hits.len() > 1 && hits.len() <= 10);

        // frecency lifts an equal-tier result above its siblings
        let starts: Vec<EmojiHit<'_>> = search_emojis(&s, Some(sf(GENERAL)), "hea", 50, &none, 0);
        let target = starts.iter().rev().find(|h| matches!(h, EmojiHit::Unicode(_))).cloned();
        let target = target.unwrap_or_else(|| panic!("no unicode hit"));
        let mut f = EmojiFrecency::default();
        for _ in 0..5 {
            f.record(&target.frecency_key(), 1_000);
        }
        let reranked = search_emojis(&s, Some(sf(GENERAL)), "hea", 50, &f, 1_000);
        let tier_of = |h: &EmojiHit<'_>| match h {
            EmojiHit::Unicode(e) => {
                let n: Vec<&str> = e.names.iter().map(String::as_str).collect();
                let k: Vec<&str> = e.keywords.iter().map(String::as_str).collect();
                match_tier(&n, &k, "hea")
            }
            EmojiHit::Custom { .. } => None,
        };
        let first_same_tier = reranked.iter().find(|h| tier_of(h) == tier_of(&target));
        assert_eq!(first_same_tier, Some(&target));
    }

    #[test]
    fn custom_emoji_and_external_permission() {
        let s = fixture_state();
        let none = EmojiFrecency::default();
        let own = search_emojis(&s, Some(sf(GENERAL)), "partyparrot", 5, &none, 0);
        assert_eq!(own.first().map(EmojiHit::insert_text), Some("<a:partyparrot:9001>".to_owned()));
        let external = search_emojis(&s, Some(sf(GENERAL)), "betaemoji", 5, &none, 0);
        assert_eq!(external.first().map(EmojiHit::insert_text), Some("<:betaemoji:9003>".to_owned()), "default perms allow external");

        // deny USE_EXTERNAL_EMOJIS for @everyone in GENERAL's guild
        let mut s2 = fixture_state();
        if let Some(g) = s2.guilds.get_mut(&sf(GUILD))
            && let Some(r) = g.roles.get_mut(&sf(GUILD))
        {
            r.permissions &= !bits::USE_EXTERNAL_EMOJIS;
        }
        let blocked = search_emojis(&s2, Some(sf(GENERAL)), "betaemoji", 5, &none, 0);
        assert!(blocked.is_empty());
        let still_own = search_emojis(&s2, Some(sf(GENERAL)), "pepe", 5, &none, 0);
        assert_eq!(still_own.first().map(EmojiHit::name), Some("pepe"));
    }

    #[test]
    fn empty_query_returns_frecent() {
        let s = fixture_state();
        let mut f = EmojiFrecency::default();
        f.record("custom:9002", 10);
        f.record("custom:9002", 11);
        let heart = unicode_by_name("heart").map(|e| e.surrogates.clone()).unwrap_or_default();
        f.record(&heart, 12);
        let hits = search_emojis(&s, Some(sf(GENERAL)), "", 10, &f, 20);
        assert_eq!(hits.iter().map(EmojiHit::name).collect::<Vec<_>>(), vec!["pepe", "heart"]);
        assert!(search_emojis(&s, Some(sf(GENERAL)), "", 10, &EmojiFrecency::default(), 0).is_empty());
    }

    #[test]
    fn frecency_decay_and_bounds() {
        let day = 86_400_000;
        let mut f = EmojiFrecency::default();
        f.record("a", 0);
        let fresh = f.score("a", 0);
        let week = f.score("a", 7 * day);
        assert!((week - fresh / 2.0).abs() < 1e-9, "half-life is 7 days");
        assert!(f.score("missing", 0) == 0.0);
        f.record("", 0);
        f.record(&"x".repeat(257), 0);
        assert_eq!(f.entries.len(), 1);
        for i in 0..250 {
            f.record(&format!("k{i}"), 1_000 + i);
        }
        assert_eq!(f.entries.len(), MAX_TRACKED_KEYS);
        assert!(f.entries.contains_key("k249"), "newest kept");
        assert!(!f.entries.contains_key("k0"), "oldest dropped");
        let json = serde_json::to_string(&f).unwrap_or_default();
        assert_eq!(serde_json::from_str::<EmojiFrecency>(&json).ok(), Some(f));
    }

    #[test]
    fn emoji_trigger() {
        assert_eq!(emoji_query(":sm"), Some("sm"));
        assert_eq!(emoji_query("hello :smile"), Some("smile"));
        assert_eq!(emoji_query("a\n:ab"), Some("ab"));
        assert_eq!(emoji_query(":+1"), Some("+1"));
        assert_eq!(emoji_query(":a"), None, "needs two chars");
        assert_eq!(emoji_query("a:b"), None);
        assert_eq!(emoji_query("a:bc"), None, "colon must follow whitespace");
        assert_eq!(emoji_query("http://x"), None);
        assert_eq!(emoji_query(":smile: "), None);
        assert_eq!(emoji_query(":smile:"), None, "closing colon is not in the class");
        assert_eq!(emoji_query(""), None);
        assert_eq!(emoji_query(":sm ile"), None);
    }

    #[test]
    fn mention_and_channel_triggers() {
        assert_eq!(mention_query("@"), Some(""));
        assert_eq!(mention_query("hi @bo"), Some("bo"));
        assert_eq!(mention_query("a@b"), None);
        assert_eq!(mention_query("@bob "), None);
        assert_eq!(mention_query("mail me@x.com"), None);
        assert_eq!(mention_query("@bob#12"), Some("bob#12"));
        assert_eq!(channel_query("#"), Some(""));
        assert_eq!(channel_query("see #gen"), Some("gen"));
        assert_eq!(channel_query("issue#1"), None, "mid-word");
        assert_eq!(channel_query("#gen "), None);
    }
}
