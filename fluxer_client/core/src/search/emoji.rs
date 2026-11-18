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

use crate::state::{ClientState, Guild, GuildEmoji, Snowflake, bits, has};

const EMOJI_JSON: &str = include_str!("../../data/emojis.json");

/// Result cap for `:shortcode` autocomplete (`AutocompleteOptionBuilders.ts`).
pub const AUTOCOMPLETE_LIMIT: usize = 10;
/// Empty `:` query shows this many frecent emoji.
pub const FRECENT_LIMIT: usize = 5;
/// Number of frecent emoji the picker's empty state shows.
pub const PICKER_FRECENT_LIMIT: usize = 42;
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

/// An owned picker entry: a unicode emoji or one guild's custom emoji.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmojiChoice {
    pub name: String,
    pub shortcode: String,
    pub names: Vec<String>,
    pub glyph: String,
    pub skins: Vec<String>,
    pub custom_id: Option<Snowflake>,
    pub guild_id: Option<Snowflake>,
    pub animated: bool,
    pub category: Option<EmojiCategory>,
    pub frecency_key: String,
}

impl EmojiChoice {
    pub fn from_unicode(e: &UnicodeEmoji) -> Self {
        Self {
            name: e.name.clone(),
            shortcode: e.name.clone(),
            names: e.names.clone(),
            glyph: e.surrogates.clone(),
            skins: e.skins.clone(),
            custom_id: None,
            guild_id: None,
            animated: false,
            category: Some(e.category),
            frecency_key: e.surrogates.clone(),
        }
    }

    pub fn from_custom(guild_id: Snowflake, e: &GuildEmoji) -> Self {
        Self {
            name: e.name.clone(),
            shortcode: e.name.clone(),
            names: vec![e.name.clone()],
            glyph: String::new(),
            skins: Vec::new(),
            custom_id: Some(e.id),
            guild_id: Some(guild_id),
            animated: e.animated,
            category: None,
            frecency_key: format!("custom:{}", e.id),
        }
    }

    pub fn from_hit(hit: &EmojiHit<'_>) -> Self {
        match hit {
            EmojiHit::Unicode(e) => Self::from_unicode(e),
            EmojiHit::Custom { guild_id, emoji } => Self::from_custom(*guild_id, emoji),
        }
    }

    /// The glyph with skin tone `tone` (1..=5); custom emoji are unchanged.
    pub fn with_tone(&self, tone: usize) -> Self {
        let mut choice = self.clone();
        if choice.custom_id.is_none() && (1..=choice.skins.len()).contains(&tone) {
            choice.glyph = choice.skins[tone - 1].clone();
        }
        choice
    }

    pub fn partial_emoji(&self, tone: usize) -> crate::state::PartialEmoji {
        crate::state::PartialEmoji {
            id: self.custom_id,
            name: if self.custom_id.is_some() { self.name.clone() } else { self.with_tone(tone).glyph },
            animated: self.animated,
        }
    }

    /// Text inserted into the composer: `<:name:id>` / `<a:name:id>` for
    /// custom emoji, the (toned) glyph for unicode.
    pub fn insert_text(&self, tone: usize) -> String {
        if let Some(id) = self.custom_id {
            let prefix = if self.animated { "a" } else { "" };
            format!("<{prefix}:{}:{id}>", self.name)
        } else {
            self.with_tone(tone).glyph
        }
    }

