//! Quick switcher (Ctrl+K) ranking over `ClientState`.
//!
//! TS: `QuickSwitcher.tsx:60-72` (prefix modes), `QuickSwitcherResultGenerators.ts`
//! (default sections, general search, caps), `QuickSwitcherCandidateBuilder.ts`
//! (sort weights), `SearchTextMatching.ts:30-47` (scores).
//!
//! Home / Favorites (`VIRTUAL_GUILD`) and settings-tab results are listed
//! only for a non-empty query (general search and `*`; settings only in
//! general search), scored on the title alone like `scoreNavigationCandidates`.
//! Not ported: settings subtabs, the DM-disabled runtime flag, Unicode
//! confusable matching, other-account results, forum/thread
//! channels, server-side member search. An empty search in a prefix mode
//! lists that category by sort weight (score 0), up to [`MODE_LIMIT`]
//! (assumed; TS source for that case not located).

use std::cmp::Reverse;
use std::collections::BTreeSet;

use crate::state::{
    ChannelKind, ClientState, RelationshipKind, Snowflake, message_notifications, parse_timestamp_ms,
};

pub const CATEGORY_LIMIT: usize = 5;
pub const MODE_LIMIT: usize = 100;
/// "Previous" shows 7 when alone, 3 when other sections exist; the first 3
/// are hidden from the other sections.
pub const PREVIOUS_ALONE: usize = 7;
pub const PREVIOUS_WITH_SECTIONS: usize = 3;
pub const PREVIOUS_HIDDEN_FROM_SECTIONS: usize = 3;
const HISTORY_LIMIT: usize = 8;
const UNREAD_WEIGHT_BOOST_MS: u64 = 7 * 86_400_000;

/// Prefix mode of a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    /// `@` (friends and current-community members) or `@@` (`all == true`).
    Users { all: bool },
    /// `#`
    TextChannels,
    /// `!`
    VoiceChannels,
    /// `*`
    Guilds,
}

/// A query split into its prefix mode and trimmed search text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedQuery<'a> {
    pub mode: Option<QueryMode>,
    pub search: &'a str,
}

/// `parsePrefix`: `@@` first, then the first character.
pub fn parse_query(query: &str) -> ParsedQuery<'_> {
    if let Some(rest) = query.strip_prefix("@@") {
        return ParsedQuery { mode: Some(QueryMode::Users { all: true }), search: rest.trim() };
    }
    let mode = match query.chars().next() {
        Some('@') => Some(QueryMode::Users { all: false }),
        Some('#') => Some(QueryMode::TextChannels),
        Some('!') => Some(QueryMode::VoiceChannels),
        Some('*') => Some(QueryMode::Guilds),
        _ => None,
    };
    let rest = if mode.is_some() { &query[1..] } else { query };
    ParsedQuery { mode, search: rest.trim() }
}

/// `scoreSearchTerm`: exact 10, starts-with 7, substring 5, every
/// comma/space-separated word present 3, subsequence 1 (if `fuzzy`), else 0.
/// Case-insensitive.
pub fn score_search_term(value: &str, query: &str, fuzzy: bool) -> u32 {
    let value = value.to_lowercase();
    let query = query.to_lowercase();
    if value.starts_with(&query) {
        return if value == query { 10 } else { 7 };
    }
    if value.contains(&query) {
        return 5;
    }
    let mut words = query.split([',', ' ']).filter(|w| !w.is_empty()).peekable();
    if words.peek().is_some() && words.all(|w| value.contains(w)) {
        return 3;
    }
    if fuzzy && is_subsequence(&query, &value) {
        return 1;
    }
    0
}

/// `fuzzySearch`: needle longer than haystack never matches; equal length
/// must be equal; else needle chars appear in order.
fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let (n, h) = (needle.chars().count(), haystack.chars().count());
    if n > h {
        return false;
    }
    if n == h {
        return needle == haystack;
    }
    let mut hay = haystack.chars();
    needle.chars().all(|c| hay.any(|x| x == c))
}

/// What a result opens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SwitcherTarget {
    /// A person; opens the DM when `dm_channel_id` is set (else creates it).
    User { user_id: Snowflake, dm_channel_id: Option<Snowflake> },
    GroupDm { channel_id: Snowflake },
    TextChannel { channel_id: Snowflake, guild_id: Option<Snowflake> },
    VoiceChannel { channel_id: Snowflake, guild_id: Option<Snowflake> },
    Guild { guild_id: Snowflake },
    /// Virtual guild `home` (direct messages).
    Home,
    /// Virtual guild `favorites`.
    Favorites,
    /// A settings tab; `id` is one of [`SETTINGS_TABS`] (official tab type).
    Settings { id: String },
}

impl SwitcherTarget {
    /// `type|id` dedupe key.
    fn key(&self) -> (u8, String) {
        match self {
            Self::User { user_id, .. } => (0, user_id.to_string()),
            Self::GroupDm { channel_id } => (1, channel_id.to_string()),
            Self::TextChannel { channel_id, .. } => (2, channel_id.to_string()),
            Self::VoiceChannel { channel_id, .. } => (3, channel_id.to_string()),
            Self::Guild { guild_id } => (4, guild_id.to_string()),
            Self::Home => (5, "home".to_owned()),
            Self::Favorites => (5, "favorites".to_owned()),
            Self::Settings { id } => (6, id.clone()),
        }
    }
}

