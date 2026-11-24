//! Plain snapshots of `ClientState` for rendering. Built under the state
//! lock, then rendered without it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use fluxer_client_core::state::{
    ATTACHMENT_FLAG_ANIMATED, ATTACHMENT_FLAG_EXPLICIT, Channel, ChannelKind, ClientState,
    Message, MessageState, Snowflake, Status, Unread, User, animated_proxy_url, static_proxy_url,
};
use fluxer_client_markdown::{Document, LowerOptions, MarkdownContext, MentionResolver, ResolvedRole, TimeFormat, parse};
use gpui_kit::SharedString;

use crate::format::{
    SystemIcon, default_avatar_index, fit_image, format_full_date, format_message_time, format_short_time,
    initials, message_time, starts_group, system_text,
};
use crate::ui::l10n;
use crate::ui::embed::{self, EmbedView, TextKind};
use crate::ui::message_actions::ActionTarget;
use crate::ui::reactions::{ReactionPill, reaction_pills};
use crate::ui::reply::{ReplyLine, member_name, reply_line};

/// What the guild rail has selected: the DM list or one guild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Home,
    Guild(Snowflake),
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuildItem {
    pub id: Snowflake,
    pub name: SharedString,
    pub initials: SharedString,
    pub icon_url: Option<SharedString>,
    pub unread: Unread,
    /// Any user is in a voice channel of the guild (`hasVoiceActivity`).
    pub has_voice_activity: bool,
    /// `(online, members)` when known.
    pub counts: Option<(u32, u32)>,
}

/// Glyph shown before a channel name (`ChannelUtils.getIcon`, simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelIcon {
    Text,
    Announcement,
    Voice,
    Thread,
    Forum,
    Link,
    Dm,
    Category,
}

impl ChannelIcon {
    pub fn of(kind: ChannelKind) -> Self {
        match kind {
            ChannelKind::GuildVoice => Self::Voice,
            ChannelKind::GuildCategory => Self::Category,
            ChannelKind::GuildAnnouncement => Self::Announcement,
            ChannelKind::AnnouncementThread | ChannelKind::PublicThread | ChannelKind::PrivateThread => Self::Thread,
            ChannelKind::GuildForum | ChannelKind::GuildMedia => Self::Forum,
            ChannelKind::GuildLink => Self::Link,
            ChannelKind::Dm | ChannelKind::GroupDm | ChannelKind::DmPersonalNotes => Self::Dm,
            ChannelKind::GuildText | ChannelKind::Unknown(_) => Self::Text,
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Self::Text => "#",
            Self::Announcement => "📢",
            Self::Voice => "🔊",
            Self::Thread => "🧵",
            Self::Forum => "💬",
            Self::Link => "🔗",
            Self::Dm => "@",
            Self::Category => "",
        }
    }
}

