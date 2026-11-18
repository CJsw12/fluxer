//! Wire entity types. Unknown JSON fields are ignored; `null` on a
//! non-optional field falls back to the default so one odd field does not
//! drop a whole entity.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::content::{Attachment, Embed, MessageReference, Reaction, Sticker, message_flags};
use super::ids::{Snowflake, bitfield_str};

/// `null` or missing -> `T::default()`.
pub(crate) fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Nonces are strings on the wire but clients may send numbers.
fn nonce_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<serde_json::Value>::deserialize(deserializer)? {
        Some(serde_json::Value::String(s)) => Some(s),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    })
}

/// Shallow JSON merge `{...base, ...patch}` (TS `CHANNEL_UPDATE`,
/// `MESSAGE_UPDATE` `withUpdates`, `USER_UPDATE`). Fields not in `patch`
/// keep their current value, including client-only fields.
pub(crate) fn overlay<T: Serialize + DeserializeOwned>(
    base: &T,
    patch: &serde_json::Value,
) -> Result<T, serde_json::Error> {
    let mut merged = serde_json::to_value(base)?;
    if let (Some(dst), Some(src)) = (merged.as_object_mut(), patch.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    serde_json::from_value(merged)
}

/// `UserPartial` subset (`UserResponseSchemas.ts:54-74`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: Snowflake,
    #[serde(default, deserialize_with = "null_default")]
    pub username: String,
    #[serde(default)]
    pub discriminator: Option<String>,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub bot: bool,
}

impl User {
    /// `global_name` if set, else `username`.
    pub fn display_name(&self) -> &str {
        match &self.global_name {
            Some(name) if !name.is_empty() => name,
            _ => &self.username,
        }
    }
}

/// Guild role (`GuildRoleSchemas.ts:9-20`). @everyone has `id == guild_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Role {
    pub id: Snowflake,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub color: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub position: i32,
    #[serde(with = "bitfield_str", default)]
    pub permissions: u64,
    #[serde(default, deserialize_with = "null_default")]
    pub hoist: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub mentionable: bool,
}

/// Channel permission overwrite. `kind` 0 = role, 1 = member.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionOverwrite {
    pub id: Snowflake,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(with = "bitfield_str", default)]
    pub allow: u64,
    #[serde(with = "bitfield_str", default)]
    pub deny: u64,
}

/// Guild (READY `GuildReadyData` flattened with `properties`, or wire
/// `Guild` from GUILD_UPDATE). Roles live here so permission math needs
/// only the guild.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Guild {
    pub id: Snowflake,
    pub name: String,
    pub icon: Option<String>,
    pub owner_id: Option<Snowflake>,
    pub joined_at: Option<String>,
    pub member_count: Option<u64>,
    /// Guild-visit (preview) session; permissions are masked.
    pub visiting: bool,
    pub roles: BTreeMap<Snowflake, Role>,
    /// `default_message_notifications` (ALL_MESSAGES = 0, ONLY_MENTIONS = 1).
    pub default_message_notifications: i8,
    /// `LARGE_GUILD_OVERRIDE` or `VERY_LARGE_GUILD` in `features`.
    pub large_feature: bool,
}

/// `LARGE_GUILD_THRESHOLD` (`GatewayConstants.ts`).
pub const LARGE_GUILD_THRESHOLD: u64 = 250;

impl Guild {
    /// `Guild.effectiveMessageNotifications`: large guilds (by feature or
    /// over [`LARGE_GUILD_THRESHOLD`] members) default to ONLY_MENTIONS;
    /// otherwise the guild's own default. An unknown member count only
    /// honours the default.
    pub fn effective_message_notifications(&self) -> i8 {
        let Some(count) = self.member_count else { return self.default_message_notifications };
        if self.large_feature || count > LARGE_GUILD_THRESHOLD {
            return super::handlers::user_settings::message_notifications::ONLY_MENTIONS;
        }
        self.default_message_notifications
    }
}

/// Fields of the wire `Guild` the client keeps (`GuildResponseSchemas.ts:143-191`).
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct GuildProperties {
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub owner_id: Option<Snowflake>,
    #[serde(default, deserialize_with = "null_default")]
    pub default_message_notifications: i8,
    #[serde(default, deserialize_with = "null_default")]
    pub features: Vec<String>,
}

impl GuildProperties {
    pub(crate) fn large_feature(&self) -> bool {
        self.features.iter().any(|f| f == "LARGE_GUILD_OVERRIDE" || f == "VERY_LARGE_GUILD")
    }
}

/// Channel type (`ChannelConstants.ts:5-19`). Values above 255 exist
/// (998, 999), so the raw value is `u16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChannelKind {
    #[default]
    GuildText,
    Dm,
    GuildVoice,
    GroupDm,
    GuildCategory,
    GuildAnnouncement,
    AnnouncementThread,
    PublicThread,
    PrivateThread,
    GuildForum,
    GuildMedia,
    GuildLink,
    DmPersonalNotes,
    Unknown(u16),
}

