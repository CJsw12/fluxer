//! `ClientState`: the entity caches, the dispatch router and cross-domain
//! UI queries. Each event's reducer lives in `handlers/<domain>.rs`; a new
//! event is one arm in [`ClientState::apply_at`] plus a method in its
//! domain file. Mirrors the foreground MobX stores where they differ from
//! the snapshot reducer (mention counting on MESSAGE_CREATE), see
//! `STATE_MODEL.md`.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::handlers::emojis::GuildEmoji;
use super::handlers::member_list::MemberLists;
use super::handlers::presence::Presence;
use super::handlers::relationships::Relationship;
use super::handlers::typing::Typer;
use super::handlers::user_settings::{UserGuildSettings, UserSettings};
use super::ids::Snowflake;
use super::messages::MessageStore;
use super::model::{Channel, ChannelKind, Guild, Member, ReadState, User, parse_timestamp_ms};
use super::permissions::{bits, compute_permissions, has, thread_view_permissions};
use crate::Dispatch;

/// What changed, so the UI knows what to redraw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum StateChange {
    /// Everything was replaced (READY).
    Ready,
    /// Guild added or its name/icon/owner/count changed (guild list).
    GuildUpserted { guild_id: Snowflake },
    /// Guild left, deleted or became unavailable (outage).
    GuildRemoved { guild_id: Snowflake, unavailable: bool },
    /// Channel created/updated. `guild_id: None` = DM list.
    ChannelUpserted { channel_id: Snowflake, guild_id: Option<Snowflake> },
    ChannelRemoved { channel_id: Snowflake, guild_id: Option<Snowflake> },
    /// Roles or the current user's member changed: recompute visibility
    /// and send permission for every channel of the guild.
    PermissionsChanged { guild_id: Snowflake },
    MemberUpdated { guild_id: Snowflake, user_id: Snowflake },
    UserUpdated { user_id: Snowflake },
    /// The channel's message window changed.
    MessagesChanged { channel_id: Snowflake },
    /// Unread flag or mention count of the channel may have changed.
    ReadStateChanged { channel_id: Snowflake },
    /// The set of users typing in the channel changed.
    TypingChanged { channel_id: Snowflake },
    /// Another user's presence (status, custom status) changed.
    PresenceUpdated { user_id: Snowflake },
    /// Friend / block / request state with this user changed.
    RelationshipChanged { user_id: Snowflake },
    /// A user note changed (READY, USER_NOTE_UPDATE, local echo).
    NotesChanged,
    /// `user_settings` changed (theme, locale, status, guild folders).
    UserSettingsChanged,
    /// Notification settings of a guild (`None` = DMs) changed.
    GuildSettingsChanged { guild_id: Option<Snowflake> },
    /// The guild's custom emoji list changed.
    EmojisChanged { guild_id: Snowflake },
    /// A lazy member list changed (`GUILD_MEMBER_LIST_UPDATE`).
    MemberListChanged { guild_id: Snowflake, list_id: String, channel_id: Option<Snowflake> },
}

/// Unread indicator for a channel or guild.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Unread {
    pub unread: bool,
    pub mention_count: u32,
}

/// A category with its children, or a top-level channel (`children` empty).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelTreeNode {
    pub channel: Channel,
    pub children: Vec<Channel>,
}