/// Channels with a message pane in this client: text-based and threads,
/// except voice (its text chat belongs to the voice UI, out of scope).
pub fn opens_message_pane(kind: ChannelKind) -> bool {
    kind.has_messages() && kind != ChannelKind::GuildVoice
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelItem {
    pub id: Snowflake,
    pub name: SharedString,
    pub icon: ChannelIcon,
    pub is_category: bool,
    /// Has a message pane (text, announcement, DM...). Voice/link/forum
    /// channels are listed but not selectable.
    pub has_messages: bool,
    pub can_manage: bool,
    pub can_manage_permissions: bool,
    pub nested: bool,
    pub unread: Unread,
    pub muted: bool,
    /// Primary DM recipient user id (1:1 or group DM fallback) for avatar rendering.
    pub dm_user_id: Option<Snowflake>,
    /// Primary DM recipient avatar URL when `media` endpoint and avatar hash exist.
    pub dm_avatar_url: Option<SharedString>,
    /// Primary DM recipient presence status (1:1 DM only).
    pub dm_status: Option<Status>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImagePreview {
    pub url: SharedString,
    /// Media proxy URL of the attachment; drawn instead of `url` at the
    /// drawn size (`getOptimizedMediaURL`), so a large original is never
    /// decoded at full resolution.
    pub proxy_url: Option<SharedString>,
    /// A GIF or animated image: the animated proxy variant is drawn.
    pub animated: bool,
    pub width: f32,
    pub height: f32,
}

impl ImagePreview {
    /// The URL to draw at window `scale`: the proxy variant at the drawn
    /// size in device pixels (animated for GIFs), else the original.
    pub fn source(&self, scale: f32) -> SharedString {
        let Some(proxy) = &self.proxy_url else { return self.url.clone() };
        let scale = if scale.is_finite() && scale > 0. { scale } else { 1. };
        let device = |css: f32| (css * scale).round().max(1.) as u32;
        let (w, h) = (device(self.width), device(self.height));
        let url = if self.animated { animated_proxy_url(proxy, w, h) } else { static_proxy_url(proxy, w, h) };
        url.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImageCover {
    pub spoiler: bool,
    pub nsfw: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileAttachmentRow {
    pub id: String,
    pub filename: String,
    pub size_label: String,
    pub url: String,
    pub spoiler: bool,
    /// `ExpiryFootnote` text under the card (`Attachment.tsx`).
    pub expiry: Option<SharedString>,
}

/// `ExpiryFootnote` text for attachments (`getEarliestAttachmentExpiry`):
/// "Expires on {date}" or a range, "Expired ..." once any is past; `None`
/// when no attachment carries an expiry.
pub(crate) fn attachment_expiry_label(attachments: &[&fluxer_client_core::state::Attachment]) -> Option<String> {
    let now = Local::now();
    let (mut earliest, mut latest): (Option<DateTime<Local>>, Option<DateTime<Local>>) = (None, None);
    let mut expired = false;
    for a in attachments {
        let at = match a.expires_at.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
            Some(at) => at.with_timezone(&Local),
            None if a.expired => now,
            None => continue,
        };
        earliest = Some(earliest.map_or(at, |e| e.min(at)));
        latest = Some(latest.map_or(at, |l| l.max(at)));
        expired |= a.expired || at <= now;
    }
    let (first, last) = (crate::format::format_short_date(&earliest?), crate::format::format_short_date(&latest?));
    Some(match (first != last, expired) {
        (true, true) => l10n::tr_args("Expired between {earliest} and {latest}", &[("earliest", &first), ("latest", &last)]),
        (true, false) => l10n::tr_args("Expires between {earliest} and {latest}", &[("earliest", &first), ("latest", &last)]),
        (false, true) => l10n::tr_args("Expired on {earliest}", &[("earliest", &first)]),
        (false, false) => l10n::tr_args("Expires on {earliest}", &[("earliest", &first)]),
    })
}

/// Human-readable file size (`"512 B"`, `"12.4 KB"`, `"2.1 MB"`).
pub(crate) fn format_file_size(bytes: u64) -> String {
    crate::ui::attachments::format_size(bytes)
}

/// A system message (join, pin...): icon and text, no avatar.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemRow {
    pub icon: SystemIcon,
    pub text: SharedString,
    /// Byte ranges of `text` in bold (user names).
    pub bold: Vec<std::ops::Range<usize>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub id: Snowflake,
    pub author_id: Snowflake,
    pub author: SharedString,
    pub initials: SharedString,
    pub avatar_url: Option<SharedString>,
    /// `getDefaultAvatarIndex`, used when there is no avatar image.
    pub default_avatar: usize,
    pub time: SharedString,
    /// Short time-of-day (`7:20 PM` / `19:20`) shown on hover for grouped messages.
    pub short_time: SharedString,
    /// Set for system message types; rendered as icon + text + time.
    pub system: Option<SystemRow>,
    /// Date separator shown above this row ("Wednesday, November 18, 2026"):
    /// the first loaded message and the first of each local day.
    pub day_divider: Option<SharedString>,
    /// The "NEW" divider goes above this row: the first message after the
    /// unread anchor captured when the channel was opened.
    pub new_divider: bool,
    /// "Welcome to #name" header above the first row once the start of the
    /// channel is loaded (guild channels).
    pub channel_start: Option<SharedString>,
    /// First message of a group: shows avatar, author and time.
    pub group_start: bool,
    /// True when the message mentions the logged-in user (`@user`, `@everyone`, or one of the user's roles).
    pub mentioned: bool,
    /// Markdown source: the content, then a `📎 [name](url)` line per
    /// non-image attachment.
    pub body: SharedString,
    /// `body` parsed by the Fluxer markdown parser; `None` when the body is
    /// empty or the row is a system message.
    pub document: Option<Arc<Document>>,
    pub images: Vec<ImagePreview>,
    /// Spoiler / NSFW cover flags parallel to `images`.
    pub image_covers: Vec<ImageCover>,
    /// Non-image file attachments for file cards.
    pub(crate) file_attachments: Vec<FileAttachmentRow>,
    /// `ExpiryFootnote` text under the image mosaic (`AttachmentMosaic`).
    pub image_expiry: Option<SharedString>,
    /// Embeds to draw (empty when embeds are off or suppressed).
    pub embeds: Vec<EmbedView>,
    pub state: MessageState,
    /// "Uploading… N%" for a pending message with files (set by the view
    /// from upload progress events, not by the view model).
    pub upload_percent: Option<u8>,
    /// The line above a reply (target author and excerpt).
    pub reply: Option<ReplyLine>,
    /// Reaction pills under the body.
    pub reactions: Vec<ReactionPill>,
    /// What the hover action bar offers for this message.
    pub actions: ActionTarget,
}

/// The message window of a channel as rows, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessagePane {
    pub rows: Vec<MessageRow>,
    pub has_more_before: bool,
    /// Newer messages were trimmed: incoming ones are not appended until
    /// the newer pages are loaded again.
    pub has_more_after: bool,
    /// A REST page has been applied.
    pub ready: bool,
}

/// `getGuildIconURL` (static webp, size 96); `None` when the guild has no custom icon.
pub fn guild_icon_url(media: &str, guild_id: Snowflake, icon: Option<&str>) -> Option<String> {
    let hash = icon.filter(|h| !h.is_empty())?;
    let hash = hash.strip_prefix("a_").unwrap_or(hash);
    Some(format!(
        "{}/icons/{}/{hash}.webp?size=96",
        media.trim_end_matches('/'),
        guild_id
    ))
}

#[allow(dead_code)]
pub fn guild_items(state: &ClientState) -> Vec<GuildItem> {
    guild_items_with_media(state, None)
}

pub fn guild_items_with_media(state: &ClientState, media: Option<&str>) -> Vec<GuildItem> {
    state
        .sorted_guilds()
        .into_iter()
        .map(|g| GuildItem {
            id: g.id,
            name: g.name.clone().into(),
            initials: initials(&g.name).into(),
            icon_url: media
                .and_then(|m| guild_icon_url(m, g.id, g.icon.as_deref()))
                .map(Into::into),
            unread: state.guild_unread(g.id),
            has_voice_activity: state.guild_has_voice_activity(g.id),
            counts: state.guild_counts(g.id).map(|(o, m)| {
                (u32::try_from(o).unwrap_or(u32::MAX), u32::try_from(m).unwrap_or(u32::MAX))
            }),
        })
        .collect()
}

/// Unread across all DMs, for the Home button.
pub fn dm_unread(state: &ClientState) -> Unread {
    let mut total = Unread::default();
    for c in state.dm_channels() {
        let u = state.channel_unread(c.id);
        total.unread |= u.unread;
        total.mention_count = total.mention_count.saturating_add(u.mention_count);
    }
    total
}

fn show_channel_when_hiding_muted(
    state: &ClientState,
    channel_id: Snowflake,
    effective_muted: bool,
    selected_channel: Option<Snowflake>,
    connected_voice_channel: Option<Snowflake>,
) -> bool {
    selected_channel == Some(channel_id)
        || connected_voice_channel == Some(channel_id)
        || !effective_muted
        || state.channel_unread(channel_id).mention_count > 0
}

pub fn channel_items_with_active(
    state: &ClientState,
    place: Place,
    media: Option<&str>,
    selected_channel: Option<Snowflake>,
    connected_voice_channel: Option<Snowflake>,
) -> Vec<ChannelItem> {
    let now_ms = i64::try_from(crate::accounts::now_ms()).unwrap_or(i64::MAX);
    match place {
        // The personal-notes self-DM has its own sidebar row, not a DM row.
        Place::Home => state
            .dm_channels()
            .into_iter()
            .filter(|c| c.kind != ChannelKind::DmPersonalNotes)
            .map(|c| channel_item(state, c, false, now_ms, media))
            .collect(),
        Place::Guild(guild_id) => {
            let hide_muted = state.guild_hides_muted_channels(guild_id);
            let mut items = Vec::new();
            for node in state.channel_tree(guild_id) {
                if !hide_muted {
                    items.push(channel_item(state, &node.channel, false, now_ms, media));
                    for child in &node.children {
                        items.push(channel_item(state, child, true, now_ms, media));
                    }
                    continue;
                }
                if node.channel.kind.is_category() {
                    let cat_muted = state.is_channel_muted(Some(guild_id), node.channel.id, now_ms);
                    let visible_children: Vec<&Channel> = node
                        .children
                        .iter()
                        .filter(|child| {
                            let ch_muted = state.is_channel_muted(Some(guild_id), child.id, now_ms);
                            show_channel_when_hiding_muted(
                                state,
                                child.id,
                                cat_muted || ch_muted,
                                selected_channel,
                                connected_voice_channel,
                            )
                        })
                        .collect();
                    if node.children.is_empty() || !visible_children.is_empty() {
                        items.push(channel_item(state, &node.channel, false, now_ms, media));
                        for child in visible_children {
                            items.push(channel_item(state, child, true, now_ms, media));
                        }
                    }
                } else {
                    let ch_muted = state.is_channel_muted(Some(guild_id), node.channel.id, now_ms);
                    if show_channel_when_hiding_muted(
                        state,
                        node.channel.id,
                        ch_muted,
                        selected_channel,
                        connected_voice_channel,
                    ) {
                        items.push(channel_item(state, &node.channel, false, now_ms, media));
                    }
                }
            }
            items
        }
    }
}

fn channel_item(
    state: &ClientState,
    channel: &Channel,
    nested: bool,
    now_ms: i64,
    media: Option<&str>,
) -> ChannelItem {
    let dm_recipient = match channel.kind {
        ChannelKind::Dm | ChannelKind::DmPersonalNotes => {
            channel.recipients.first().or(state.current_user.as_ref())
        }
        ChannelKind::GroupDm => channel.recipients.first(),
        _ => None,
    };
    let dm_user_id = dm_recipient.map(|u| u.id);
    let dm_avatar_url = dm_recipient
        .and_then(|u| media.and_then(|m| avatar_url(m, u)))
        .map(Into::into);
    let dm_status = match channel.kind {
        ChannelKind::Dm => dm_recipient.map(|u| state.presence_status(u.id)),
        _ => None,
    };
    ChannelItem {
        id: channel.id,
        name: channel_name(channel).into(),
        icon: ChannelIcon::of(channel.kind),
        is_category: channel.kind.is_category(),
        has_messages: opens_message_pane(channel.kind),
        can_manage: state.can_manage_channel(channel.id),
        can_manage_permissions: state.can_manage_channel_permissions(channel.id),
        nested,
        unread: state.channel_unread(channel.id),
        muted: state.is_channel_muted(channel.guild_id, channel.id, now_ms),
        dm_user_id,
        dm_avatar_url,
        dm_status,
    }
}

/// Channel name; DMs without a name use their recipients' display names.
pub fn channel_name(channel: &Channel) -> String {
    if let Some(name) = channel.name.as_deref().filter(|n| !n.is_empty()) {
        return name.to_owned();
    }
    let names: Vec<&str> = channel.recipients.iter().map(|u| u.display_name()).collect();
    if names.is_empty() { format!("channel {}", channel.id) } else { names.join(", ") }
}

/// The channel's message window, times in the local time zone.
/// `media` is the instance media endpoint (avatars). `unread_anchor` is
/// `ClientState::unread_anchor` captured when the channel was opened.
pub fn message_pane(
    state: &ClientState,
    channel_id: Snowflake,
    media: Option<&str>,
    unread_anchor: Option<Snowflake>,
) -> MessagePane {
    message_pane_at(state, channel_id, media, unread_anchor, &Local::now())
}

pub fn message_pane_at<Tz: TimeZone>(
    state: &ClientState,
    channel_id: Snowflake,
    media: Option<&str>,
    unread_anchor: Option<Snowflake>,
    now: &DateTime<Tz>,
) -> MessagePane
where
    Tz::Offset: std::fmt::Display,
{
    let Some(window) = state.messages.window(channel_id) else {
        return MessagePane::default();
    };
    let channel = state.channels.get(&channel_id);
    let guild_id = channel.and_then(|c| c.guild_id);
    let tz = now.timezone();
    // 24-hour clock, like the message times.
    let time = TimeFormat { now_ms: now.timestamp_millis(), ..TimeFormat::local(false) };
    let mut documents = Documents {
        previous: DOCUMENTS.with(|cache| std::mem::take(&mut *cache.borrow_mut())),
        current: HashMap::with_capacity(window.messages.len()),
        embed_previous: EMBED_DOCUMENTS.with(|cache| std::mem::take(&mut *cache.borrow_mut())),
        embed_current: HashMap::new(),
    };
    let row_ctx = RowContext { state, guild_id, media, time: &time, render_embeds: render_embeds(state) };
    let mut rows = Vec::with_capacity(window.messages.len());
    let mut prev: Option<(&Message, Option<DateTime<Tz>>)> = None;
    // `createChannelStream`: one divider before the first message newer
    // than the anchor.
    let mut unread_pending = unread_anchor;
    for m in &window.messages {
        let at = message_time(m, &tz);
        let group_start = starts_group(prev.as_ref().map(|(p, t)| (*p, t.as_ref())), m, at.as_ref());
        let mut row = row_ctx.row(m, at.as_ref(), now, group_start, &mut documents);
        let prev_day = prev.as_ref().and_then(|(_, t)| t.as_ref()).map(|t| t.date_naive());
        if let Some(at) = &at
            && (prev.is_none() || prev_day != Some(at.date_naive()))
        {
            row.day_divider = Some(format_full_date(at).into());
        }
        if unread_pending.is_some_and(|anchor| m.id > anchor) {
            row.new_divider = true;
            unread_pending = None;
        }
        rows.push(row);
        prev = Some((m, at));
    }
    if window.ready
        && !window.has_more_before
        && let (Some(first), Some(channel)) = (rows.first_mut(), channel.filter(|c| c.guild_id.is_some()))
    {
        first.channel_start = Some(format!("{}{}", ChannelIcon::of(channel.kind).glyph(), channel_name(channel)).into());
    }
    // Only the shown messages stay cached.
    DOCUMENTS.with(|cache| *cache.borrow_mut() = documents.current);
    EMBED_DOCUMENTS.with(|cache| *cache.borrow_mut() = documents.embed_current);
    MessagePane {
        rows,
        has_more_before: window.has_more_before,
        has_more_after: window.has_more_after,
        ready: window.ready,
    }
}

/// A parsed message body, reused while the message is not edited.
struct CachedDocument {
    edited: Option<String>,
    source: SharedString,
    document: Arc<Document>,
}

thread_local! {
    /// Parsed bodies of the last built pane by message id, so refreshes and
    /// scrolling do not parse again. Panes are built on the GPUI thread.
    static DOCUMENTS: RefCell<HashMap<Snowflake, CachedDocument>> = RefCell::new(HashMap::new());
    /// Parsed embed texts (descriptions, fields) of the last built pane.
    static EMBED_DOCUMENTS: RefCell<HashMap<(TextKind, String), Arc<Document>>> = RefCell::new(HashMap::new());
}

/// `UserSettings.renderEmbeds` (on until the settings say otherwise).
pub(crate) fn render_embeds(state: &ClientState) -> bool {
    state.user_settings.raw.get("render_embeds").and_then(serde_json::Value::as_bool).unwrap_or(true)
}

/// The cache of the previous pane and the entries of the one being built.
struct Documents {
    previous: HashMap<Snowflake, CachedDocument>,
    current: HashMap<Snowflake, CachedDocument>,
    embed_previous: HashMap<(TextKind, String), Arc<Document>>,
    embed_current: HashMap<(TextKind, String), Arc<Document>>,
}

struct RowContext<'a> {
    state: &'a ClientState,
    guild_id: Option<Snowflake>,
    media: Option<&'a str>,
    time: &'a TimeFormat,
    render_embeds: bool,
}

impl RowContext<'_> {
    fn row<Tz: TimeZone>(
        &self,
        m: &Message,
        at: Option<&DateTime<Tz>>,
        now: &DateTime<Tz>,
        group_start: bool,
        documents: &mut Documents,
    ) -> MessageRow
    where
        Tz::Offset: std::fmt::Display,
    {
        let mut body = m.content.clone();
        let mut images = Vec::new();
        let mut image_covers = Vec::new();
        let mut file_attachments = Vec::new();
        for a in &m.attachments {
            let spoiler = a.is_spoiler() || a.filename.starts_with("SPOILER_");
            let nsfw = a.flags & ATTACHMENT_FLAG_EXPLICIT != 0;
            let is_image = a.content_type.as_deref().is_some_and(|t| t.starts_with("image/"));
            if is_image && let Some(url) = &a.url {
                let (width, height) = fit_image(a.width, a.height);
                let animated =
                    a.content_type.as_deref() == Some("image/gif") || a.flags & ATTACHMENT_FLAG_ANIMATED != 0;
                images.push(ImagePreview {
                    url: url.clone().into(),
                    proxy_url: a.proxy_url.clone().map(Into::into),
                    animated,
                    width,
                    height,
                });
                image_covers.push(ImageCover { spoiler, nsfw });
                continue;
            }
            file_attachments.push(FileAttachmentRow {
                id: a.id.to_string(),
                filename: a.filename.clone(),
                size_label: format_file_size(a.size),
                url: a.url.clone().unwrap_or_default(),
                spoiler,
                expiry: attachment_expiry_label(&[a]).map(Into::into),
            });
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            match &a.url {
                Some(url) => body.push_str(&format!("📎 [{}]({url})", a.filename)),
                None => body.push_str(&format!("📎 {}", a.filename)),
            }
        }
        let mut time = at.map(|at| format_message_time(at, now)).unwrap_or_default();
        let short_time = at.map(format_short_time).unwrap_or_default();
        if m.is_edited() {
            time.push_str(" (edited)");
        }
        let author = self.user_name(&m.author);
        let mentioned = m.mentions.first().map(|u| self.user_name(u));
        let system = system_text(m, &author, mentioned.as_deref())
            .map(|s| SystemRow { icon: s.icon, text: s.text.into(), bold: s.bold });
        let body: SharedString = body.into();
        let mut document = (system.is_none() && !body.is_empty()).then(|| self.document(m, &body, documents));
        let show_embeds = self.render_embeds && system.is_none() && !m.suppresses_embeds();
        let embeds = if show_embeds {
            let footer_time = |iso: &str| {
                let at = chrono::DateTime::parse_from_rfc3339(iso).ok()?.with_timezone(&now.timezone());
                Some(format_message_time(&at, now))
            };
            embed::embed_views(m, &mut |text, kind| self.embed_document(m, text, kind, documents), &footer_time)
        } else {
            Vec::new()
        };
        // A bare link whose embeds are all media: the media replaces the URL.
        if show_embeds && document.as_deref().is_some_and(|d| embed::hides_bare_url(m, d)) {
            document = None;
        }
        let me_id = self.state.current_user_id();
        let mentions_me = m.mention_everyone
            || me_id.is_some_and(|me| {
                m.mentions.iter().any(|u| u.id == me)
                    || (!m.mention_roles.is_empty()
                        && self
                            .guild_id
                            .and_then(|gid| self.state.members.get(&gid)?.get(&me))
                            .is_some_and(|member| m.mention_roles.iter().any(|rid| member.roles.contains(rid))))
            });
        MessageRow {
            id: m.id,
            author_id: m.author.id,
            initials: initials(&author).into(),
            author: author.into(),
            avatar_url: self.media.and_then(|media| avatar_url(media, &m.author)).map(Into::into),
            default_avatar: default_avatar_index(m.author.id),
            time: time.into(),
            short_time: short_time.into(),
            system,
            day_divider: None,
            new_divider: false,
            channel_start: None,
            group_start,
            mentioned: mentions_me,
            body,
            document,
            images,
            image_covers,
            file_attachments,
            image_expiry: attachment_expiry_label(&m.attachments.iter().filter(|a| a.content_type.as_deref().is_some_and(|t| t.starts_with("image/")) && a.url.is_some()).collect::<Vec<_>>()).map(Into::into),
            embeds,
            state: m.state,
            upload_percent: None,
            reply: reply_line(self.state, m, self.media),
            reactions: reaction_pills(&m.reactions, self.media),
            actions: ActionTarget::of(self.state, m),
        }
    }

    /// The parsed body, reused while the id, edit time and source match.
    fn document(&self, m: &Message, body: &SharedString, documents: &mut Documents) -> Arc<Document> {
        let cached = documents.previous.remove(&m.id).filter(|c| c.edited == m.edited_timestamp && c.source == *body);
        let document = match cached {
            Some(cached) => cached.document,
            None => {
                let resolver = StateMentions { state: self.state, guild_id: self.guild_id, message: m };
                Arc::new(parse(
                    body,
                    &LowerOptions {
                        context: MarkdownContext::StandardWithJumbo,
                        resolver: &resolver,
                        media_endpoint: self.media,
                        time: self.time,
                        disable_animated_emoji: false,
                    },
                ))
            }
        };
        documents.current.insert(
            m.id,
            CachedDocument { edited: m.edited_timestamp.clone(), source: body.clone(), document: document.clone() },
        );
        document
    }

    /// A parsed embed text, reused from the previous pane by kind and text.
    fn embed_document(&self, m: &Message, text: &str, kind: TextKind, documents: &mut Documents) -> Arc<Document> {
        let key = (kind, text.to_owned());
        if let Some(document) = documents.embed_current.get(&key) {
            return document.clone();
        }
        let document = match documents.embed_previous.remove(&key) {
            Some(document) => document,
            None => {
                let resolver = StateMentions { state: self.state, guild_id: self.guild_id, message: m };
                Arc::new(parse(
                    text,
                    &LowerOptions {
                        context: kind.context(),
                        resolver: &resolver,
                        media_endpoint: self.media,
                        time: self.time,
                        disable_animated_emoji: false,
                    },
                ))
            }
        };
        documents.embed_current.insert(key, document.clone());
        document
    }

    fn user_name(&self, user: &User) -> String {
        member_name(self.state, self.guild_id, user)
    }
}

