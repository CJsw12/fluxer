//! Composer autocomplete: `@` members, roles and @everyone/@here, `#`
//! channels, `:` emoji. The candidate lists are pure functions over
//! `ClientState` (`useLexicalAutocomplete.ts:349-470`,
//! `AutocompleteOptionBuilders.ts`); the popup renders above the input.

use std::ops::Range;

use fluxer_client_core::search::emoji::{
    AUTOCOMPLETE_LIMIT, EmojiFrecency, EmojiHit, channel_query, emoji_query, mention_query, search_emojis,
};
use fluxer_client_core::state::{
    ChannelKind, ClientState, Member, Snowflake, User, bits, has, user_can_view_channel,
};
use gpui_kit::{AnyElement, Context, FontWeight, div, prelude::*, px};

use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::composer_text::Insert;
use crate::ui::l10n;

/// `MENTION_RESULT_LIMIT`.
pub const MENTION_RESULT_LIMIT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Mention,
    Channel,
    Emoji,
}

/// A trigger found in the text up to the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerMatch<'a> {
    pub trigger: Trigger,
    /// Text after the trigger character.
    pub query: &'a str,
    /// Byte offset of the trigger character; the pick replaces
    /// `start..text.len()`.
    pub start: usize,
}

/// `hasOpenCodeFence`: an odd number of ``` before the cursor.
pub fn has_open_code_fence(text: &str) -> bool {
    text.matches("```").count() % 2 == 1
}

/// `detectAutocompleteTrigger` order: mention, channel, emoji. Nothing
/// inside an open code block.
pub fn detect(text_before_cursor: &str) -> Option<TriggerMatch<'_>> {
    if has_open_code_fence(text_before_cursor) {
        return None;
    }
    let (trigger, query) = if let Some(q) = mention_query(text_before_cursor) {
        (Trigger::Mention, q)
    } else if let Some(q) = channel_query(text_before_cursor) {
        (Trigger::Channel, q)
    } else {
        (Trigger::Emoji, emoji_query(text_before_cursor)?)
    };
    let start = text_before_cursor.len() - query.len() - 1;
    Some(TriggerMatch { trigger, query, start })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionKind {
    User,
    Role,
    Special,
    Channel,
    Emoji,
}

/// One row of the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    pub label: String,
    /// Secondary text (username tag, `:name:`).
    pub detail: Option<String>,
    /// Emoji glyph or `:` for custom emoji, shown before the label.
    pub icon: Option<String>,
    pub insert: Insert,
    /// [`EmojiHit::frecency_key`] of an emoji pick.
    pub emoji_key: Option<String>,
}

/// The open popup.
#[derive(Debug, Clone, PartialEq)]
pub struct Autocomplete {
    pub trigger: Trigger,
    /// Bytes of the input text that a pick replaces (trigger to cursor).
    pub range: Range<usize>,
    pub items: Vec<Suggestion>,
    pub selected: usize,
}

impl Autocomplete {
    /// Up/Down, wrapping.
    pub fn move_selection(&mut self, delta: i32) {
        let Ok(len) = i64::try_from(self.items.len()) else { return };
        if len == 0 {
            return;
        }
        let current = i64::try_from(self.selected).unwrap_or(0);
        self.selected = usize::try_from((current + i64::from(delta)).rem_euclid(len)).unwrap_or(0);
    }
}

/// The popup for `text_before_cursor` in `channel_id`, or `None` when there
/// is no trigger or nothing matches.
pub fn compute(
    state: &ClientState,
    channel_id: Snowflake,
    text_before_cursor: &str,
    frecency: &EmojiFrecency,
    now_ms: u64,
) -> Option<Autocomplete> {
    let m = detect(text_before_cursor)?;
    let items = match m.trigger {
        Trigger::Mention => mention_suggestions(state, channel_id, m.query),
        Trigger::Channel => channel_suggestions(state, channel_id, m.query),
        Trigger::Emoji => emoji_suggestions(state, channel_id, m.query, frecency, now_ms),
    };
    if items.is_empty() {
        return None;
    }
    Some(Autocomplete { trigger: m.trigger, range: m.start..text_before_cursor.len(), items, selected: 0 })
}

// ---- matching -------------------------------------------------------------

/// match-sorter's lowest default ranking (MATCHES): the query's characters
/// appear in order, case-insensitively. Higher rankings (equal, prefix,
/// word start, contains, acronym) all imply this, and every caller re-sorts
/// afterwards, so only the threshold matters.
fn fuzzy_matches(value: &str, query: &str) -> bool {
    let value = value.to_lowercase();
    let mut chars = value.chars();
    query.to_lowercase().chars().all(|q| chars.any(|c| c == q))
}