    /// All shortcodes as `:name:` joined by spaces.
    pub fn label(&self) -> String {
        self.names.iter().map(|n| format!(":{n}:")).collect::<Vec<_>>().join(" ")
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

/// The channel's own guild and whether external guilds' emoji are allowed
/// there (DMs and no channel allow them).
fn channel_emoji_scope(state: &ClientState, channel_id: Option<Snowflake>) -> (Option<Snowflake>, bool) {
    let channel = channel_id.and_then(|c| state.channels.get(&c));
    let own_guild = channel.and_then(|c| c.guild_id);
    let external_ok = match channel {
        Some(c) if c.guild_id.is_some() => has(state.channel_permissions(c.id), bits::USE_EXTERNAL_EMOJIS),
        _ => true,
    };
    (own_guild, external_ok)
}

/// Custom emoji the user may use in `channel_id`: the channel's own guild
/// always; other guilds need USE_EXTERNAL_EMOJIS there. In DMs or with no
/// channel every guild's emoji are allowed (TS `filterEmojisForAutocomplete`
/// differs for no-channel contexts: unverified).
fn usable_custom<'a>(
    state: &'a ClientState,
    channel_id: Option<Snowflake>,
) -> Vec<(Snowflake, &'a GuildEmoji)> {
    let (own_guild, external_ok) = channel_emoji_scope(state, channel_id);
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

/// Picker's frecent choices: the channel's usable custom emoji frecency
/// intersected with [`EmojiFrecency`]. Deleted custom ids are skipped and
/// the result is capped at `limit`.
pub fn frecency_choices(
    state: &ClientState,
    channel_id: Option<Snowflake>,
    frecency: &EmojiFrecency,
    now_ms: u64,
    limit: usize,
) -> Vec<EmojiChoice> {
    let customs = usable_custom(state, channel_id);
    let mut choices: Vec<EmojiChoice> = frecent_hits(&customs, frecency, now_ms, limit)
        .iter()
        .map(EmojiChoice::from_hit)
        .filter(|c| c.custom_id.is_none_or(|id| customs.iter().any(|(_, e)| e.id == id)))
        .collect();
    choices.truncate(limit);
    choices
}

/// Usable custom emoji grouped by guild: the channel's own guild first, then
/// [`ClientState::sorted_guilds`], external guilds only when the channel
/// allows them. Emojis within a guild are snowflake-descending.
pub fn custom_choices_by_guild(
    state: &ClientState,
    channel_id: Option<Snowflake>,
) -> Vec<(Snowflake, String, Vec<EmojiChoice>)> {
    let (own_guild, external_ok) = channel_emoji_scope(state, channel_id);
    let mut guilds: Vec<&Guild> = Vec::new();
    if let Some(id) = own_guild
        && let Some(guild) = state.guilds.get(&id)
    {
        guilds.push(guild);
    }
    for guild in state.sorted_guilds() {
        if Some(guild.id) == own_guild || !external_ok {
            continue;
        }
        guilds.push(guild);
    }
    guilds
        .into_iter()
        .filter_map(|guild| {
            let mut emojis: Vec<&GuildEmoji> = state.emojis.get(&guild.id).map(|list| list.iter().collect()).unwrap_or_default();
            emojis.sort_by(|a, b| b.id.0.cmp(&a.id.0));
            if emojis.is_empty() {
                return None;
            }
            let choices = emojis.into_iter().map(|e| EmojiChoice::from_custom(guild.id, e)).collect();
            Some((guild.id, guild.name.clone(), choices))
        })
        .collect()
}

/// Picker query normalization: emoticons map to their emoji name, else strip
/// surrounding colons and join words with `_`.
pub fn normalize_picker_query(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(name) = emoticon_name(trimmed) {
        return name.to_owned();
    }
    trimmed.trim_matches(':').trim().split_whitespace().collect::<Vec<_>>().join("_")
}

/// Picker search: an empty query is frecent; otherwise unicode plus usable
/// custom emoji scored like [`search_emojis`] but without its result cap.
pub fn picker_search_choices(
    state: &ClientState,
    channel_id: Option<Snowflake>,
    query: &str,
    frecency: &EmojiFrecency,
    now_ms: u64,
) -> Vec<EmojiChoice> {
    let query = normalize_picker_query(query).to_ascii_lowercase();
    if query.is_empty() {
        return frecency_choices(state, channel_id, frecency, now_ms, PICKER_FRECENT_LIMIT);
    }

    let mut scored: Vec<(MatchTier, f64, String, usize, EmojiChoice)> = Vec::new();
    for e in unicode_emojis() {
        let names: Vec<&str> = e.names.iter().map(String::as_str).collect();
        let keywords: Vec<&str> = e.keywords.iter().map(String::as_str).collect();
        if let Some(tier) = match_tier(&names, &keywords, &query) {
            let choice = EmojiChoice::from_unicode(e);
            let f = frecency.score(&choice.frecency_key, now_ms);
            scored.push((tier, f, choice.name.to_lowercase(), e.order, choice));
        }
    }
    let base = unicode_emojis().len();
    let custom_groups = custom_choices_by_guild(state, channel_id);
    let customs: Vec<EmojiChoice> = custom_groups.into_iter().flat_map(|(_, _, choices)| choices).collect();
    for (i, choice) in customs.into_iter().enumerate() {
        if let Some(tier) = match_tier(&[choice.name.as_str()], &[], &query) {
            let f = frecency.score(&choice.frecency_key, now_ms);
            scored.push((tier, f, choice.name.to_lowercase(), base + i, choice));
        }
    }
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.total_cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });
    scored.into_iter().map(|s| s.4).collect()
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