/// Mention names for one message's markdown, from the client state.
pub struct StateMentions<'a> {
    pub state: &'a ClientState,
    /// Guild of the message's channel: member nicknames and its roles.
    pub guild_id: Option<Snowflake>,
    /// Its `mentions` carry users the state may not have cached.
    pub message: &'a Message,
}

impl MentionResolver for StateMentions<'_> {
    fn user(&self, id: &str) -> Option<String> {
        let id = Snowflake::parse(id)?;
        let state = self.state;
        let member = self.guild_id.and_then(|g| state.members.get(&g)?.get(&id));
        let user = self
            .message
            .mentions
            .iter()
            .find(|u| u.id == id)
            .or_else(|| member.map(|member| &member.user))
            .or_else(|| state.users.get(&id))
            .or_else(|| state.current_user.as_ref().filter(|u| u.id == id))
            .or_else(|| state.channels.values().flat_map(|c| &c.recipients).find(|u| u.id == id))?;
        Some(member_name(state, self.guild_id, user))
    }

    /// A role of the message's guild (of any guild outside one).
    fn role(&self, id: &str) -> Option<ResolvedRole> {
        let id = Snowflake::parse(id)?;
        let state = self.state;
        let role = match self.guild_id.and_then(|g| state.guilds.get(&g)) {
            Some(guild) => guild.roles.get(&id),
            None => state.guilds.values().find_map(|g| g.roles.get(&id)),
        }?;
        let color = u32::try_from(role.color).ok().filter(|c| *c != 0 && *c <= 0xff_ffff);
        Some(ResolvedRole { name: role.name.clone(), color })
    }

    /// Text, announcement, voice, link and thread channels.
    fn channel(&self, id: &str) -> Option<String> {
        let channel = self.state.channels.get(&Snowflake::parse(id)?)?;
        let mentionable = matches!(
            channel.kind,
            ChannelKind::GuildText
                | ChannelKind::GuildAnnouncement
                | ChannelKind::GuildVoice
                | ChannelKind::GuildLink
                | ChannelKind::AnnouncementThread
                | ChannelKind::PublicThread
                | ChannelKind::PrivateThread
        );
        mentionable.then(|| channel_name(channel))
    }
}