/// `parseMentionQuery`: `name#tag` splits at the first `#`.
fn parse_mention_query(query: &str) -> (&str, Option<&str>) {
    match query.split_once('#') {
        Some((name, tag)) => (name, Some(tag)),
        None => (query, None),
    }
}

/// `formatUserTag` with unique usernames: `name#disc` unless the
/// discriminator is all zeros (or missing).
fn user_tag(user: &User) -> String {
    match user.discriminator.as_deref() {
        Some(d) if !d.is_empty() && !d.chars().all(|c| c == '0') => format!("{}#{d}", user.username),
        _ => user.username.clone(),
    }
}

/// A user with their nick in this guild (if any).
struct Person<'a> {
    user: &'a User,
    nick: Option<&'a str>,
}

impl Person<'_> {
    /// `getMemberDisplayName`: nick, else global name, else username.
    fn display_name(&self) -> &str {
        self.nick.filter(|n| !n.is_empty()).unwrap_or_else(|| self.user.display_name())
    }

    fn matches(&self, query: &str) -> bool {
        let (name, tag) = parse_mention_query(query);
        if let Some(tag) = tag {
            let name = name.to_lowercase();
            let starts = |v: &str| v.to_lowercase().starts_with(&name);
            let name_ok = name.is_empty()
                || starts(&self.user.username)
                || starts(self.display_name())
                || self.nick.is_some_and(starts);
            let disc = self.user.discriminator.as_deref().unwrap_or_default().to_lowercase();
            return name_ok && disc.starts_with(&tag.to_lowercase());
        }
        let name = name.trim();
        if name.is_empty() {
            return true;
        }
        let tag = user_tag(self.user);
        [Some(self.display_name()), self.nick, self.user.global_name.as_deref(), Some(&self.user.username), Some(&tag)]
            .into_iter()
            .flatten()
            .any(|v| fuzzy_matches(v, name))
    }

    fn suggestion(&self) -> Suggestion {
        let tag = user_tag(self.user);
        Suggestion {
            kind: SuggestionKind::User,
            label: self.display_name().to_owned(),
            detail: Some(tag.clone()),
            icon: None,
            insert: Insert::Token { display: format!("@{tag}"), wire: format!("<@{}>", self.user.id) },
            emoji_key: None,
        }
    }
}

/// `filterDMUsers` / `filterGuildMembers` (no stable server rank): match,
/// sort by display name, keep [`MENTION_RESULT_LIMIT`].
fn filter_people(people: Vec<Person<'_>>, query: &str) -> Vec<Suggestion> {
    let mut matched: Vec<Person<'_>> = people.into_iter().filter(|p| p.matches(query)).collect();
    matched.sort_by_key(|p| (p.display_name().to_lowercase(), p.user.id));
    matched.iter().take(MENTION_RESULT_LIMIT).map(Person::suggestion).collect()
}

fn special(kind: &str) -> Suggestion {
    Suggestion {
        kind: SuggestionKind::Special,
        label: kind.to_owned(),
        detail: None,
        icon: None,
        insert: Insert::Text(kind.to_owned()),
        emoji_key: None,
    }
}