    fn sample_unicode() -> UnicodeEmoji {
        UnicodeEmoji {
            name: "wave".to_owned(),
            names: vec!["wave".to_owned(), "bye".to_owned()],
            keywords: Vec::new(),
            surrogates: "BASE".to_owned(),
            category: EmojiCategory::People,
            skins: vec!["T1".to_owned(), "T2".to_owned(), "T3".to_owned()],
            order: 0,
        }
    }

    fn sample_custom() -> GuildEmoji {
        GuildEmoji { id: sf(9001), name: "partyparrot".to_owned(), animated: true }
    }

    #[test]
    fn emoji_choice_methods() {
        let e = sample_unicode();
        let choice = EmojiChoice::from_unicode(&e);
        assert_eq!(choice.name, "wave");
        assert_eq!(choice.shortcode, "wave");
        assert_eq!(choice.names, vec!["wave", "bye"]);
        assert_eq!(choice.glyph, "BASE");
        assert_eq!(choice.skins, vec!["T1", "T2", "T3"]);
        assert_eq!(choice.custom_id, None);
        assert_eq!(choice.guild_id, None);
        assert!(!choice.animated);
        assert_eq!(choice.category, Some(EmojiCategory::People));
        assert_eq!(choice.frecency_key, "BASE");
        assert_eq!(choice.label(), ":wave: :bye:");

        assert_eq!(choice.with_tone(2).glyph, "T2");
        assert_eq!(choice.with_tone(0).glyph, "BASE");
        assert_eq!(choice.with_tone(4).glyph, "BASE", "out of range keeps base");
        assert_eq!(choice.glyph, "BASE", "original unchanged");

        let partial = choice.partial_emoji(2);
        assert_eq!(partial.id, None);
        assert_eq!(partial.name, "T2");
        assert!(!partial.animated);
        assert_eq!(choice.insert_text(2), "T2");

        let custom = EmojiChoice::from_custom(sf(GUILD), &sample_custom());
        assert_eq!(custom.name, "partyparrot");
        assert_eq!(custom.shortcode, "partyparrot");
        assert_eq!(custom.names, vec!["partyparrot"]);
        assert!(custom.glyph.is_empty());
        assert!(custom.skins.is_empty());
        assert_eq!(custom.custom_id, Some(sf(9001)));
        assert_eq!(custom.guild_id, Some(sf(GUILD)));
        assert!(custom.animated);
        assert_eq!(custom.category, None);
        assert_eq!(custom.frecency_key, "custom:9001");
        assert_eq!(custom.with_tone(3).glyph, "");
        assert_eq!(custom.insert_text(3), "<a:partyparrot:9001>");
        assert_eq!(custom.partial_emoji(3).id, Some(sf(9001)));
        assert_eq!(custom.partial_emoji(3).name, "partyparrot");
        assert!(custom.partial_emoji(3).animated);
        assert_eq!(custom.label(), ":partyparrot:");

        let plain = EmojiChoice::from_custom(sf(GUILD), &GuildEmoji { id: sf(9002), name: "pepe".to_owned(), animated: false });
        assert_eq!(plain.insert_text(0), "<:pepe:9002>");
    }