/// The logged-in user for the user panel.
#[derive(Debug, Clone, PartialEq)]
pub struct UserSummary {
    pub id: Snowflake,
    pub name: SharedString,
    pub initials: SharedString,
    pub avatar_url: Option<SharedString>,
    pub default_avatar: usize,
}

/// `None` before READY.
pub fn current_user(state: &ClientState, media: Option<&str>) -> Option<UserSummary> {
    let user = state.current_user.as_ref()?;
    let name = user.display_name();
    Some(UserSummary {
        id: user.id,
        name: name.to_owned().into(),
        initials: initials(name).into(),
        avatar_url: media.and_then(|media| avatar_url(media, user)).map(Into::into),
        default_avatar: default_avatar_index(user.id),
    })
}

/// `getUserAvatarURL` (static webp, size 80); `None` for the default avatar.
pub fn avatar_url(media: &str, user: &User) -> Option<String> {
    let hash = user.avatar.as_deref().filter(|h| !h.is_empty())?;
    let hash = hash.strip_prefix("a_").unwrap_or(hash);
    Some(format!("{}/avatars/{}/{hash}.webp?size=80", media.trim_end_matches('/'), user.id))
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;
    use fluxer_client_core::Dispatch;
    use fluxer_client_core::state::LoadMode;
    use serde_json::json;

    use super::*;

    fn user(avatar: Option<&str>) -> User {
        serde_json::from_value(json!({ "id": "42", "username": "u", "avatar": avatar })).expect("user")
    }

    /// Attachments draw the proxy at device size; GIFs get the animated
    /// variant (`?width&height&animated=true`), others the still webp.
    #[test]
    fn attachment_images_use_proxy_variants() {
        let gif = ImagePreview {
            url: "https://cdn/a.gif".into(),
            proxy_url: Some("https://media/attachments/1/2/a.gif".into()),
            animated: true,
            width: 220.,
            height: 160.,
        };
        assert_eq!(gif.source(1.5).as_ref(), "https://media/attachments/1/2/a.gif?width=330&height=240&animated=true");
        let png = ImagePreview { animated: false, ..gif.clone() };
        assert_eq!(png.source(1.).as_ref(), "https://media/attachments/1/2/a.gif?format=webp&width=220&height=160");
        let local = ImagePreview { proxy_url: None, ..gif };
        assert_eq!(local.source(2.).as_ref(), "https://cdn/a.gif");
    }

    #[test]
    fn avatar_urls_follow_ts() {
        assert_eq!(
            avatar_url("https://media.example/", &user(Some("a_abc"))).as_deref(),
            Some("https://media.example/avatars/42/abc.webp?size=80")
        );
        assert_eq!(avatar_url("https://m", &user(Some("abc"))).as_deref(), Some("https://m/avatars/42/abc.webp?size=80"));
        assert_eq!(avatar_url("https://m", &user(None)), None);
    }

    #[test]
    fn voice_and_non_text_channels_have_no_pane() {
        assert!(!opens_message_pane(ChannelKind::GuildVoice));
        assert!(!opens_message_pane(ChannelKind::GuildCategory));
        assert!(!opens_message_pane(ChannelKind::GuildLink));
        assert!(!opens_message_pane(ChannelKind::GuildForum));
        assert!(opens_message_pane(ChannelKind::GuildText));
        assert!(opens_message_pane(ChannelKind::GuildAnnouncement));
        assert!(opens_message_pane(ChannelKind::Dm));
        assert!(opens_message_pane(ChannelKind::PublicThread));
        assert_eq!(ChannelIcon::of(ChannelKind::GuildVoice).glyph(), "🔊");
        assert_eq!(ChannelIcon::of(ChannelKind::GuildText).glyph(), "#");
    }

    /// Rows resolve mentions against the state, group by author/time and
    /// split image attachments from file links.
    #[test]
    fn rows_resolve_names_group_and_split_images() {
        let mut state = ClientState::new();
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s",
                "user": { "id": "100", "username": "me" },
                "guilds": [{
                    "id": "1",
                    "properties": { "name": "G", "owner_id": "100" },
                    "roles": [{ "id": "1", "name": "@everyone", "permissions": "0" },
                              { "id": "7", "name": "Mods", "permissions": "0" }],
                    "channels": [{ "id": "11", "type": 0, "name": "general", "position": 0 }],
                    "members": [{ "user": { "id": "200", "username": "alice", "global_name": "Alice" },
                                  "nick": "Ali", "roles": [] }]
                }],
                "private_channels": [],
                "read_states": []
            }),
        });
        let msg = |id: &str, author: &str, ts: &str, content: &str, attachments: serde_json::Value| -> Message {
            serde_json::from_value(json!({
                "id": id, "channel_id": "11", "content": content, "timestamp": ts,
                "author": { "id": author, "username": format!("user{author}") },
                "mentions": [{ "id": "300", "username": "carol", "global_name": "Carol" }],
                "attachments": attachments
            }))
            .expect("message")
        };
        let page = vec![
            msg("1004", "300", "2026-11-18T10:20:00+00:00", "late", json!([])),
            msg("1003", "200", "2026-11-18T10:06:00+00:00", "img", json!([
                { "id": "1", "filename": "a.png", "content_type": "image/png", "url": "https://cdn/a.png",
                  "width": 800, "height": 600 },
                { "id": "2", "filename": "b.zip", "content_type": "application/zip", "url": "https://cdn/b.zip" }
            ])),
            msg("1002", "200", "2026-11-18T10:01:00+00:00", "<@200> <@300> <#11> <@&7> <:wave:5>", json!([])),
        ];
        state.messages.load(Snowflake(11), page, LoadMode::Replace);
        let utc = FixedOffset::east_opt(0).expect("offset");
        let now = DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00").expect("now").with_timezone(&utc);
        let pane = message_pane_at(&state, Snowflake(11), Some("https://media"), None, &now);
        let rows = &pane.rows;
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].body.as_ref(), "<@200> <@300> <#11> <@&7> <:wave:5>");
        let document = rows[0].document.as_ref().expect("parsed body");
        assert_eq!(document.plain_text(), "@Ali @Carol general @Mods <:wave:5>");
        assert_eq!(rows[0].author.as_ref(), "Ali", "member nick is the author name");
        assert_eq!(rows[0].time.as_ref(), "11/18/2026, 10:01 AM");
        let starts: Vec<bool> = rows.iter().map(|r| r.group_start).collect();
        assert_eq!(starts, [true, false, true]);
        assert_eq!(
            rows[1].images,
            [ImagePreview {
                url: "https://cdn/a.png".into(),
                proxy_url: None,
                animated: false,
                width: 400.,
                height: 300.
            }]
        );
        assert_eq!(rows[1].body.as_ref(), "img\n\n📎 [b.zip](https://cdn/b.zip)");
    }

    fn ready_one_channel(state: &mut ClientState) {
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s",
                "user": { "id": "100", "username": "me" },
                "guilds": [{
                    "id": "1",
                    "properties": { "name": "G", "owner_id": "100" },
                    "roles": [{ "id": "1", "name": "@everyone", "permissions": "0" }],
                    "channels": [{ "id": "11", "type": 0, "name": "general", "position": 0 }],
                    "members": [{ "user": { "id": "200", "username": "tester2" }, "roles": [] }]
                }],
                "private_channels": [],
                "read_states": []
            }),
        });
    }

    fn page_message(id: Snowflake, author: &str, kind: u8, ts: &str) -> Message {
        serde_json::from_value(json!({
            "id": id.0.to_string(), "channel_id": "11", "content": "", "timestamp": ts, "type": kind,
            "author": { "id": author, "username": format!("user{author}") },
        }))
        .expect("message")
    }

    /// Date separators before the first message of each day, the NEW divider
    /// before the first message after the anchor, the channel-start header
    /// once the history is complete, and system rows for joins.
    #[test]
    fn dividers_and_system_rows() {
        let mut state = ClientState::new();
        ready_one_channel(&mut state);
        let at = |s: &str| Snowflake::from_unix_ms(DateTime::parse_from_rfc3339(s).expect("ts").timestamp_millis() as u64);
        let ts = ["2026-11-17T23:00:00+00:00", "2026-11-18T01:17:00+00:00", "2026-11-18T01:18:00+00:00", "2026-11-18T01:19:00+00:00"];
        let ids: Vec<Snowflake> = ts.iter().map(|t| at(t)).collect();
        // Newest first, one short page: no more history.
        let page = vec![
            page_message(ids[3], "300", 0, ts[3]),
            page_message(ids[2], "300", 0, ts[2]),
            page_message(ids[1], "200", 7, ts[1]),
            page_message(ids[0], "300", 0, ts[0]),
        ];
        state.messages.load(Snowflake(11), page, LoadMode::Replace);
        let utc = FixedOffset::east_opt(0).expect("offset");
        let now = DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00").expect("now").with_timezone(&utc);
        let pane = message_pane_at(&state, Snowflake(11), None, Some(ids[1]), &now);
        let rows = &pane.rows;
        assert!(!pane.has_more_before);
        let days: Vec<Option<&str>> = rows.iter().map(|r| r.day_divider.as_ref().map(|d| d.as_ref())).collect();
        assert_eq!(days, [Some("Tuesday, November 17, 2026"), Some("Wednesday, November 18, 2026"), None, None]);
        let new: Vec<bool> = rows.iter().map(|r| r.new_divider).collect();
        assert_eq!(new, [false, false, true, false]);
        assert_eq!(rows[0].channel_start.as_deref(), Some("#general"));
        assert!(rows[1..].iter().all(|r| r.channel_start.is_none()));
        let join = rows[1].system.as_ref().expect("join row");
        assert_eq!(join.icon, SystemIcon::Join);
        assert_eq!(join.bold.len(), 1);
        assert_eq!(&join.text[join.bold[0].clone()], "user200");
        assert!(rows.iter().enumerate().all(|(i, r)| (i == 1) == r.system.is_some()));
        // The message after a system row starts a group.
        assert!(rows[2].group_start && !rows[3].group_start);
        assert_eq!(rows[0].default_avatar, 300 % 6);
        // No anchor: no divider.
        let pane = message_pane_at(&state, Snowflake(11), None, None, &now);
        assert!(pane.rows.iter().all(|r| !r.new_divider));
    }

    /// A guild with a colored role, an uncolored one, a text, a voice and a
    /// category channel, a nicknamed member; a DM with a recipient.
    fn resolver_state() -> ClientState {
        let mut state = ClientState::new();
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s",
                "user": { "id": "100", "username": "me", "global_name": "Me" },
                "guilds": [{
                    "id": "1",
                    "properties": { "name": "G", "owner_id": "100" },
                    "roles": [{ "id": "1", "name": "@everyone", "permissions": "0" },
                              { "id": "7", "name": "Mods", "permissions": "0", "color": 0x3498db },
                              { "id": "8", "name": "Plain", "permissions": "0", "color": 0 }],
                    "channels": [{ "id": "11", "type": 0, "name": "general", "position": 0 },
                                 { "id": "12", "type": 2, "name": "Lounge", "position": 1 },
                                 { "id": "13", "type": 4, "name": "Info", "position": 2 }],
                    "members": [{ "user": { "id": "200", "username": "alice", "global_name": "Alice" },
                                  "nick": "Ali", "roles": [] }]
                }],
                "private_channels": [{ "id": "50", "type": 1,
                                       "recipients": [{ "id": "400", "username": "dave", "global_name": "Dave" }] }],
                "read_states": []
            }),
        });
        state
    }

    fn mention_message(channel: &str) -> Message {
        serde_json::from_value(json!({
            "id": "900", "channel_id": channel, "content": "", "timestamp": "2026-11-18T10:00:00+00:00",
            "author": { "id": "100", "username": "me" },
            "mentions": [{ "id": "300", "username": "carol", "global_name": "Carol" }]
        }))
        .expect("message")
    }

    #[test]
    fn resolver_names_users_by_nick_and_fallbacks() {
        let state = resolver_state();
        let m = mention_message("11");
        let guild = StateMentions { state: &state, guild_id: Some(Snowflake(1)), message: &m };
        assert_eq!(guild.user("200").as_deref(), Some("Ali"), "member nick");
        assert_eq!(guild.user("300").as_deref(), Some("Carol"), "from the message's mentions");
        assert_eq!(guild.user("100").as_deref(), Some("Me"), "the current user");
        assert_eq!(guild.user("400").as_deref(), Some("Dave"), "a DM recipient");
        assert_eq!(guild.user("999"), None);
        assert_eq!(guild.user("not-an-id"), None);
        // Outside the guild the nick does not apply.
        let dm = StateMentions { state: &state, guild_id: None, message: &m };
        assert_eq!(dm.user("200").as_deref(), Some("Alice"));
    }

    #[test]
    fn resolver_gives_role_colors_and_mentionable_channels() {
        let state = resolver_state();
        let m = mention_message("11");
        let guild = StateMentions { state: &state, guild_id: Some(Snowflake(1)), message: &m };
        assert_eq!(guild.role("7"), Some(ResolvedRole { name: "Mods".to_owned(), color: Some(0x3498db) }));
        assert_eq!(guild.role("8"), Some(ResolvedRole { name: "Plain".to_owned(), color: None }));
        assert_eq!(guild.role("9"), None);
        // In a DM roles are looked up in every guild.
        let dm = StateMentions { state: &state, guild_id: None, message: &m };
        assert_eq!(dm.role("7").map(|r| r.name).as_deref(), Some("Mods"));
        assert_eq!(guild.channel("11").as_deref(), Some("general"));
        assert_eq!(guild.channel("12").as_deref(), Some("Lounge"));
        assert_eq!(guild.channel("13"), None, "categories are not mentionable");
        assert_eq!(guild.channel("50"), None, "DMs are not mentionable");
        assert_eq!(guild.channel("77"), None);
    }

    /// Parsed bodies are reused across panes until the message is edited.
    #[test]
    fn documents_are_cached_until_edited() {
        let mut state = resolver_state();
        let msg = |id: &str, content: &str, edited: Option<&str>| -> Message {
            serde_json::from_value(json!({
                "id": id, "channel_id": "11", "content": content, "timestamp": "2026-11-18T10:00:00+00:00",
                "edited_timestamp": edited, "author": { "id": "200", "username": "alice" }
            }))
            .expect("message")
        };
        state.messages.load(Snowflake(11), vec![msg("1002", "b <@&7>", None), msg("1001", "a", None)], LoadMode::Replace);
        let utc = FixedOffset::east_opt(0).expect("offset");
        let now = DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00").expect("now").with_timezone(&utc);
        let docs = |state: &ClientState| -> Vec<Arc<Document>> {
            message_pane_at(state, Snowflake(11), None, None, &now)
                .rows
                .into_iter()
                .map(|r| r.document.expect("parsed body"))
                .collect()
        };
        let first = docs(&state);
        let again = docs(&state);
        assert!(first.iter().zip(&again).all(|(a, b)| Arc::ptr_eq(a, b)), "unchanged messages are not parsed again");
        let edited = vec![msg("1002", "b edited", Some("2026-11-18T11:00:00+00:00")), msg("1001", "a", None)];
        state.messages.load(Snowflake(11), edited, LoadMode::Replace);
        let after = docs(&state);
        assert!(Arc::ptr_eq(&first[0], &after[0]));
        assert!(!Arc::ptr_eq(&first[1], &after[1]));
        assert_eq!(after[1].plain_text(), "b edited");
        assert_eq!(first[1].plain_text(), "b @Mods");
    }

    #[test]
    fn format_file_size_formats_bytes_kb_mb() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(12_698), "12.4 KB");
        assert_eq!(format_file_size(2_202_010), "2.1 MB");
    }

    #[test]
    fn attachment_spoiler_explicit_and_file_rows_propagate() {
        let mut state = resolver_state();
        let m: Message = serde_json::from_value(json!({
            "id": "2001",
            "channel_id": "11",
            "content": "",
            "timestamp": "2026-11-18T10:00:00+00:00",
            "author": { "id": "200", "username": "alice" },
            "attachments": [
                {
                    "id": "1",
                    "filename": "flag_spoiler.png",
                    "content_type": "image/png",
                    "url": "https://cdn/1.png",
                    "width": 400,
                    "height": 300,
                    "flags": fluxer_client_core::state::ATTACHMENT_FLAG_SPOILER
                },
                {
                    "id": "2",
                    "filename": "SPOILER_prefix.png",
                    "content_type": "image/png",
                    "url": "https://cdn/2.png",
                    "width": 400,
                    "height": 300,
                    "flags": 0
                },
                {
                    "id": "3",
                    "filename": "explicit.png",
                    "content_type": "image/png",
                    "url": "https://cdn/3.png",
                    "width": 400,
                    "height": 300,
                    "flags": ATTACHMENT_FLAG_EXPLICIT
                },
                {
                    "id": "4",
                    "filename": "SPOILER_report.pdf",
                    "content_type": "application/pdf",
                    "size": 12_698,
                    "url": "https://cdn/report.pdf",
                    "flags": 0
                }
            ]
        }))
        .expect("message");
        state.messages.load(Snowflake(11), vec![m], LoadMode::Replace);
        let utc = FixedOffset::east_opt(0).expect("offset");
        let now = DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00").expect("now").with_timezone(&utc);
        let pane = message_pane_at(&state, Snowflake(11), None, None, &now);
        assert_eq!(pane.rows.len(), 1);
        let row = &pane.rows[0];
        assert_eq!(
            row.image_covers,
            [
                ImageCover { spoiler: true, nsfw: false },
                ImageCover { spoiler: true, nsfw: false },
                ImageCover { spoiler: false, nsfw: true },
            ]
        );
        assert_eq!(
            row.file_attachments,
            [FileAttachmentRow {
                id: "4".to_owned(),
                filename: "SPOILER_report.pdf".to_owned(),
                size_label: "12.4 KB".to_owned(),
                url: "https://cdn/report.pdf".to_owned(),
                spoiler: true,
                expiry: None,
            }]
        );
    }

    #[test]
    fn hide_muted_channels_follows_visibility_rules() {
        let mut state = resolver_state();
        let guild = Snowflake(1);
        state.apply(&Dispatch {
            t: "GUILD_CREATE".to_owned(),
            s: 2,
            d: json!({
                "id": guild.to_string(),
                "properties": { "name": "Dev Guild" },
                "roles": [{ "id": guild.to_string(), "name": "@everyone", "permissions": fluxer_client_core::state::bits::VIEW_CHANNEL.to_string() }],
                "channels": [
                    { "id": "10", "type": 4, "name": "Cat", "position": 0 },
                    { "id": "11", "type": 0, "name": "open", "parent_id": "10", "position": 0 },
                    { "id": "12", "type": 0, "name": "muted-plain", "parent_id": "10", "position": 1 },
                    { "id": "13", "type": 0, "name": "muted-selected", "parent_id": "10", "position": 2 },
                    { "id": "14", "type": 0, "name": "muted-mentioned", "parent_id": "10", "position": 3 },
                    { "id": "20", "type": 4, "name": "All Muted Cat", "position": 1 },
                    { "id": "21", "type": 0, "name": "hidden-child", "parent_id": "20", "position": 0 }
                ],
                "members": [{ "user": { "id": "100", "username": "me" }, "roles": [] }]
            }),
        });
        state.apply(&Dispatch {
            t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
            s: 3,
            d: json!({
                "guild_id": guild.to_string(),
                "hide_muted_channels": true,
                "channel_overrides": {
                    "12": { "channel_id": "12", "muted": true },
                    "13": { "channel_id": "13", "muted": true },
                    "14": { "channel_id": "14", "muted": true },
                    "21": { "channel_id": "21", "muted": true }
                }
            }),
        });
        state.apply(&Dispatch {
            t: "MESSAGE_CREATE".to_owned(),
            s: 4,
            d: json!({
                "id": "999",
                "channel_id": "14",
                "content": "<@100>",
                "timestamp": "2026-11-18T10:00:00+00:00",
                "author": { "id": "200", "username": "alice" },
                "mentions": [{ "id": "100", "username": "me" }]
            }),
        });

        let ids: Vec<u64> = channel_items_with_active(&state, Place::Guild(guild), None, Some(Snowflake(13)), None)
            .into_iter()
            .map(|c| c.id.0)
            .collect();
        // Cat (10) + open (11) + selected muted (13) + mentioned muted (14); muted-plain (12) and All Muted Cat (20, 21) hidden.
        assert_eq!(ids, vec![10, 11, 13, 14]);
    }
}