/// Client state for one account. Network-free: feed it `Dispatch` frames
/// and REST results.
#[derive(Debug, Default)]
pub struct ClientState {
    pub session_id: Option<String>,
    pub(super) ready_generation: u64,
    pub current_user: Option<User>,
    pub(super) current_user_mfa_enabled: Option<bool>,
    pub users: BTreeMap<Snowflake, User>,
    pub guilds: BTreeMap<Snowflake, Guild>,
    pub unavailable_guilds: BTreeSet<Snowflake>,
    pub channels: BTreeMap<Snowflake, Channel>,
    pub(super) denied_channel_access: BTreeSet<Snowflake>,
    /// guild id -> user id -> member. READY carries only some members;
    /// the current user's member is the one that matters here.
    pub members: BTreeMap<Snowflake, BTreeMap<Snowflake, Member>>,
    pub read_states: BTreeMap<Snowflake, ReadState>,
    /// Guild order from `user_settings.guild_folders[].guild_ids`.
    pub guild_order: Vec<Snowflake>,
    pub messages: MessageStore,
    /// user id -> relationship (friends, blocks, requests).
    pub relationships: BTreeMap<Snowflake, Relationship>,
    /// user id -> presence of other users; the current user is never
    /// stored (TS `Presence.handlePresenceUpdate`). Missing = offline.
    pub presences: BTreeMap<Snowflake, Presence>,
    /// user id -> private note shown on their profile.
    pub notes: BTreeMap<Snowflake, String>,
    /// channel id -> users typing there, in start order.
    pub typing: BTreeMap<Snowflake, Vec<Typer>>,
    pub user_settings: UserSettings,
    /// guild id (`None` = DMs) -> the user's notification settings.
    pub guild_settings: BTreeMap<Option<Snowflake>, UserGuildSettings>,
    /// guild id -> custom emojis.
    pub emojis: BTreeMap<Snowflake, Vec<GuildEmoji>>,
    /// Lazy member lists (`GUILD_MEMBER_LIST_UPDATE`).
    pub member_lists: MemberLists,
}

/// `channelLayoutRank`: text-like channels, then voice, then the rest.
fn sidebar_rank(kind: ChannelKind) -> u8 {
    match kind {
        ChannelKind::GuildText
        | ChannelKind::GuildAnnouncement
        | ChannelKind::GuildLink
        | ChannelKind::GuildForum
        | ChannelKind::GuildMedia => 0,
        ChannelKind::GuildVoice => 1,
        _ => 2,
    }
}