/// `@` candidates: members (or recent speakers for an empty query), then
/// @everyone/@here, then roles. DMs list the recipients.
pub fn mention_suggestions(state: &ClientState, channel_id: Snowflake, query: &str) -> Vec<Suggestion> {
    let Some(channel) = state.channels.get(&channel_id) else { return Vec::new() };
    let me = state.current_user_id();
    let (name_query, _) = parse_mention_query(query);
    let name_query = name_query.trim().to_lowercase();
    let Some(guild_id) = channel.guild_id else {
        let people: Vec<Person<'_>> =
            channel.recipients.iter().filter(|u| Some(u.id) != me).map(|user| Person { user, nick: None }).collect();
        let mut out = filter_people(people, query);
        // `areSpecialMentionsAllowed`: group DMs only outside guilds.
        if channel.kind == ChannelKind::GroupDm {
            out.extend(specials(&name_query));
        }
        return out;
    };
    let members = state.members.get(&guild_id);
    let speakers = if query.is_empty() { recent_speakers(state, channel_id, guild_id) } else { Vec::new() };
    let mut out = if speakers.is_empty() {
        let people: Vec<Person<'_>> = members
            .into_iter()
            .flat_map(|m| m.values())
            .filter(|m| user_can_view_channel(state, m.user.id, channel_id))
            .map(|m: &Member| Person { user: &m.user, nick: m.nick.as_deref() })
            .collect();
        filter_people(people, query)
    } else {
        speakers
    };
    let can_mention_everyone = has(state.channel_permissions(channel_id), bits::MENTION_EVERYONE);
    if can_mention_everyone {
        out.extend(specials(&name_query));
    }
    if let Some(guild) = state.guilds.get(&guild_id) {
        let mut roles: Vec<_> = guild
            .roles
            .values()
            .filter(|r| r.id != guild_id && (can_mention_everyone || r.mentionable))
            .filter(|r| name_query.is_empty() || r.name.to_lowercase().contains(&name_query))
            .collect();
        roles.sort_by_key(|r| (std::cmp::Reverse(r.position), r.id));
        out.extend(roles.into_iter().take(MENTION_RESULT_LIMIT).map(|r| Suggestion {
            kind: SuggestionKind::Role,
            label: format!("@{}", r.name),
            detail: None,
            icon: None,
            insert: Insert::Token { display: format!("@{}", r.name), wire: format!("<@&{}>", r.id) },
            emoji_key: None,
        }));
    }
    out
}

/// `SPECIAL_MENTIONS` filtered by `kind.slice(1).includes(query)`.
fn specials(query: &str) -> impl Iterator<Item = Suggestion> + '_ {
    ["@everyone", "@here"].into_iter().filter(move |k| k[1..].contains(query)).map(special)
}

/// `buildRecentSpeakerOptions`: newest authors in the loaded messages that
/// are known members, up to [`MENTION_RESULT_LIMIT`].
fn recent_speakers(state: &ClientState, channel_id: Snowflake, guild_id: Snowflake) -> Vec<Suggestion> {
    let (Some(window), Some(members)) = (state.messages.window(channel_id), state.members.get(&guild_id)) else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for message in window.messages.iter().rev() {
        let author = message.author.id;
        if seen.contains(&author) {
            continue;
        }
        seen.push(author);
        if let Some(m) = members.get(&author) {
            out.push(Person { user: &m.user, nick: m.nick.as_deref() }.suggestion());
            if out.len() >= MENTION_RESULT_LIMIT {
                break;
            }
        }
    }
    out
}

/// `#` candidates: the guild's channels except categories (and, unlike the
/// web client, except channels the user cannot view), fuzzy on the name,
/// in position order (threads last).
pub fn channel_suggestions(state: &ClientState, channel_id: Snowflake, query: &str) -> Vec<Suggestion> {
    let Some(guild_id) = state.channels.get(&channel_id).and_then(|c| c.guild_id) else { return Vec::new() };
    let mut channels: Vec<_> = state
        .channels
        .values()
        .filter(|c| c.guild_id == Some(guild_id) && !c.kind.is_category() && state.can_view_channel(c.id))
        .filter(|c| fuzzy_matches(c.name.as_deref().unwrap_or_default(), query))
        .collect();
    channels.sort_by_key(|c| (if c.kind.is_thread() { i64::MAX } else { i64::from(c.position) }, c.id));
    channels
        .into_iter()
        .take(MENTION_RESULT_LIMIT)
        .map(|c| {
            let name = c.name.clone().unwrap_or_default();
            let category = c.parent_id.and_then(|p| state.channels.get(&p)).and_then(|p| p.name.clone());
            Suggestion {
                kind: SuggestionKind::Channel,
                label: format!("#{name}"),
                detail: category,
                icon: None,
                insert: Insert::Token { display: format!("#{name}"), wire: format!("<#{}>", c.id) },
                emoji_key: None,
            }
        })
        .collect()
}

/// `:` candidates (`Emoji.search(channel, query, 10)`): unicode inserts the
/// glyph, custom emoji a `:name:` token sent as `<:name:id>`.
pub fn emoji_suggestions(
    state: &ClientState,
    channel_id: Snowflake,
    query: &str,
    frecency: &EmojiFrecency,
    now_ms: u64,
) -> Vec<Suggestion> {
    search_emojis(state, Some(channel_id), query, AUTOCOMPLETE_LIMIT, frecency, now_ms)
        .into_iter()
        .map(|hit| {
            let name = format!(":{}:", hit.name());
            let (icon, insert) = match &hit {
                EmojiHit::Unicode(e) => (e.surrogates.clone(), Insert::Text(e.surrogates.clone())),
                EmojiHit::Custom { .. } => {
                    ("✦".to_owned(), Insert::Token { display: name.clone(), wire: hit.insert_text() })
                }
            };
            Suggestion {
                kind: SuggestionKind::Emoji,
                label: name,
                detail: None,
                icon: Some(icon),
                insert,
                emoji_key: Some(hit.frecency_key()),
            }
        })
        .collect()
}