    #[test]
    fn frecency_choices_cap_and_skip_deleted() {
        let s = fixture_state();
        let mut f = EmojiFrecency::default();
        f.record("custom:9999", 1_000);
        for i in 0..60 {
            f.record(&unicode_emojis()[i].surrogates, 1_000);
        }
        let choices = frecency_choices(&s, Some(sf(GENERAL)), &f, 2_000, PICKER_FRECENT_LIMIT);
        assert_eq!(choices.len(), PICKER_FRECENT_LIMIT);
        assert!(choices.iter().all(|c| c.custom_id != Some(sf(9999))), "deleted custom id skipped");
        assert_eq!(frecency_choices(&s, Some(sf(GENERAL)), &f, 2_000, 10).len(), 10);
    }

    #[test]
    fn custom_choices_grouped_by_guild() {
        let s = fixture_state();
        let groups = custom_choices_by_guild(&s, Some(sf(GENERAL)));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, sf(GUILD));
        assert_eq!(groups[0].1, "Alpha");
        assert_eq!(
            groups[0].2.iter().map(|c| c.custom_id).collect::<Vec<_>>(),
            vec![Some(sf(9002)), Some(sf(9001))],
            "own guild first, snowflake descending"
        );
        assert_eq!(groups[1].0, sf(GUILD2));
        assert_eq!(groups[1].2.iter().map(|c| c.custom_id).collect::<Vec<_>>(), vec![Some(sf(9003))]);

        // deny USE_EXTERNAL_EMOJIS for @everyone in GENERAL's guild
        let mut denied = fixture_state();
        if let Some(g) = denied.guilds.get_mut(&sf(GUILD))
            && let Some(r) = g.roles.get_mut(&sf(GUILD))
        {
            r.permissions &= !bits::USE_EXTERNAL_EMOJIS;
        }
        let only_own = custom_choices_by_guild(&denied, Some(sf(GENERAL)));
        assert_eq!(only_own.iter().map(|g| g.0).collect::<Vec<_>>(), vec![sf(GUILD)]);
        assert!(only_own.iter().all(|g| g.2.iter().all(|c| c.guild_id == Some(sf(GUILD)))));
    }

    #[test]
    fn picker_query_normalization() {
        assert_eq!(normalize_picker_query(":smile:"), "smile");
        assert_eq!(normalize_picker_query("hello world"), "hello_world");
        assert_eq!(normalize_picker_query("  hello   world  "), "hello_world");
        assert_eq!(normalize_picker_query(":+1"), "+1");
        assert_eq!(normalize_picker_query("<3"), "heart");
        assert_eq!(normalize_picker_query(""), "");
    }

    #[test]
    fn picker_search_choices_tiers_and_frecency() {
        let s = fixture_state();
        let none = EmojiFrecency::default();
        let heart = picker_search_choices(&s, Some(sf(GENERAL)), "heart", &none, 0);
        assert_eq!(heart.first().map(|c| c.name.as_str()), Some("heart"));

        let custom = picker_search_choices(&s, Some(sf(GENERAL)), ":partyparrot:", &none, 0);
        assert_eq!(custom.iter().map(|c| c.custom_id).collect::<Vec<_>>(), vec![Some(sf(9001))]);

        let many = picker_search_choices(&s, Some(sf(GENERAL)), "a", &none, 0);
        assert!(many.len() > AUTOCOMPLETE_LIMIT, "no 10-result cap");

        let heart_shortcut = picker_search_choices(&s, Some(sf(GENERAL)), "<3", &none, 0);
        assert!(heart_shortcut.iter().any(|c| c.name == "heart"));

        let mut f = EmojiFrecency::default();
        f.record("custom:9002", 10);
        f.record("custom:9002", 11);
        let glyph = unicode_by_name("heart").map(|e| e.surrogates.clone()).unwrap_or_default();
        f.record(&glyph, 12);
        let frecent = picker_search_choices(&s, Some(sf(GENERAL)), "   ", &f, 20);
        assert_eq!(frecent.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["pepe", "heart"]);
    }
}