/// Settings tabs the client implements: official tab type and label
/// (`ALL_TABS_DESCRIPTORS` order, `QuickSwitcherCandidateBuilder.ts:352-370`).
pub const SETTINGS_TABS: &[(&str, &str)] = &[
    ("my_profile", "Profile"),
    ("account_security", "Account"),
    ("privacy_safety", "Privacy"),
    ("linked_accounts", "Connections"),
    ("appearance", "Appearance"),
    ("themes", "Themes"),
    ("notifications", "Notifications"),
    ("chat_settings", "Chat"),
    ("voice_video", "Voice & video"),
    ("accessibility", "Accessibility"),
    ("language", "Language"),
    ("keybinds", "Keybinds"),
    ("desktop_settings", "Windows app"),
    ("advanced_settings", "Advanced"),
    ("applications", "Applications"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct SwitcherResult {
    pub target: SwitcherTarget,
    pub title: String,
    pub subtitle: Option<String>,
    /// 0 for the default (empty-query) sections.
    pub score: u32,
    /// Search value that produced the score.
    pub matched_text: String,
    pub sort_weight: u64,
}

/// Section header kinds (the app localizes the text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitcherHeader {
    Previous,
    Mentions,
    Unread,
    /// "Searching ..." header of a prefix mode.
    Searching(QueryMode),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SwitcherItem {
    Header(SwitcherHeader),
    Result(SwitcherResult),
}

/// Inputs the core does not own.
#[derive(Debug, Clone, Default)]
pub struct SwitcherContext {
    /// Visited channels, most recent first (app-persisted; may repeat).
    pub recent_visits: Vec<Snowflake>,
    pub current_channel: Option<Snowflake>,
    pub current_guild: Option<Snowflake>,
    pub now_ms: i64,
}

struct Candidate {
    target: SwitcherTarget,
    title: String,
    subtitle: Option<String>,
    search_values: Vec<String>,
    sort_weight: u64,
    is_channel_like: bool,
}

fn timestamp_or_zero(ms: Option<i64>) -> u64 {
    ms.and_then(|m| u64::try_from(m).ok()).unwrap_or(0)
}

/// `getChannelSortWeight`: last message (else channel) time, +7 days when
/// unread or mentioned.
pub fn channel_sort_weight(state: &ClientState, channel_id: Snowflake) -> u64 {
    let Some(channel) = state.channels.get(&channel_id) else { return 0 };
    let base = channel.last_message_id.unwrap_or(channel.id).timestamp_ms();
    let unread = state.channel_unread(channel_id);
    if unread.unread || unread.mention_count > 0 {
        base.saturating_add(UNREAD_WEIGHT_BOOST_MS)
    } else {
        base
    }
}

fn guild_name(state: &ClientState, guild_id: Option<Snowflake>) -> Option<String> {
    guild_id.and_then(|g| state.guilds.get(&g)).map(|g| g.name.clone())
}

fn user_candidate(
    state: &ClientState,
    user_id: Snowflake,
    dm_channel_id: Option<Snowflake>,
    nick: Option<&str>,
    weight: u64,
) -> Option<Candidate> {
    let user = state.users.get(&user_id).or_else(|| {
        // DM recipients may not be in `users`.
        dm_channel_id.and_then(|c| state.channels.get(&c)).and_then(|c| c.recipients.iter().find(|u| u.id == user_id))
    })?;
    let title = nick.filter(|n| !n.is_empty()).unwrap_or_else(|| user.display_name()).to_owned();
    let mut values = vec![title.clone(), user.username.clone(), user.id.to_string()];
    values.extend(nick.map(str::to_owned));
    Some(Candidate {
        target: SwitcherTarget::User { user_id, dm_channel_id },
        title,
        subtitle: Some(user.username.clone()),
        search_values: values,
        sort_weight: weight,
        is_channel_like: false,
    })
}

/// All user candidates (`buildCandidateSets`), one per user. `community`
/// limits to friends plus that guild's members (the `@` mode); `None` = all.
fn user_candidates(state: &ClientState, ctx: &SwitcherContext, community: Option<Snowflake>) -> Vec<Candidate> {
    let me = state.current_user_id();
    let mut out: Vec<Candidate> = Vec::new();
    let mut seen: BTreeSet<Snowflake> = BTreeSet::new();
    let member_of = |id: Snowflake| community.is_some_and(|g| state.members.get(&g).is_some_and(|m| m.contains_key(&id)));
    let allowed = |id: Snowflake| {
        community.is_none()
            || member_of(id)
            || state.relationships.get(&id).is_some_and(|r| r.kind() == RelationshipKind::Friend)
    };
    let nick_in_community = |id: Snowflake| {
        community.and_then(|g| state.members.get(&g)?.get(&id)).and_then(|m| m.nick.clone())
    };

    // People with a DM: keep the newest channel's weight.
    for channel in state.channels.values().filter(|c| c.kind == ChannelKind::Dm) {
        let Some(other) = channel.recipients.iter().find(|u| Some(u.id) != me).map(|u| u.id) else { continue };
        if !allowed(other) {
            continue;
        }
        let weight = channel_sort_weight(state, channel.id);
        let nick = nick_in_community(other);
        let Some(cand) = user_candidate(state, other, Some(channel.id), nick.as_deref(), weight) else { continue };
        if let Some(existing) = out.iter_mut().find(|c| c.target.key() == cand.target.key()) {
            if existing.sort_weight < weight {
                *existing = cand;
            }
        } else {
            seen.insert(other);
            out.push(cand);
        }
    }
    for rel in state.relationships.values().filter(|r| r.kind() == RelationshipKind::Friend) {
        if seen.insert(rel.id) {
            let weight = timestamp_or_zero(rel.since.as_deref().and_then(parse_timestamp_ms));
            let nick = nick_in_community(rel.id);
            if let Some(c) = user_candidate(state, rel.id, None, nick.as_deref(), weight) {
                out.push(c);
            }
        }
    }
    if community.is_none() {
        for user in state.users.values().filter(|u| Some(u.id) != me) {
            if seen.insert(user.id) {
                if let Some(c) = user_candidate(state, user.id, None, None, user.id.timestamp_ms()) {
                    out.push(c);
                }
            }
        }
    }
    let guild = community.or(ctx.current_guild);
    if let Some(members) = guild.and_then(|g| state.members.get(&g)) {
        for member in members.values().filter(|m| Some(m.user.id) != me) {
            if seen.insert(member.user.id) {
                let weight = timestamp_or_zero(member.joined_at.as_deref().and_then(parse_timestamp_ms));
                let title = member.nick.as_deref().filter(|n| !n.is_empty()).unwrap_or_else(|| member.user.display_name());
                let mut values = vec![title.to_owned(), member.user.username.clone(), member.user.id.to_string()];
                values.extend(member.nick.clone());
                out.push(Candidate {
                    target: SwitcherTarget::User { user_id: member.user.id, dm_channel_id: None },
                    title: title.to_owned(),
                    subtitle: Some(member.user.username.clone()),
                    search_values: values,
                    sort_weight: weight,
                    is_channel_like: false,
                });
            }
        }
    }
    out.retain(|c| matches!(&c.target, SwitcherTarget::User { user_id, .. } if Some(*user_id) != me));
    out
}

fn group_dm_candidates(state: &ClientState) -> Vec<Candidate> {
    let me = state.current_user_id();
    state
        .channels
        .values()
        .filter(|c| c.kind == ChannelKind::GroupDm)
        .map(|c| {
            let names: Vec<String> =
                c.recipients.iter().filter(|u| Some(u.id) != me).map(|u| u.display_name().to_owned()).collect();
            let title = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| names.join(", "));
            let mut values = vec![title.clone()];
            values.extend(names.iter().cloned());
            Candidate {
                target: SwitcherTarget::GroupDm { channel_id: c.id },
                title,
                subtitle: Some(names.join(", ")),
                search_values: values,
                sort_weight: channel_sort_weight(state, c.id),
                is_channel_like: true,
            }
        })
        .collect()
}

fn guild_channel_candidates(state: &ClientState, voice: bool) -> Vec<Candidate> {
    state
        .channels
        .values()
        .filter(|c| c.guild_id.is_some() && !c.kind.is_thread())
        .filter(|c| {
            if voice {
                c.kind == ChannelKind::GuildVoice
            } else {
                matches!(c.kind, ChannelKind::GuildText | ChannelKind::GuildAnnouncement)
            }
        })
        .filter(|c| state.can_view_channel(c.id))
        .map(|c| {
            let guild = guild_name(state, c.guild_id);
            let title = c.name.clone().unwrap_or_default();
            let mut values = vec![title.clone()];
            if !voice {
                values.extend(c.topic.clone());
            }
            values.extend(guild.clone());
            let target = if voice {
                SwitcherTarget::VoiceChannel { channel_id: c.id, guild_id: c.guild_id }
            } else {
                SwitcherTarget::TextChannel { channel_id: c.id, guild_id: c.guild_id }
            };
            Candidate {
                target,
                title,
                subtitle: guild,
                search_values: values,
                sort_weight: channel_sort_weight(state, c.id),
                is_channel_like: true,
            }
        })
        .collect()
}

fn guild_candidates(state: &ClientState, exclude: Option<Snowflake>) -> Vec<Candidate> {
    state
        .guilds
        .values()
        .filter(|g| Some(g.id) != exclude)
        .map(|g| Candidate {
            target: SwitcherTarget::Guild { guild_id: g.id },
            title: g.name.clone(),
            subtitle: None,
            search_values: vec![g.name.clone(), g.id.to_string()],
            sort_weight: timestamp_or_zero(g.joined_at.as_deref().and_then(parse_timestamp_ms)),
            is_channel_like: false,
        })
        .collect()
}

/// Home (always) and Favorites (only with favorites): `virtualGuildCandidates`.
fn virtual_guild_candidates(ctx: &SwitcherContext, has_favorites: bool) -> Vec<Candidate> {
    let weight = u64::try_from(ctx.now_ms).unwrap_or(0);
    let mut out = vec![Candidate {
        target: SwitcherTarget::Home,
        title: "Home".to_owned(),
        subtitle: Some("Direct messages".to_owned()),
        search_values: vec!["Home".to_owned()],
        sort_weight: weight,
        is_channel_like: false,
    }];
    if has_favorites {
        out.push(Candidate {
            target: SwitcherTarget::Favorites,
            title: "Favorites".to_owned(),
            subtitle: None,
            search_values: vec!["Favorites".to_owned()],
            sort_weight: weight,
            is_channel_like: false,
        });
    }
    out
}

fn settings_candidates() -> Vec<Candidate> {
    let tabs = SETTINGS_TABS
        .iter()
        .map(|(id, label)| Candidate {
            target: SwitcherTarget::Settings { id: (*id).to_owned() },
            title: (*label).to_owned(),
            subtitle: None,
            search_values: vec![(*label).to_owned()],
            sort_weight: 0,
            is_channel_like: false,
        });
    let sections = [
        ("privacy_safety", "Sensitive content", "Privacy"),
        ("notifications", "Text-to-speech notifications", "Notifications"),
    ]
    .into_iter()
    .map(|(id, title, tab)| Candidate {
        target: SwitcherTarget::Settings { id: id.to_owned() },
        title: title.to_owned(),
        subtitle: Some(tab.to_owned()),
        search_values: vec![title.to_owned(), "settings".to_owned()],
        sort_weight: 0,
        is_channel_like: false,
    });
    tabs.chain(sections).collect()
}

fn rank(cands: Vec<Candidate>, search: &str, limit: usize) -> Vec<SwitcherResult> {
    let channel_query = search.replace(' ', "-");
    let mut ranked: Vec<SwitcherResult> = cands
        .into_iter()
        .filter_map(|c| {
            let mut best = (0, String::new());
            for v in &c.search_values {
                let mut score = score_search_term(v, search, true);
                if c.is_channel_like && search.contains(' ') {
                    score = score.max(score_search_term(v, &channel_query, true));
                }
                if score > best.0 {
                    best = (score, v.clone());
                }
            }
            if search.is_empty() {
                best = (0, c.title.clone());
            } else if best.0 == 0 {
                return None;
            }
            Some(SwitcherResult {
                target: c.target,
                title: c.title,
                subtitle: c.subtitle,
                score: best.0,
                matched_text: best.1,
                sort_weight: c.sort_weight,
            })
        })
        .collect();
    ranked.sort_by(compare_results);
    ranked.truncate(limit);
    ranked
}

/// Score desc; equal-score users alphabetically by matched text; then
/// sort weight desc.
fn compare_results(a: &SwitcherResult, b: &SwitcherResult) -> std::cmp::Ordering {
    let both_users = matches!(a.target, SwitcherTarget::User { .. }) && matches!(b.target, SwitcherTarget::User { .. });
    b.score
        .cmp(&a.score)
        .then_with(|| {
            if both_users {
                a.matched_text.to_lowercase().cmp(&b.matched_text.to_lowercase())
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .then(b.sort_weight.cmp(&a.sort_weight))
}

/// Ranked results for a non-empty query without a prefix (`generateGeneralResults`).
fn general_results(
    state: &ClientState,
    ctx: &SwitcherContext,
    search: &str,
    has_favorites: bool,
) -> Vec<SwitcherResult> {
    let run = |limit: usize| -> [Vec<SwitcherResult>; 6] {
        [
            rank(user_candidates(state, ctx, None), search, limit),
            rank(group_dm_candidates(state), search, limit),
            rank(guild_channel_candidates(state, false), search, limit),
            rank(guild_candidates(state, ctx.current_guild), search, limit),
            rank(virtual_guild_candidates(ctx, has_favorites), search, limit),
            rank(settings_candidates(), search, limit),
        ]
    };
    let mut categories = run(CATEGORY_LIMIT);
    let non_empty: Vec<usize> = (0..categories.len()).filter(|i| !categories[*i].is_empty()).collect();
    if let [only] = non_empty[..] {
        let mut wide = run(MODE_LIMIT);
        categories[only] = std::mem::take(&mut wide[only]);
    }
    let mut seen = BTreeSet::new();
    let mut all: Vec<SwitcherResult> =
        categories.into_iter().flatten().filter(|r| seen.insert(r.target.key())).collect();
    all.sort_by(compare_results);
    all
}

/// A channel as a result row; `None` for kinds the switcher skips or
/// channels the user cannot view.
fn channel_result(state: &ClientState, channel_id: Snowflake) -> Option<SwitcherResult> {
    let channel = state.channels.get(&channel_id)?;
    if !state.can_view_channel(channel_id) {
        return None;
    }
    let weight = channel_sort_weight(state, channel_id);
    let guild = guild_name(state, channel.guild_id);
    let (target, title, subtitle) = match channel.kind {
        ChannelKind::Dm => {
            let me = state.current_user_id();
            let other = channel.recipients.iter().find(|u| Some(u.id) != me)?;
            let name = state.users.get(&other.id).unwrap_or(other).display_name().to_owned();
            (
                SwitcherTarget::User { user_id: other.id, dm_channel_id: Some(channel_id) },
                name,
                Some(other.username.clone()),
            )
        }
        ChannelKind::GroupDm => {
            let me = state.current_user_id();
            let names: Vec<String> =
                channel.recipients.iter().filter(|u| Some(u.id) != me).map(|u| u.display_name().to_owned()).collect();
            let title = channel.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| names.join(", "));
            (SwitcherTarget::GroupDm { channel_id }, title, Some(names.join(", ")))
        }
        ChannelKind::GuildText | ChannelKind::GuildAnnouncement => (
            SwitcherTarget::TextChannel { channel_id, guild_id: channel.guild_id },
            channel.name.clone().unwrap_or_default(),
            guild,
        ),
        ChannelKind::GuildVoice => (
            SwitcherTarget::VoiceChannel { channel_id, guild_id: channel.guild_id },
            channel.name.clone().unwrap_or_default(),
            guild,
        ),
        _ => return None,
    };
    Some(SwitcherResult { matched_text: title.clone(), target, title, subtitle, score: 0, sort_weight: weight })
}

/// The channel id a result stands for (a user result is its DM).
fn result_channel_id(r: &SwitcherResult) -> Option<Snowflake> {
    match r.target {
        SwitcherTarget::User { dm_channel_id, .. } => dm_channel_id,
        SwitcherTarget::GroupDm { channel_id }
        | SwitcherTarget::TextChannel { channel_id, .. }
        | SwitcherTarget::VoiceChannel { channel_id, .. } => Some(channel_id),
        SwitcherTarget::Guild { .. }
        | SwitcherTarget::Home
        | SwitcherTarget::Favorites
        | SwitcherTarget::Settings { .. } => None,
    }
}

fn collect_channels(state: &ClientState, ids: &[Snowflake], seen: &mut BTreeSet<Snowflake>) -> Vec<SwitcherResult> {
    let mut out = Vec::new();
    for id in ids {
        if seen.contains(id) {
            continue;
        }
        if let Some(result) = channel_result(state, *id) {
            seen.insert(*id);
            out.push(result);
        }
    }
    out
}

/// `isUnreadInCurrentCommunity`: viewable text/announcement channel, not
/// muted at guild, category or channel level, unread, level ALL_MESSAGES.
fn unread_in_current_guild(state: &ClientState, channel_id: Snowflake, guild_id: Snowflake, now_ms: i64) -> bool {
    let Some(c) = state.channels.get(&channel_id) else { return false };
    if !matches!(c.kind, ChannelKind::GuildText | ChannelKind::GuildAnnouncement)
        || c.guild_id != Some(guild_id)
        || !state.can_view_channel(c.id)
    {
        return false;
    }
    let g = Some(guild_id);
    let muted = state.is_guild_muted(g, now_ms)
        || c.parent_id.is_some_and(|p| state.is_channel_muted(g, p, now_ms))
        || state.is_channel_muted(g, c.id, now_ms);
    !muted
        && state.channel_unread(c.id).unread
        && crate::state::effective_notification_level(state, g, c.id) == message_notifications::ALL_MESSAGES
}

/// Results for an empty query: Previous, Mentions, Unread.
pub fn default_results(state: &ClientState, ctx: &SwitcherContext) -> Vec<SwitcherItem> {
    let mut history: Vec<Snowflake> = Vec::new();
    for id in &ctx.recent_visits {
        if Some(*id) != ctx.current_channel && !history.contains(id) {
            history.push(*id);
        }
    }
    history.truncate(HISTORY_LIMIT - 1);
    let previous = collect_channels(state, &history, &mut BTreeSet::new());

    let mut seen: BTreeSet<Snowflake> =
        previous.iter().take(PREVIOUS_HIDDEN_FROM_SECTIONS).filter_map(result_channel_id).collect();
    seen.extend(ctx.current_channel);

    let mut sections: Vec<SwitcherItem> = Vec::new();
    let mut mention_ids: Vec<(Snowflake, Snowflake)> = state
        .channels
        .values()
        .filter(|c| state.channel_unread(c.id).mention_count > 0)
        .map(|c| (c.last_message_id.unwrap_or(c.id), c.id))
        .collect();
    mention_ids.sort_by_key(|(last, id)| Reverse((*last, *id)));
    let mention_ids: Vec<Snowflake> = mention_ids.into_iter().map(|(_, id)| id).collect();
    let mentions = collect_channels(state, &mention_ids, &mut seen);
    if !mentions.is_empty() {
        sections.push(SwitcherItem::Header(SwitcherHeader::Mentions));
        sections.extend(mentions.into_iter().map(SwitcherItem::Result));
    }
    if let Some(guild_id) = ctx.current_guild {
        let mut unread: Vec<&crate::state::Channel> = state
            .channels
            .values()
            .filter(|c| unread_in_current_guild(state, c.id, guild_id, ctx.now_ms))
            .collect();
        unread.sort_by_key(|c| (c.position, c.id));
        let ids: Vec<Snowflake> = unread.iter().map(|c| c.id).collect();
        let unread = collect_channels(state, &ids, &mut seen);
        if !unread.is_empty() {
            sections.push(SwitcherItem::Header(SwitcherHeader::Unread));
            sections.extend(unread.into_iter().map(SwitcherItem::Result));
        }
    }
    let cap = if sections.is_empty() { PREVIOUS_ALONE } else { PREVIOUS_WITH_SECTIONS };
    let shown: Vec<SwitcherItem> = previous.into_iter().take(cap).map(SwitcherItem::Result).collect();
    if shown.is_empty() {
        return sections;
    }
    let mut out = vec![SwitcherItem::Header(SwitcherHeader::Previous)];
    out.extend(shown);
    out.extend(sections);
    out
}

/// Everything for the modal: empty query -> default sections; prefix mode
/// -> header plus up to [`MODE_LIMIT`] results; otherwise general search.
///
/// The client has no favorites store yet, so this lists no Favorites row
/// (TODO(backlog)); use [`quick_switcher_results_with`] once it has one.
pub fn quick_switcher_results(state: &ClientState, ctx: &SwitcherContext, query: &str) -> Vec<SwitcherItem> {
    quick_switcher_results_with(state, ctx, query, false)
}

/// [`quick_switcher_results`] with `Favorites.hasAnyFavorites`.
pub fn quick_switcher_results_with(
    state: &ClientState,
    ctx: &SwitcherContext,
    query: &str,
    has_favorites: bool,
) -> Vec<SwitcherItem> {
    let parsed = parse_query(query);
    let results: Vec<SwitcherResult> = match parsed.mode {
        Some(mode) => {
            let cands = match mode {
                QueryMode::Users { all } => {
                    let community = if all { None } else { ctx.current_guild };
                    user_candidates(state, ctx, community)
                }
                QueryMode::TextChannels => guild_channel_candidates(state, false),
                QueryMode::VoiceChannels => guild_channel_candidates(state, true),
                QueryMode::Guilds => {
                    let mut cands = guild_candidates(state, None);
                    if !parsed.search.is_empty() {
                        cands.extend(virtual_guild_candidates(ctx, has_favorites));
                    }
                    cands
                }
            };
            let mut out = vec![SwitcherItem::Header(SwitcherHeader::Searching(mode))];
            out.extend(rank(cands, parsed.search, MODE_LIMIT).into_iter().map(SwitcherItem::Result));
            return out;
        }
        None if parsed.search.is_empty() => return default_results(state, ctx),
        None => general_results(state, ctx, parsed.search, has_favorites),
    };
    results.into_iter().map(SwitcherItem::Result).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::fixture::*;
    use serde_json::json;

    fn results(items: &[SwitcherItem]) -> Vec<&SwitcherResult> {
        items.iter().filter_map(|i| if let SwitcherItem::Result(r) = i { Some(r) } else { None }).collect()
    }

    fn titles(items: &[SwitcherItem]) -> Vec<String> {
        results(items).iter().map(|r| r.title.clone()).collect()
    }

    fn ctx() -> SwitcherContext {
        SwitcherContext { current_guild: Some(sf(GUILD)), current_channel: Some(sf(GENERAL)), now_ms: 1_750_000_100_000, ..Default::default() }
    }

    #[test]
    fn score_table() {
        assert_eq!(score_search_term("General", "general", true), 10);
        assert_eq!(score_search_term("general-chat", "gen", true), 7);
        assert_eq!(score_search_term("my general", "gen", true), 5);
        assert_eq!(score_search_term("big red fox", "fox red", true), 3);
        assert_eq!(score_search_term("big red fox", "fox,red", true), 3);
        assert_eq!(score_search_term("general", "gnrl", true), 1);
        assert_eq!(score_search_term("general", "gnrl", false), 0);
        assert_eq!(score_search_term("abc", "abcd", true), 0, "needle longer");
        assert_eq!(score_search_term("abc", "xyz", true), 0);
        assert_eq!(score_search_term("anything", "", true), 7, "empty query is a prefix of everything");
    }

    #[test]
    fn prefix_parsing() {
        assert_eq!(parse_query("@@ bob"), ParsedQuery { mode: Some(QueryMode::Users { all: true }), search: "bob" });
        assert_eq!(parse_query("@bob"), ParsedQuery { mode: Some(QueryMode::Users { all: false }), search: "bob" });
        assert_eq!(parse_query("#gen").mode, Some(QueryMode::TextChannels));
        assert_eq!(parse_query("!voice").mode, Some(QueryMode::VoiceChannels));
        assert_eq!(parse_query("* alp"), ParsedQuery { mode: Some(QueryMode::Guilds), search: "alp" });
        assert_eq!(parse_query("plain"), ParsedQuery { mode: None, search: "plain" });
        assert_eq!(parse_query("@"), ParsedQuery { mode: Some(QueryMode::Users { all: false }), search: "" });
        assert_eq!(parse_query(""), ParsedQuery { mode: None, search: "" });
    }

    #[test]
    fn mode_searches() {
        let s = fixture_state();
        let c = ctx();
        let text = quick_switcher_results(&s, &c, "#staff");
        assert!(matches!(text[0], SwitcherItem::Header(SwitcherHeader::Searching(QueryMode::TextChannels))));
        // ME (helper, no mod role) cannot view staff-room
        assert!(titles(&text).is_empty());
        let text = quick_switcher_results(&s, &c, "#gen");
        assert_eq!(titles(&text), vec!["general"]);
        let voice = quick_switcher_results(&s, &c, "!lounge");
        assert_eq!(titles(&voice), vec!["Voice Lounge"]);
        let guilds = quick_switcher_results(&s, &c, "*beta");
        assert_eq!(titles(&guilds), vec!["Beta Club"]);
        // plain @ = friends + community members + DM users; dave is neither
        let users = quick_switcher_results(&s, &c, "@");
        let names = titles(&users);
        assert!(names.contains(&"Bobby".to_owned()) && names.contains(&"carol".to_owned()));
        assert!(!names.contains(&"dave".to_owned()));
        // @@ includes everyone known except me
        let all = titles(&quick_switcher_results(&s, &c, "@@"));
        assert!(all.contains(&"dave".to_owned()) && !all.contains(&"me".to_owned()));
        // nickname is searchable
        assert_eq!(titles(&quick_switcher_results(&s, &c, "@bobby")), vec!["Bobby"]);
    }

    #[test]
    fn general_search_single_category_reruns_with_wide_limit() {
        let mut s = fixture_state();
        // 8 extra users named "zed N": only the user category matches "zed".
        for i in 0..8 {
            let id = 5000 + i;
            s.users.insert(sf(id), serde_json::from_value(user_json(id, &format!("zed{i}"))).unwrap());
        }
        let items = quick_switcher_results(&s, &ctx(), "zed");
        assert_eq!(results(&items).len(), 8, "single non-empty category is not capped at 5");
        // two categories -> each capped at CATEGORY_LIMIT
        s.channels.insert(
            sf(7000),
            serde_json::from_value(json!({"id": "7000", "guild_id": GUILD.to_string(), "type": 0, "name": "zed-chat"})).unwrap(),
        );
        let items = quick_switcher_results(&s, &ctx(), "zed");
        let users = results(&items).iter().filter(|r| matches!(r.target, SwitcherTarget::User { .. })).count();
        assert_eq!(users, CATEGORY_LIMIT);
    }

    #[test]
    fn ordering_score_then_alpha_then_weight() {
        let mut s = fixture_state();
        for (id, name) in [(5001, "anna"), (5002, "annabel"), (5003, "joanna")] {
            s.users.insert(sf(id), serde_json::from_value(user_json(id, name)).unwrap());
        }
        let items = quick_switcher_results(&s, &ctx(), "anna");
        // exact(10) anna, then prefix(7) annabel, then substring(5) joanna
        assert_eq!(titles(&items), vec!["anna", "annabel", "joanna"]);
        // equal score users alphabetical by matched text
        let items = quick_switcher_results(&s, &ctx(), "a");
        let users: Vec<&SwitcherResult> = results(&items).into_iter().filter(|r| r.score == 7 && matches!(r.target, SwitcherTarget::User { .. })).collect();
        let mut sorted = users.iter().map(|r| r.matched_text.to_lowercase()).collect::<Vec<_>>();
        let original = sorted.clone();
        sorted.sort();
        assert_eq!(original, sorted);
    }

    #[test]
    fn unread_boost_orders_channels() {
        let mut s = fixture_state();
        // two equally scored channels: older one is unread -> boosted above the newer read one
        s.channels.insert(sf(7001), serde_json::from_value(json!({
            "id": "7001", "guild_id": GUILD.to_string(), "type": 0, "name": "dev-a", "last_message_id": mid(100).to_string()})).unwrap());
        s.channels.insert(sf(7002), serde_json::from_value(json!({
            "id": "7002", "guild_id": GUILD.to_string(), "type": 0, "name": "dev-b", "last_message_id": mid(50).to_string()})).unwrap());
        s.read_states.insert(sf(7001), crate::state::ReadState { ack_message_id: Some(mid(100)), known: true, ..Default::default() });
        s.read_states.insert(sf(7002), crate::state::ReadState { ack_message_id: Some(mid(1)), known: true, ..Default::default() });
        assert!(channel_sort_weight(&s, sf(7002)) > channel_sort_weight(&s, sf(7001)));
        let items = quick_switcher_results(&s, &ctx(), "#dev");
        assert_eq!(titles(&items), vec!["dev-b", "dev-a"]);
    }

    #[test]
    fn dedupes_dm_user_and_channels() {
        let s = fixture_state();
        let items = quick_switcher_results(&s, &ctx(), "bob");
        let bobs = results(&items).iter().filter(|r| matches!(r.target, SwitcherTarget::User { user_id, .. } if user_id == sf(OTHER))).count();
        assert_eq!(bobs, 1);
        assert!(results(&items).iter().any(|r| matches!(r.target, SwitcherTarget::User { dm_channel_id: Some(c), .. } if c == sf(DM))));
    }

    #[test]
    fn default_sections_and_caps() {
        let mut s = fixture_state();
        let mut c = ctx();
        c.current_channel = Some(sf(ANNOUNCE));
        c.recent_visits = vec![sf(ANNOUNCE), sf(DM), sf(DM), sf(GROUP_DM), sf(STAFF), sf(VOICE), sf(GENERAL)];
        let items = quick_switcher_results(&s, &c, "");
        // GENERAL has 1 mention and is unread: sections exist -> previous capped at 3.
        assert_eq!(items[0], SwitcherItem::Header(SwitcherHeader::Previous));
        let prev: Vec<_> = results(&items[1..]).into_iter().take(3).map(|r| r.title.clone()).collect();
        assert_eq!(prev, vec!["bob", "Study Group", "Voice Lounge"], "staff-room is not viewable, current channel excluded");
        // GENERAL was visited 7th: not in the first 3 previous, so it appears under Mentions
        let mentions_at = items.iter().position(|i| matches!(i, SwitcherItem::Header(SwitcherHeader::Mentions)));
        assert!(mentions_at.is_some_and(|i| matches!(&items[i + 1], SwitcherItem::Result(r) if r.title == "general")));

        // nothing unread/mentioned: previous alone shows up to 7
        s.read_states.clear();
        for id in [GENERAL, ANNOUNCE, DM, GROUP_DM] {
            let last = s.channels.get(&sf(id)).and_then(|ch| ch.last_message_id);
            s.read_states.insert(sf(id), crate::state::ReadState { ack_message_id: last, known: true, ..Default::default() });
        }
        c.recent_visits = vec![sf(DM), sf(GROUP_DM), sf(VOICE), sf(GENERAL), sf(7)];
        let items = quick_switcher_results(&s, &c, "");
        let n = results(&items).len();
        assert!(n >= 4 && !items.iter().any(|i| matches!(i, SwitcherItem::Header(SwitcherHeader::Mentions))));
    }

    fn targets(items: &[SwitcherItem], f: impl Fn(&SwitcherTarget) -> bool) -> usize {
        results(items).iter().filter(|r| f(&r.target)).count()
    }

    #[test]
    fn home_favorites_and_settings_need_a_non_empty_query() {
        let s = fixture_state();
        let virtual_or_settings =
            |t: &SwitcherTarget| matches!(t, SwitcherTarget::Home | SwitcherTarget::Favorites | SwitcherTarget::Settings { .. });
        let mut c = ctx();
        c.recent_visits = vec![sf(DM), sf(GROUP_DM)];
        for query in ["", "   "] {
            assert_eq!(targets(&quick_switcher_results_with(&s, &c, query, true), virtual_or_settings), 0, "{query:?}");
        }
        // prefix mode with an empty search keeps the plain guild list
        assert_eq!(targets(&quick_switcher_results_with(&s, &c, "*", true), virtual_or_settings), 0);

        let home = quick_switcher_results(&s, &c, "home");
        let row = results(&home).into_iter().find(|r| r.target == SwitcherTarget::Home).expect("Home row");
        assert_eq!((row.title.as_str(), row.subtitle.as_deref(), row.score), ("Home", Some("Direct messages"), 10));
    }

    #[test]
    fn favorites_row_requires_favorites() {
        let s = fixture_state();
        let c = ctx();
        assert_eq!(targets(&quick_switcher_results_with(&s, &c, "favorites", false), |t| *t == SwitcherTarget::Favorites), 0);
        assert_eq!(targets(&quick_switcher_results(&s, &c, "favorites"), |t| *t == SwitcherTarget::Favorites), 0);
        let with = quick_switcher_results_with(&s, &c, "favorites", true);
        assert_eq!(targets(&with, |t| *t == SwitcherTarget::Favorites), 1);
        assert_eq!(targets(&with, |t| *t == SwitcherTarget::Home), 0, "Home does not match 'favorites'");
    }

    #[test]
    fn navigation_rows_are_deduped_in_general_and_star_modes() {
        let s = fixture_state();
        let c = ctx();
        for query in ["home", "*home", "fav", "*fav"] {
            let items = quick_switcher_results_with(&s, &c, query, true);
            let mut keys: Vec<_> = results(&items).iter().map(|r| r.target.key()).collect();
            let n = keys.len();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), n, "{query}: duplicate rows");
        }
        let star = quick_switcher_results_with(&s, &c, "*home", true);
        assert!(matches!(star[0], SwitcherItem::Header(SwitcherHeader::Searching(QueryMode::Guilds))));
        assert_eq!(targets(&star, |t| *t == SwitcherTarget::Home), 1);
        assert_eq!(targets(&star, |t| matches!(t, SwitcherTarget::Settings { .. })), 0, "settings are general-search only");
        let ids: BTreeSet<&str> = SETTINGS_TABS.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), SETTINGS_TABS.len(), "settings tab ids are unique");
    }

    #[test]
    fn settings_results_map_to_tab_ids() {
        let s = fixture_state();
        let c = ctx();
        let settings = |query: &str| -> Vec<String> {
            results(&quick_switcher_results(&s, &c, query))
                .iter()
                .filter_map(|r| match &r.target {
                    SwitcherTarget::Settings { id } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(settings("keybinds"), vec!["keybinds"]);
        assert_eq!(settings("voice")[0], "voice_video");
        assert_eq!(settings("privacy")[0], "privacy_safety");
        assert!(settings("zzzzqq").is_empty());
        let items = quick_switcher_results(&s, &c, "windows app");
        let first = results(&items).into_iter().find(|r| matches!(r.target, SwitcherTarget::Settings { .. })).unwrap();
        assert_eq!(first.target, SwitcherTarget::Settings { id: "desktop_settings".to_owned() });
    }
}