// ---- popup ----------------------------------------------------------------

/// The popup rows; a click picks the row, hovering selects it.
pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> Option<AnyElement> {
    let ac = view.autocomplete.as_ref()?;
    let heading = match ac.trigger {
        Trigger::Mention => l10n::tr("Members"),
        Trigger::Channel => l10n::tr("Channels"),
        Trigger::Emoji => l10n::tr("Emoji"),
    };
    let mut list = div()
        .id("autocomplete")
        .flex()
        .flex_col()
        .max_h(px(320.))
        .overflow_y_scroll()
        .track_scroll(&view.autocomplete_scroll)
        .py_1()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .shadow_md()
        .child(div().px_3().py_1().text_xs().text_color(theme.muted_foreground).child(heading));
    for (ix, item) in ac.items.iter().enumerate() {
        let selected = ix == ac.selected;
        let row = div()
            .id(("autocomplete-row", ix))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .mx_1()
            .rounded(px(4.))
            .when(selected, |d| d.bg(theme.accent))
            .children(item.icon.clone().map(|i| div().w(px(20.)).flex_none().child(i)))
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).child(item.label.clone()))
            .children(item.detail.clone().map(|d| div().text_xs().text_color(theme.muted_foreground).child(d)))
            .on_mouse_move(cx.listener(move |this, _, _, cx| this.autocomplete_hover(ix, cx)))
            .on_click(cx.listener(move |this, _, window, cx| this.autocomplete_accept(ix, window, cx)));
        list = list.child(row);
    }
    // Zero-height anchor: the popup grows upwards from the input's top edge.
    Some(div().relative().h_0().child(div().absolute().bottom_1().left_0().right_0().child(list)).into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_table() {
        fn t(s: &str) -> Option<(Trigger, &str, usize)> {
            detect(s).map(|m| (m.trigger, m.query, m.start))
        }
        assert_eq!(t("@"), Some((Trigger::Mention, "", 0)));
        assert_eq!(t("hi @an"), Some((Trigger::Mention, "an", 3)));
        assert_eq!(t("a@b"), None, "mid-word @ does not trigger");
        assert_eq!(t("see #gen"), Some((Trigger::Channel, "gen", 4)));
        assert_eq!(t("x#gen"), None);
        assert_eq!(t(" :ab"), Some((Trigger::Emoji, "ab", 1)));
        assert_eq!(t(":a"), None, "emoji needs two chars");
        assert_eq!(t("http://x"), None);
        assert_eq!(t("a:b"), None);
        assert_eq!(t("hi @an "), None, "a space ends the word");
        assert_eq!(t("```\n@an"), None, "open code fence");
        assert_eq!(t("```x``` @an"), Some((Trigger::Mention, "an", 8)));
    }

    #[test]
    fn mention_query_tags_and_fuzzy() {
        assert_eq!(parse_mention_query("ann#12"), ("ann", Some("12")));
        assert!(fuzzy_matches("General", "gnl"));
        assert!(!fuzzy_matches("General", "lg"));
        let user = User {
            id: Snowflake(5),
            username: "ann".into(),
            discriminator: Some("0042".into()),
            global_name: Some("Annie".into()),
            avatar: None,
            bot: false,
        };
        let p = Person { user: &user, nick: Some("Captain") };
        assert_eq!(p.display_name(), "Captain");
        assert!(p.matches("cap") && p.matches("annie") && p.matches("ann#00") && p.matches("#0042"));
        assert!(!p.matches("ann#1"));
        let s = p.suggestion();
        assert_eq!(s.insert, Insert::Token { display: "@ann#0042".into(), wire: "<@5>".into() });
    }

    #[test]
    fn selection_wraps() {
        let mut ac = Autocomplete { trigger: Trigger::Emoji, range: 0..0, items: vec![special("@here"); 3], selected: 0 };
        ac.move_selection(-1);
        assert_eq!(ac.selected, 2);
        ac.move_selection(1);
        assert_eq!(ac.selected, 0);
    }
}
