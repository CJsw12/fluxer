//! Permission bits and `computePermissions`
//! (`fluxer_app/src/features/permissions/utils/PermissionUtils.ts:105-201`).

use super::ids::Snowflake;
use super::model::{Channel, Guild, Member, Role};

/// Bit values from `packages/constants/src/ChannelConstants.ts:172-210`.
pub mod bits {
    pub const CREATE_INSTANT_INVITE: u64 = 1 << 0;
    pub const KICK_MEMBERS: u64 = 1 << 1;
    pub const BAN_MEMBERS: u64 = 1 << 2;
    pub const ADMINISTRATOR: u64 = 1 << 3;
    pub const MANAGE_CHANNELS: u64 = 1 << 4;
    pub const MANAGE_GUILD: u64 = 1 << 5;
    pub const ADD_REACTIONS: u64 = 1 << 6;
    pub const VIEW_AUDIT_LOG: u64 = 1 << 7;
    pub const PRIORITY_SPEAKER: u64 = 1 << 8;
    pub const STREAM: u64 = 1 << 9;
    pub const VIEW_CHANNEL: u64 = 1 << 10;
    pub const SEND_MESSAGES: u64 = 1 << 11;
    pub const SEND_TTS_MESSAGES: u64 = 1 << 12;
    pub const MANAGE_MESSAGES: u64 = 1 << 13;
    pub const EMBED_LINKS: u64 = 1 << 14;
    pub const ATTACH_FILES: u64 = 1 << 15;
    pub const READ_MESSAGE_HISTORY: u64 = 1 << 16;
    pub const MENTION_EVERYONE: u64 = 1 << 17;
    pub const USE_EXTERNAL_EMOJIS: u64 = 1 << 18;
    pub const CONNECT: u64 = 1 << 20;
    pub const SPEAK: u64 = 1 << 21;
    pub const MUTE_MEMBERS: u64 = 1 << 22;
    pub const DEAFEN_MEMBERS: u64 = 1 << 23;
    pub const MOVE_MEMBERS: u64 = 1 << 24;
    pub const USE_VAD: u64 = 1 << 25;
    pub const CHANGE_NICKNAME: u64 = 1 << 26;
    pub const MANAGE_NICKNAMES: u64 = 1 << 27;
    pub const MANAGE_ROLES: u64 = 1 << 28;
    pub const MANAGE_WEBHOOKS: u64 = 1 << 29;
    pub const MANAGE_EXPRESSIONS: u64 = 1 << 30;
    pub const USE_EXTERNAL_STICKERS: u64 = 1 << 37;
    pub const MODERATE_MEMBERS: u64 = 1 << 40;
    pub const CREATE_EXPRESSIONS: u64 = 1 << 43;
    pub const PIN_MESSAGES: u64 = 1 << 51;
    pub const BYPASS_SLOWMODE: u64 = 1 << 52;
    pub const UPDATE_RTC_REGION: u64 = 1 << 53;
    pub const VIEW_CHANNEL_MEMBERS: u64 = 1 << 54;

    /// OR of every defined bit (`ALL_PERMISSIONS`).
    pub const ALL: u64 = CREATE_INSTANT_INVITE
        | KICK_MEMBERS
        | BAN_MEMBERS
        | ADMINISTRATOR
        | MANAGE_CHANNELS
        | MANAGE_GUILD
        | ADD_REACTIONS
        | VIEW_AUDIT_LOG
        | PRIORITY_SPEAKER
        | STREAM
        | VIEW_CHANNEL
        | SEND_MESSAGES
        | SEND_TTS_MESSAGES
        | MANAGE_MESSAGES
        | EMBED_LINKS
        | ATTACH_FILES
        | READ_MESSAGE_HISTORY
        | MENTION_EVERYONE
        | USE_EXTERNAL_EMOJIS
        | CONNECT
        | SPEAK
        | MUTE_MEMBERS
        | DEAFEN_MEMBERS
        | MOVE_MEMBERS
        | USE_VAD
        | CHANGE_NICKNAME
        | MANAGE_NICKNAMES
        | MANAGE_ROLES
        | MANAGE_WEBHOOKS
        | MANAGE_EXPRESSIONS
        | USE_EXTERNAL_STICKERS
        | MODERATE_MEMBERS
        | CREATE_EXPRESSIONS
        | PIN_MESSAGES
        | BYPASS_SLOWMODE
        | UPDATE_RTC_REGION
        | VIEW_CHANNEL_MEMBERS;

    /// `DEFAULT_PERMISSIONS` (used when the @everyone role is missing).
    pub const DEFAULT: u64 = CREATE_INSTANT_INVITE
        | ADD_REACTIONS
        | STREAM
        | VIEW_CHANNEL
        | SEND_MESSAGES
        | EMBED_LINKS
        | ATTACH_FILES
        | READ_MESSAGE_HISTORY
        | USE_EXTERNAL_EMOJIS
        | CONNECT
        | SPEAK
        | USE_VAD
        | CHANGE_NICKNAME
        | USE_EXTERNAL_STICKERS
        | VIEW_CHANNEL_MEMBERS;

    /// `GUILD_VISITOR_PERMISSIONS_MASK` (`GuildVisitConstants.ts:6`).
    pub const VISITOR_MASK: u64 = VIEW_CHANNEL | READ_MESSAGE_HISTORY;