impl ClientState {
    pub fn ready_generation(&self) -> u64 {
        self.ready_generation
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_user_id(&self) -> Option<Snowflake> {
        self.current_user.as_ref().map(|u| u.id)
    }

    /// Apply one gateway dispatch at the current wall-clock time. Unknown
    /// events are ignored.
    pub fn apply(&mut self, dispatch: &Dispatch) -> Vec<StateChange> {
        self.apply_at(dispatch, now_ms())
    }

    /// [`Self::apply`] with an explicit clock (Unix ms), used for typing
    /// expiry. One arm per event; the reducers live in `handlers/`.
    pub fn apply_at(&mut self, dispatch: &Dispatch, now_ms: i64) -> Vec<StateChange> {
        let d = &dispatch.d;
        let mut out = Vec::new();
        let out_ref = &mut out;
        match dispatch.t.as_str() {
            // handlers/ready.rs, handlers/users.rs
            "READY" => self.apply_ready(d, out_ref),
            "USER_UPDATE" => self.apply_user_update(d, out_ref),
            // handlers/guilds.rs
            "GUILD_CREATE" | "GUILD_SYNC" => self.apply_guild_create(d, out_ref),
            "GUILD_UPDATE" => self.apply_guild_update(d, out_ref),
            "GUILD_DELETE" => self.apply_guild_delete(d, out_ref),
            "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => self.apply_role_upsert(d, out_ref),
            "GUILD_ROLE_UPDATE_BULK" => self.apply_role_bulk(d, out_ref),
            "GUILD_ROLE_DELETE" => self.apply_role_delete(d, out_ref),
            // handlers/emojis.rs
            "GUILD_EMOJIS_UPDATE" => self.apply_guild_emojis_update(d, out_ref),
            // handlers/members.rs, handlers/member_list.rs
            "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => self.apply_member_upsert(d, out_ref),
            "GUILD_MEMBER_REMOVE" => self.apply_member_remove(d, out_ref),
            "GUILD_MEMBER_LIST_UPDATE" => self.apply_member_list_update(d, out_ref),
            // handlers/channels.rs
            "CHANNEL_CREATE" => self.apply_channel_create(d, out_ref),
            "CHANNEL_UPDATE" => {
                if dispatch.s != 0 && d.get("permission_overwrites").is_some()
                    && let Some(id) = d.get("id").and_then(|id| id.as_str()).and_then(Snowflake::parse)
                {
                    self.denied_channel_access.remove(&id);
                }
                self.apply_channel_update(d, out_ref);
            }
            "CHANNEL_UPDATE_BULK" => self.apply_channel_update_bulk(d, out_ref),
            "CHANNEL_DELETE" => self.apply_channel_delete(d, out_ref),
            "CHANNEL_RECIPIENT_ADD" => self.apply_channel_recipient_add(d, out_ref),
            "CHANNEL_RECIPIENT_REMOVE" => self.apply_channel_recipient_remove(d, out_ref),
            "PASSIVE_UPDATES" => self.apply_passive_updates(d, out_ref),
            // handlers/messages.rs
            "MESSAGE_CREATE" => self.apply_message_create(d, out_ref),
            "MESSAGE_UPDATE" => self.apply_message_update(d, out_ref),
            "MESSAGE_DELETE" => self.apply_message_delete(d, out_ref),
            "MESSAGE_DELETE_BULK" => self.apply_message_delete_bulk(d, out_ref),
            // handlers/reactions.rs
            "MESSAGE_REACTION_ADD" => self.apply_reaction_add(d, out_ref),
            "MESSAGE_REACTION_ADD_MANY" => self.apply_reaction_add_many(d, out_ref),
            "MESSAGE_REACTION_REMOVE" => self.apply_reaction_remove(d, out_ref),
            "MESSAGE_REACTION_REMOVE_ALL" => self.apply_reaction_remove_all(d, out_ref),
            "MESSAGE_REACTION_REMOVE_EMOJI" => self.apply_reaction_remove_emoji(d, out_ref),
            // handlers/read_states.rs
            "MESSAGE_ACK" => self.apply_message_ack(d, out_ref),
            // handlers/typing.rs
            "TYPING_START" => self.apply_typing_start(d, now_ms, out_ref),
            // handlers/presence.rs
            "PRESENCE_UPDATE" => self.apply_presence_update(d, out_ref),
            "PRESENCE_UPDATE_BULK" => self.apply_presence_update_bulk(d, out_ref),
            // handlers/relationships.rs
            "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => self.apply_relationship_upsert(d, out_ref),
            "RELATIONSHIP_REMOVE" => self.apply_relationship_remove(d, out_ref),
            // handlers/notes.rs
            "USER_NOTE_UPDATE" => self.apply_user_note_update(d, out_ref),
            // handlers/user_settings.rs
            "USER_SETTINGS_UPDATE" => self.apply_user_settings_update(d, out_ref),
            "USER_GUILD_SETTINGS_UPDATE" => self.apply_user_guild_settings_update(d, out_ref),
            other => tracing::debug!(event = other, "ignored dispatch"),
        }
        out
    }

    // ---- queries --------------------------------------------------------

    /// Guilds in folder order; guilds not in any folder first, by name
    /// (`GuildList.sortGuildArrayInPlace`).
    pub fn sorted_guilds(&self) -> Vec<&Guild> {
        let index = |id: Snowflake| self.guild_order.iter().position(|g| *g == id);
        let mut list: Vec<&Guild> = self.guilds.values().collect();
        list.sort_by(|a, b| match (index(a.id), index(b.id)) {
            (None, None) => a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => x.cmp(&y),
        });
        list
    }

    /// Channel tree for a guild in sidebar order (`ChannelOrganization.ts`
    /// `organizeChannels`): channels without a category first, then each
    /// category (by position, id) with its children. Inside each group text
    /// channels (text, announcement, link, forum, media) come before voice
    /// channels, each sorted by (position, id) (`compareChannelOrdering`).
    /// Threads and channels the current user cannot view are omitted;
    /// categories with no visible children are hidden.
    pub fn channel_tree(&self, guild_id: Snowflake) -> Vec<ChannelTreeNode> {
        let mut visible: Vec<&Channel> = self
            .channels
            .values()
            .filter(|c| c.guild_id == Some(guild_id) && !c.kind.is_thread() && self.can_view_channel(c.id))
            .collect();
        visible.sort_by_key(|c| (sidebar_rank(c.kind), c.position, c.id));
        let category_ids: BTreeSet<Snowflake> = self
            .channels
            .values()
            .filter(|c| c.guild_id == Some(guild_id) && c.kind.is_category())
            .map(|c| c.id)
            .collect();
        let in_category = |c: &Channel| c.parent_id.is_some_and(|p| category_ids.contains(&p));
        let mut nodes: Vec<ChannelTreeNode> = visible
            .iter()
            .filter(|c| !c.kind.is_category() && !in_category(c))
            .map(|c| ChannelTreeNode { channel: (*c).clone(), children: Vec::new() })
            .collect();
        for category in visible.iter().filter(|c| c.kind.is_category()) {
            let children: Vec<Channel> = visible
                .iter()
                .filter(|c| !c.kind.is_category() && c.parent_id == Some(category.id))
                .map(|c| (*c).clone())
                .collect();
            // Categories with no visible children are hidden to members, but shown
            // to managers so newly created empty categories stay reachable.
            if !children.is_empty() || self.can_manage_channel(category.id) || self.can_manage_channel_permissions(category.id) {
                nodes.push(ChannelTreeNode { channel: (*category).clone(), children });
            }
        }
        nodes
    }

    /// DM and group DM channels, newest activity first.
    pub fn dm_channels(&self) -> Vec<&Channel> {
        let mut list: Vec<&Channel> = self
            .channels
            .values()
            .filter(|c| c.guild_id.is_none() && c.kind.is_private())
            .collect();
        list.sort_by(|a, b| {
            let ka = a.last_message_id.unwrap_or(a.id);
            let kb = b.last_message_id.unwrap_or(b.id);
            kb.cmp(&ka)
        });
        list
    }

    /// `ReadStateEntryStatusMachine.isUnread`: unread if the ack is older
    /// than the channel's newest message. Without an ack, the newest message
    /// must be newer than the ack floor (`ReadStateEntry.ackFloorTimestamp`):
    /// the guild's `joined_at`, else the channel's creation time.
    pub fn channel_unread(&self, channel_id: Snowflake) -> Unread {
        let Some(channel) = self.channels.get(&channel_id) else {
            return Unread::default();
        };
        if !channel.kind.has_messages() {
            return Unread::default();
        }
        let rs = self.read_states.get(&channel_id);
        let unread = match (channel.last_message_id, rs.and_then(|r| r.ack_message_id)) {
            (None, _) => false,
            (Some(last), Some(ack)) => ack < last,
            (Some(last), None) => {
                let floor = self.ack_floor_ms(channel);
                i64::try_from(last.timestamp_ms()).is_ok_and(|ts| ts > floor)
            }
        };
        Unread { unread, mention_count: rs.map_or(0, |r| r.mention_count) }
    }

    /// Where the "NEW" divider goes when an unread channel is opened: messages
    /// with a larger id are unread. The ack, else the ack floor as the
    /// smallest snowflake of that millisecond. `None` when nothing is unread.
    pub fn unread_anchor(&self, channel_id: Snowflake) -> Option<Snowflake> {
        if !self.channel_unread(channel_id).unread {
            return None;
        }
        let channel = self.channels.get(&channel_id)?;
        match self.read_states.get(&channel_id).and_then(|r| r.ack_message_id) {
            Some(ack) => Some(ack),
            None => {
                let floor = u64::try_from(self.ack_floor_ms(channel)).unwrap_or(0);
                // Messages in the floor's own millisecond are not newer.
                Some(Snowflake(Snowflake::from_unix_ms(floor.saturating_add(1)).0.saturating_sub(1)))
            }
        }
    }

    /// `ReadStateEntry.ackFloorTimestamp`: guild `joined_at`, else the
    /// channel's creation time, in Unix ms.
    fn ack_floor_ms(&self, channel: &Channel) -> i64 {
        let joined_at = channel
            .guild_id
            .and_then(|g| self.guilds.get(&g))
            .and_then(|g| g.joined_at.as_deref())
            .and_then(parse_timestamp_ms)
            .filter(|ms| *ms != 0);
        joined_at.unwrap_or_else(|| i64::try_from(channel.id.timestamp_ms()).unwrap_or(i64::MAX))
    }

    /// Any viewable channel unread; mentions summed.
    pub fn guild_unread(&self, guild_id: Snowflake) -> Unread {
        let mut total = Unread::default();
        for c in self.channels.values().filter(|c| c.guild_id == Some(guild_id)) {
            if !self.can_view_channel(c.id) {
                continue;
            }
            let u = self.channel_unread(c.id);
            total.unread |= u.unread;
            total.mention_count = total.mention_count.saturating_add(u.mention_count);
        }
        total
    }

    /// Current user's guild-level permissions (before channel overwrites);
    /// 0 when not signed in or the guild is unknown.
    pub fn guild_permissions(&self, guild_id: Snowflake) -> u64 {
        let Some(me) = self.current_user_id() else { return 0 };
        let Some(guild) = self.guilds.get(&guild_id) else { return 0 };
        let member = self.members.get(&guild_id).and_then(|m| m.get(&me));
        compute_permissions(guild, me, member, None)
    }

    /// Current user's permissions in a guild channel; 0 for DMs and unknown
    /// channels. Threads derive from their parent channel
    /// (`Permission.ts:117-140`, `thread_view_permissions`); a thread whose
    /// parent is unknown gets 0.
    pub fn channel_permissions(&self, channel_id: Snowflake) -> u64 {
        if self.has_denied_channel_access(channel_id) {
            return 0;
        }
        let Some(channel) = self.channels.get(&channel_id) else { return 0 };
        if channel.kind.is_thread() {
            return match channel.parent_id {
                Some(parent) if self.channels.get(&parent).is_some_and(|p| !p.kind.is_thread()) => {
                    thread_view_permissions(self.channel_permissions(parent))
                }
                _ => 0,
            };
        }
        let Some(me) = self.current_user_id() else { return 0 };
        let Some(guild) = channel.guild_id.and_then(|g| self.guilds.get(&g)) else { return 0 };
        let member = self.members.get(&guild.id).and_then(|m| m.get(&me));
        compute_permissions(guild, me, member, Some(channel))
    }

    pub fn has_denied_channel_access(&self, channel: Snowflake) -> bool {
        self.denied_channel_access.contains(&channel)
    }

    /// A confirmed REST access denial can arrive before a permission update.
    pub fn set_denied_channel_access(&mut self, channel: Snowflake, denied: bool) {
        if denied {
            self.denied_channel_access.insert(channel);
        } else {
            self.denied_channel_access.remove(&channel);
        }
    }

    /// Private channels are always viewable (they come from READY /
    /// CHANNEL_CREATE for this user only).
    pub fn can_view_channel(&self, channel_id: Snowflake) -> bool {
        match self.channels.get(&channel_id) {
            Some(c) if c.kind.is_private() => true,
            Some(_) => has(self.channel_permissions(channel_id), bits::VIEW_CHANNEL),
            None => false,
        }
    }

    /// `ChannelTextarea.tsx:23-33` at the current wall-clock time.
    pub fn can_send_messages(&self, channel_id: Snowflake) -> bool {
        self.can_send_messages_at(channel_id, now_ms())
    }

    /// `ChannelTextarea.tsx:23-33`: private channels can always send; guild
    /// channels need VIEW_CHANNEL and SEND_MESSAGES and the current user
    /// must not be timed out (`GuildMembers.isUserTimedOut`, no owner or
    /// admin exemption in the UI). Slowmode is not checked.
    pub fn can_send_messages_at(&self, channel_id: Snowflake, now_ms: i64) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        if channel.kind.is_private() {
            return true;
        }
        if !channel.kind.has_messages()
            || !has(self.channel_permissions(channel_id), bits::VIEW_CHANNEL | bits::SEND_MESSAGES)
        {
            return false;
        }
        let timed_out = channel
            .guild_id
            .zip(self.current_user_id())
            .and_then(|(g, me)| self.members.get(&g)?.get(&me))
            .is_some_and(|m| m.is_timed_out(now_ms));
        !timed_out
    }
}

/// Wall-clock Unix milliseconds.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