impl ChannelKind {
    pub fn from_raw(v: u16) -> Self {
        match v {
            0 => Self::GuildText,
            1 => Self::Dm,
            2 => Self::GuildVoice,
            3 => Self::GroupDm,
            4 => Self::GuildCategory,
            5 => Self::GuildAnnouncement,
            10 => Self::AnnouncementThread,
            11 => Self::PublicThread,
            12 => Self::PrivateThread,
            15 => Self::GuildForum,
            16 => Self::GuildMedia,
            998 => Self::GuildLink,
            999 => Self::DmPersonalNotes,
            other => Self::Unknown(other),
        }
    }

    pub fn raw(self) -> u16 {
        match self {
            Self::GuildText => 0,
            Self::Dm => 1,
            Self::GuildVoice => 2,
            Self::GroupDm => 3,
            Self::GuildCategory => 4,
            Self::GuildAnnouncement => 5,
            Self::AnnouncementThread => 10,
            Self::PublicThread => 11,
            Self::PrivateThread => 12,
            Self::GuildForum => 15,
            Self::GuildMedia => 16,
            Self::GuildLink => 998,
            Self::DmPersonalNotes => 999,
            Self::Unknown(v) => v,
        }
    }

    /// TS `Channel.isPrivate()`: DM, group DM, personal notes.
    pub fn is_private(self) -> bool {
        matches!(self, Self::Dm | Self::GroupDm | Self::DmPersonalNotes)
    }

    /// `TEXT_BASED_CHANNEL_TYPES` (voice is text-based too).
    pub fn is_text_based(self) -> bool {
        matches!(
            self,
            Self::GuildText
                | Self::GuildVoice
                | Self::GuildAnnouncement
                | Self::Dm
                | Self::DmPersonalNotes
                | Self::GroupDm
        )
    }

    pub fn is_thread(self) -> bool {
        matches!(self, Self::AnnouncementThread | Self::PublicThread | Self::PrivateThread)
    }

    /// Snapshot reducer `isMessageChannelType`: text-based or thread.
    pub fn has_messages(self) -> bool {
        self.is_text_based() || self.is_thread()
    }

    pub fn is_category(self) -> bool {
        self == Self::GuildCategory
    }
}

impl Serialize for ChannelKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u16(self.raw())
    }
}

impl<'de> Deserialize<'de> for ChannelKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_raw(u16::deserialize(deserializer)?))
    }
}

/// Channel (`ChannelSchemas.ts:51-113`), first-milestone subset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: Snowflake,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: ChannelKind,
    #[serde(default, deserialize_with = "null_default")]
    pub position: i32,
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    #[serde(default)]
    pub last_message_id: Option<Snowflake>,
    #[serde(default, deserialize_with = "null_default")]
    pub permission_overwrites: Vec<PermissionOverwrite>,
    #[serde(default, deserialize_with = "null_default")]
    pub recipients: Vec<User>,
    #[serde(default, deserialize_with = "null_default")]
    pub nsfw: bool,
    #[serde(default)]
    pub rate_limit_per_user: Option<u32>,
}

/// Guild member (`GuildMemberSchemas.ts`), subset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub user: User,
    #[serde(default)]
    pub nick: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub roles: Vec<Snowflake>,
    #[serde(default)]
    pub joined_at: Option<String>,
    #[serde(default)]
    pub communication_disabled_until: Option<String>,
}

impl Member {
    /// TS `GuildMember.isTimedOut()`: `communication_disabled_until > now`.
    /// An unparsable timestamp counts as not timed out.
    pub fn is_timed_out(&self, now_ms: i64) -> bool {
        self.communication_disabled_until
            .as_deref()
            .and_then(parse_timestamp_ms)
            .is_some_and(|until| until > now_ms)
    }
}

/// Parses an RFC 3339 / ISO 8601 date-time (`2026-11-18T12:34:56.789+00:00`,
/// `...Z`, or no offset = UTC) to Unix milliseconds. Fractions beyond
/// milliseconds are truncated. Returns `None` for anything else.
pub fn parse_timestamp_ms(s: &str) -> Option<i64> {
    let num = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = s.get(range)?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let byte = |i: usize| s.as_bytes().get(i).copied();
    if byte(4) != Some(b'-') || byte(7) != Some(b'-') || byte(13) != Some(b':') || byte(16) != Some(b':') {
        return None;
    }
    if !matches!(byte(10), Some(b'T' | b't' | b' ')) {
        return None;
    }
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let mut i = 19;
    let mut millis = 0;
    if byte(i) == Some(b'.') {
        i += 1;
        let start = i;
        while byte(i).is_some_and(|b| b.is_ascii_digit()) {
            i += 1;
        }
        if i == start {
            return None;
        }
        let digits = s.get(start..i.min(start + 3))?;
        millis = digits.parse::<i64>().ok()? * 10_i64.pow(3 - digits.len() as u32);
    }
    let offset_minutes = match byte(i) {
        None => 0,
        Some(b'Z' | b'z') if i + 1 == s.len() => 0,
        Some(sign @ (b'+' | b'-')) if i + 6 == s.len() && byte(i + 3) == Some(b':') => {
            let total = num(i + 1..i + 3)? * 60 + num(i + 4..i + 6)?;
            if sign == b'+' { total } else { -total }
        }
        _ => return None,
    };

    // Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_minutes * 60;
    Some(seconds * 1_000 + millis)
}