    /// Thread bits (`ThreadPermissionFlags`, `ThreadPermissionUtils.ts:6-11`).
    /// Not part of `ALL` (TS `ALL_PERMISSIONS` excludes them too).
    pub const MANAGE_THREADS: u64 = 1 << 34;
    pub const CREATE_PUBLIC_THREADS: u64 = 1 << 35;
    pub const CREATE_PRIVATE_THREADS: u64 = 1 << 36;
    pub const SEND_MESSAGES_IN_THREADS: u64 = 1 << 38;
    /// `THREAD_AWARE_ALL_PERMISSIONS`.
    pub const THREAD_AWARE_ALL: u64 =
        ALL | MANAGE_THREADS | CREATE_PUBLIC_THREADS | CREATE_PRIVATE_THREADS | SEND_MESSAGES_IN_THREADS;
}

/// Effective permissions of `user_id` in `guild`, optionally in `channel`.
///
/// Same order as TS: visitor mask, owner = ALL, @everyone (or DEFAULT) OR
/// member roles, ADMINISTRATOR = ALL, then channel overwrites
/// @everyone -> member roles (allow/deny accumulated) -> member.
/// A channel from another guild or a private channel yields 0, as TS returns
/// `NONE` when no guild resolves; DM send rules live in `ClientState`.
///
/// The visitor branch applies to whoever is asked about; TS applies it only
/// to the current user, which is the only caller (`ClientState`).
///
/// Not ported: MFA-elevated stripping (`calculateElevatedPermissions`).
/// Threads: see [`thread_view_permissions`].
pub fn compute_permissions(
    guild: &Guild,
    user_id: Snowflake,
    member: Option<&Member>,
    channel: Option<&Channel>,
) -> u64 {
    let overwrites = match channel {
        Some(c) if c.guild_id == Some(guild.id) => c.permission_overwrites.as_slice(),
        Some(_) => return 0,
        None => &[],
    };
    let find = |id: Snowflake| overwrites.iter().find(|o| o.id == id);
    let everyone_role = guild.roles.get(&guild.id).map(|r| r.permissions);

    if guild.visiting {
        let mut p = everyone_role.unwrap_or(bits::DEFAULT) & !bits::ADMINISTRATOR;
        if let Some(o) = find(guild.id) {
            p &= !o.deny;
            p |= o.allow;
        }
        return p & bits::VISITOR_MASK;
    }

    if guild.owner_id == Some(user_id) {
        return bits::ALL;
    }

    let mut p = everyone_role.unwrap_or(bits::DEFAULT);
    if let Some(m) = member {
        for role_id in &m.roles {
            if let Some(role) = guild.roles.get(role_id) {
                p |= role.permissions;
            }
        }
    }

    if p & bits::ADMINISTRATOR == bits::ADMINISTRATOR {
        return bits::ALL;
    }

    if let Some(o) = find(guild.id) {
        p &= !o.deny;
        p |= o.allow;
    }
    if let Some(m) = member {
        let mut allow = 0;
        let mut deny = 0;
        for role_id in &m.roles {
            if let Some(o) = find(*role_id) {
                allow |= o.allow;
                deny |= o.deny;
            }
        }
        p &= !deny;
        p |= allow;
        if let Some(o) = find(user_id) {
            p &= !o.deny;
            p |= o.allow;
        }
    }
    p
}

/// Thread permissions derived from the parent channel's
/// (`threadViewPermissions`, `ThreadPermissionUtils.ts:66-89`): ADMINISTRATOR
/// implies every thread bit, and SEND_MESSAGES is replaced by
/// SEND_MESSAGES_IN_THREADS.
pub fn thread_view_permissions(parent: u64) -> u64 {
    let perms = if has(parent, bits::ADMINISTRATOR) {
        parent | bits::THREAD_AWARE_ALL
    } else {
        parent
    };
    let without_send = perms & !bits::SEND_MESSAGES;
    if has(perms, bits::SEND_MESSAGES_IN_THREADS) {
        without_send | bits::SEND_MESSAGES
    } else {
        without_send
    }
}

/// True when every bit of `needed` is in `permissions` (TS `Permission.can`).
pub fn has(permissions: u64, needed: u64) -> bool {
    permissions & needed == needed
}

/// Highest non-@everyone role held by `member` in `guild`, ordered by
/// descending `position` and ascending `id` (`PermissionUtils.ts:211-226`).
pub fn highest_role<'a>(guild: &'a Guild, member: Option<&Member>) -> Option<&'a Role> {
    let member = member?;
    member
        .roles
        .iter()
        .filter(|id| **id != guild.id)
        .filter_map(|id| guild.roles.get(id))
        .min_by(|a, b| b.position.cmp(&a.position).then_with(|| a.id.cmp(&b.id)))
}

/// True when `actor_id` (owning the guild or holding `actor_top`) outranks
/// `target_top` (`PermissionUtils.ts:203-209`).
pub fn is_role_higher(guild: &Guild, actor_id: Snowflake, actor_top: Option<&Role>, target_top: Option<&Role>) -> bool {
    if guild.owner_id == Some(actor_id) {
        return true;
    }
    let Some(a) = actor_top else { return false };
    let Some(b) = target_top else { return true };
    a.position > b.position || (a.position == b.position && a.id < b.id)
}

/// True when `actor_id` outranks `target_id` as a guild member
/// (`PermissionUtils.ts:228-237`), blocking self, guild owner, visiting
/// sessions, and unknown targets.
pub fn can_manage_target_member(
    guild: &Guild,
    actor_id: Snowflake,
    actor_member: Option<&Member>,
    target_id: Snowflake,
    target_member: Option<&Member>,
) -> bool {
    if actor_id == target_id || guild.visiting || guild.owner_id == Some(target_id) || target_member.is_none() {
        return false;
    }
    if guild.owner_id == Some(actor_id) {
        return true;
    }
    is_role_higher(
        guild,
        actor_id,
        highest_role(guild, actor_member),
        highest_role(guild, target_member),
    )
}