/// Per-channel read state (snapshot `SnapshotReadStateRow` subset). The
/// channel's newest message id comes from `Channel::last_message_id`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReadState {
    pub ack_message_id: Option<Snowflake>,
    pub mention_count: u32,
    pub version: Option<Snowflake>,
    /// The server told us this read state (READY entry, ACK, own message).
    pub known: bool,
}

/// READY `read_states[]` item (`read_states/shared.ts:5-12`).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ReadStateWire {
    pub id: Snowflake,
    #[serde(default)]
    pub mention_count: Option<u32>,
    #[serde(default)]
    pub last_message_id: Option<Snowflake>,
    #[serde(default)]
    pub version: Option<Snowflake>,
}

/// Client-only delivery state (`MessageStates`; EDITING not used yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MessageState {
    Sending,
    #[default]
    Sent,
    Failed,
}

/// `referenced_message` keeps "absent" (`None`) apart from `null`
/// (`Some(None)`: the reply target is gone), as the schema requires. A
/// target that fails to parse is treated as absent.
fn referenced_message<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<Box<Message>>>, D::Error> {
    Ok(match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Null => Some(None),
        value => match serde_json::from_value::<Message>(value) {
            Ok(message) => Some(Some(Box::new(message))),
            Err(err) => {
                tracing::debug!(%err, "malformed referenced_message");
                None
            }
        },
    })
}

/// Message (`MessageResponseSchemas.ts:122-180`). Not ported: `tts`,
/// `mention_channels`, `users`, `message_snapshots`, `call`, `thread`.
/// The creation time is `id.timestamp_ms()`; pending messages have an empty
/// `timestamp` string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: Snowflake,
    pub channel_id: Snowflake,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    pub author: User,
    #[serde(default, deserialize_with = "null_default")]
    pub content: String,
    #[serde(default, deserialize_with = "null_default")]
    pub timestamp: String,
    #[serde(default)]
    pub edited_timestamp: Option<String>,
    /// Message type (`ChannelConstants.ts:57-71`), 0 = DEFAULT.
    #[serde(rename = "type", default, deserialize_with = "null_default")]
    pub kind: u8,
    #[serde(default, deserialize_with = "null_default")]
    pub flags: u64,
    #[serde(default, deserialize_with = "null_default")]
    pub pinned: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub mention_everyone: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub mentions: Vec<User>,
    #[serde(default, deserialize_with = "null_default")]
    pub mention_roles: Vec<Snowflake>,
    #[serde(default, deserialize_with = "null_default")]
    pub attachments: Vec<Attachment>,
    #[serde(default, deserialize_with = "null_default")]
    pub embeds: Vec<Embed>,
    #[serde(default, deserialize_with = "null_default")]
    pub stickers: Vec<Sticker>,
    #[serde(default, deserialize_with = "null_default")]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub message_reference: Option<MessageReference>,
    /// Reply target: `None` = key absent, `Some(None)` = target deleted,
    /// `Some(Some(_))` = resolved. See `ClientState::reply_preview`.
    #[serde(default, deserialize_with = "referenced_message", skip_serializing_if = "Option::is_none")]
    pub referenced_message: Option<Option<Box<Message>>>,
    #[serde(default)]
    pub webhook_id: Option<Snowflake>,
    #[serde(default, deserialize_with = "nonce_string")]
    pub nonce: Option<String>,
    /// Client-only; never on the wire.
    #[serde(default)]
    pub state: MessageState,
}

impl Message {
    pub fn created_at_ms(&self) -> u64 {
        self.id.timestamp_ms()
    }

    pub fn is_edited(&self) -> bool {
        self.edited_timestamp.is_some()
    }

    /// The message this one replies to (default-type reference only;
    /// forwards are not replies).
    pub fn reply_to(&self) -> Option<Snowflake> {
        self.message_reference.as_ref().filter(|r| r.kind == 0).and_then(|r| r.message_id)
    }

    pub fn suppresses_embeds(&self) -> bool {
        self.flags & message_flags::SUPPRESS_EMBEDS != 0
    }

    pub(crate) fn nonce_id(&self) -> Option<Snowflake> {
        self.nonce.as_deref().and_then(Snowflake::parse)
    }
}
