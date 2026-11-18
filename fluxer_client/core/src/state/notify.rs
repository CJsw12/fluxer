//! Desktop-notification decision (`Notification.ts` `handleMessageCreate`,
//! `validateNotificationData`, `shouldNotifyBasedOnSettings`) as pure functions.
//!
//! Not handled here (app side): streamer mode, "a modal is open" (TS plays
//! a sound only), the notified-message-id LRU, the mature-content gate and
//! `RuntimeConfig.directMessagesDisabled`. Threads use the same level rules
//! as channels plus the parent's mute (per-thread levels and thread
//! memberships are not ported).

use super::client::ClientState;
use super::content::message_flags;
use super::handlers::presence::Status;
use super::handlers::relationships::RelationshipKind;
use super::handlers::user_settings::message_notifications as level;
use super::ids::Snowflake;
use super::model::{ChannelKind, Message};

/// What the app should do for an incoming message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    /// The window is focused on that channel: play the in-channel sound only.
    SameChannelSound,
    /// Sound and toast for a guild channel message.
    Message,
    /// Sound (DM variant) and toast for a DM or group DM message.
    DirectMessage,
}

fn explicit_level(n: i8) -> bool {
    (level::ALL_MESSAGES..=level::NO_MESSAGES).contains(&n)
}

/// `resolveEffectiveMessageNotifications`: the channel override, then the
/// parent category override (guild channels), then the guild setting. Values
/// outside ALL/ONLY_MENTIONS/NO_MESSAGES (INHERIT, unset) fall through to
/// the guild's `effectiveMessageNotifications` (its default, ONLY_MENTIONS
/// for large guilds); DMs fall through to ALL_MESSAGES.
pub fn effective_notification_level(state: &ClientState, guild_id: Option<Snowflake>, channel_id: Snowflake) -> i8 {
    let settings = state.guild_settings.get(&guild_id);
    let explicit = |id: Snowflake| {
        settings
            .and_then(|s| s.channel_overrides.get(&id))
            .and_then(|o| o.message_notifications)
            .filter(|n| explicit_level(*n))
    };
    if let Some(n) = explicit(channel_id) {
        return n;
    }
    if guild_id.is_some()
        && let Some(parent) = state.channels.get(&channel_id).and_then(|c| c.parent_id)
        && let Some(n) = explicit(parent)
    {
        return n;
    }
    match settings.map(|s| s.message_notifications) {
        Some(n) if explicit_level(n) => n,
        _ => guild_id
            .and_then(|id| state.guilds.get(&id))
            .map(|g| g.effective_message_notifications())
            .filter(|n| explicit_level(*n))
            .unwrap_or(level::ALL_MESSAGES),
    }
}

/// `MessageUtils.isMentioned`, honouring the suppress flags of the guild.
fn is_mentioned(
    state: &ClientState,
    me: Snowflake,
    message: &Message,
    guild_id: Option<Snowflake>,
    kind: ChannelKind,
) -> bool {
    let settings = state.guild_settings.get(&guild_id);
    let suppress_everyone = settings.is_some_and(|s| s.suppress_everyone);
    if message.mention_everyone && kind != ChannelKind::Dm && !suppress_everyone {
        return true;
    }
    if message.mentions.iter().any(|u| u.id == me) {
        return true;
    }
    let Some(guild) = guild_id else { return false };
    let suppress_roles = settings.is_some_and(|s| s.suppress_roles);
    let Some(member) = state.members.get(&guild).and_then(|m| m.get(&me)) else { return false };
    !suppress_roles && message.mention_roles.iter().any(|r| member.roles.contains(r))
}

/// Decide whether `message` raises a notification. `viewing_channel` is
/// the channel on screen; `app_focused` the window focus. Duplicate
/// suppression (notified-id LRU) is the caller's job.
pub fn should_notify(
    state: &ClientState,
    message: &Message,
    now_ms: i64,
    app_focused: bool,
    viewing_channel: Option<Snowflake>,
) -> Option<NotificationKind> {
    let me = state.current_user_id()?;
    let channel = state.channels.get(&message.channel_id)?;
    let guild_id = channel.guild_id;
    let own = message.author.id == me;
    let blocked = state
        .relationships
        .get(&message.author.id)
        .is_some_and(|r| r.kind() == RelationshipKind::Blocked);

    if own || blocked {
        return None;
    }
    if app_focused && viewing_channel == Some(message.channel_id) {
        return Some(NotificationKind::SameChannelSound);
    }
    if Status::parse(state.user_settings.status.as_deref()) == Status::Dnd {
        return None;
    }
    if message.flags & message_flags::SUPPRESS_NOTIFICATIONS != 0 {
        return None;
    }
    // `resolvesToNoMessages`: guild or channel muted, or level NO_MESSAGES.
    // A thread is also muted through its parent (`isThreadParentMuted`;
    // thread memberships are not ported).
    let parent_muted = channel.kind.is_thread()
        && channel.parent_id.is_some_and(|parent| state.is_channel_muted(guild_id, parent, now_ms));
    let muted = state.is_guild_muted(guild_id, now_ms)
        || state.is_channel_muted(guild_id, channel.id, now_ms)
        || parent_muted;
    let lvl = effective_notification_level(state, guild_id, channel.id);
    if muted || lvl == level::NO_MESSAGES {
        return None;
    }
    // ONLY_MENTIONS: mentioned, or a DM (which is not muted, checked above).
    let private = channel.kind.is_private();
    if lvl != level::ALL_MESSAGES && !private && !is_mentioned(state, me, message, guild_id, channel.kind) {
        return None;
    }
    Some(if private { NotificationKind::DirectMessage } else { NotificationKind::Message })
}

impl ClientState {
    /// [`should_notify`] for a message already in the channel window
    /// (`None` when it is not loaded).
    pub fn should_notify_for(
        &self,
        channel_id: Snowflake,
        message_id: Snowflake,
        now_ms: i64,
        app_focused: bool,
        viewing_channel: Option<Snowflake>,
    ) -> Option<NotificationKind> {
        let message = self.messages.message(channel_id, message_id)?;
        should_notify(self, message, now_ms, app_focused, viewing_channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::fixture::*;
    use serde_json::{Value, json};

    fn msg_from(author: u64, channel: u64, mut extra: Value) -> Message {
        extra["id"] = json!(mid(50).to_string());
        extra["channel_id"] = json!(channel.to_string());
        extra["author"] = user_json(author, "u");
        extra["content"] = json!("hi");
        extra["timestamp"] = json!("2025-06-15T00:00:00+00:00");
        serde_json::from_value(extra).unwrap()
    }

    fn update_settings(state: &mut ClientState, guild: Option<u64>, mut d: Value) {
        d["guild_id"] = guild.map_or(json!(null), |g| json!(g.to_string()));
        state.apply(&dispatch("USER_GUILD_SETTINGS_UPDATE", d));
    }

    const NOW: i64 = 1_750_000_100_000;

    #[test]
    fn level_resolution_order() {
        let mut s = fixture_state();
        let g = Some(sf(GUILD));
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ALL_MESSAGES);
        update_settings(&mut s, Some(GUILD), json!({"message_notifications": 1}));
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ONLY_MENTIONS);
        update_settings(
            &mut s,
            Some(GUILD),
            json!({"message_notifications": 1, "channel_overrides": [
                {"channel_id": CATEGORY.to_string(), "message_notifications": 2},
                {"channel_id": GENERAL.to_string(), "message_notifications": 0}]}),
        );
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ALL_MESSAGES, "channel wins");
        assert_eq!(effective_notification_level(&s, g, sf(STAFF)), level::NO_MESSAGES, "category beats guild");
        update_settings(
            &mut s,
            Some(GUILD),
            json!({"message_notifications": 3, "channel_overrides": [{"channel_id": GENERAL.to_string(), "message_notifications": 3}]}),
        );
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ALL_MESSAGES, "inherit");
    }

    /// Without a user setting the guild's default applies; large guilds
    /// default to mentions only (`effectiveMessageNotifications`).
    #[test]
    fn guild_default_notifications() {
        let mut s = fixture_state();
        let g = Some(sf(GUILD));
        let plain = msg_from(OTHER, GENERAL, json!({}));
        let update = |s: &mut ClientState, extra: Value| {
            let mut d = json!({"id": GUILD.to_string(), "name": "Alpha", "owner_id": OWNER.to_string()});
            for (k, v) in extra.as_object().into_iter().flatten() {
                d[k] = v.clone();
            }
            s.apply(&dispatch("GUILD_UPDATE", d));
        };
        update(&mut s, json!({"default_message_notifications": 1, "member_count": 10}));
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ONLY_MENTIONS);
        assert_eq!(should_notify(&s, &plain, NOW, false, None), None, "mentions only by default");
        update_settings(&mut s, Some(GUILD), json!({"message_notifications": 0}));
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ALL_MESSAGES, "user setting wins");
        update_settings(&mut s, Some(GUILD), json!({"message_notifications": 3}));
        update(&mut s, json!({"default_message_notifications": 0, "member_count": 251}));
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ONLY_MENTIONS, "large guild");
        update(&mut s, json!({"default_message_notifications": 0, "member_count": 5, "features": ["VERY_LARGE_GUILD"]}));
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ONLY_MENTIONS, "large by feature");
        update(&mut s, json!({"default_message_notifications": 0, "member_count": 5, "features": []}));
        assert_eq!(effective_notification_level(&s, g, sf(GENERAL)), level::ALL_MESSAGES);
    }

    /// A muted parent channel mutes its threads.
    #[test]
    fn thread_parent_mute() {
        let mut s = fixture_state();
        s.apply(&dispatch(
            "CHANNEL_CREATE",
            json!({"id": "4242", "type": 11, "name": "t", "guild_id": GUILD.to_string(), "parent_id": GENERAL.to_string()}),
        ));
        let m = msg_from(OTHER, 4242, json!({}));
        assert!(should_notify(&s, &m, NOW, false, None).is_some());
        update_settings(&mut s, Some(GUILD), json!({"channel_overrides": [{"channel_id": GENERAL.to_string(), "muted": true}]}));
        assert_eq!(should_notify(&s, &m, NOW, false, None), None);
    }

    #[test]
    fn basic_decisions() {
        let mut s = fixture_state();
        let plain = msg_from(OTHER, GENERAL, json!({}));
        assert_eq!(should_notify(&s, &plain, NOW, false, None), Some(NotificationKind::Message));
        assert_eq!(should_notify(&s, &plain, NOW, true, Some(sf(GENERAL))), Some(NotificationKind::SameChannelSound));
        assert_eq!(should_notify(&s, &plain, NOW, false, Some(sf(GENERAL))), Some(NotificationKind::Message));
        let own = msg_from(ME, GENERAL, json!({}));
        assert_eq!(should_notify(&s, &own, NOW, false, None), None);
        assert_eq!(should_notify(&s, &own, NOW, true, Some(sf(GENERAL))), None);
        let dm = msg_from(OTHER, DM, json!({}));
        assert_eq!(should_notify(&s, &dm, NOW, false, None), Some(NotificationKind::DirectMessage));
        assert_eq!(should_notify(&s, &msg_from(OTHER, 9999, json!({})), NOW, false, None), None);
        let quiet = msg_from(OTHER, GENERAL, json!({"flags": message_flags::SUPPRESS_NOTIFICATIONS}));
        assert_eq!(should_notify(&s, &quiet, NOW, false, None), None);
        s.apply(&dispatch("USER_SETTINGS_UPDATE", json!({"status": "dnd"})));
        assert_eq!(should_notify(&s, &plain, NOW, false, None), None);
    }

    #[test]
    fn blocked_author() {
        let mut s = fixture_state();
        s.apply(&dispatch(
            "RELATIONSHIP_ADD",
            json!({"id": OTHER.to_string(), "type": 2, "user": user_json(OTHER, "other")}),
        ));
        let m = msg_from(OTHER, GENERAL, json!({}));
        assert_eq!(should_notify(&s, &m, NOW, true, Some(sf(GENERAL))), None);
        assert_eq!(should_notify(&s, &m, NOW, false, None), None);
    }

    #[test]
    fn mentions_and_suppress() {
        let mut s = fixture_state();
        update_settings(&mut s, Some(GUILD), json!({"message_notifications": 1}));
        let plain = msg_from(OTHER, GENERAL, json!({}));
        assert_eq!(should_notify(&s, &plain, NOW, false, None), None);
        let direct = msg_from(OTHER, GENERAL, json!({"mentions": [user_json(ME, "me")]}));
        assert!(should_notify(&s, &direct, NOW, false, None).is_some());
        let everyone = msg_from(OTHER, GENERAL, json!({"mention_everyone": true}));
        assert!(should_notify(&s, &everyone, NOW, false, None).is_some());
        let role = msg_from(OTHER, GENERAL, json!({"mention_roles": [ROLE_HELPER.to_string()]}));
        assert!(should_notify(&s, &role, NOW, false, None).is_some());
        update_settings(&mut s, Some(GUILD), json!({"suppress_everyone": true, "suppress_roles": true}));
        assert_eq!(should_notify(&s, &everyone, NOW, false, None), None);
        assert_eq!(should_notify(&s, &role, NOW, false, None), None);
        assert!(should_notify(&s, &direct, NOW, false, None).is_some(), "user mention is never suppressed");
    }

    #[test]
    fn mutes_with_end_time() {
        let mut s = fixture_state();
        let m = msg_from(OTHER, GENERAL, json!({}));
        update_settings(&mut s, Some(GUILD), json!({"muted": true, "mute_config": {"end_time": "2030-01-01T00:00:00+00:00"}}));
        assert_eq!(should_notify(&s, &m, NOW, false, None), None, "active guild mute");
        update_settings(&mut s, Some(GUILD), json!({"muted": true, "mute_config": {"end_time": "2020-01-01T00:00:00+00:00"}}));
        assert!(should_notify(&s, &m, NOW, false, None).is_some(), "expired guild mute");
        update_settings(
            &mut s,
            Some(GUILD),
            json!({"muted": false, "channel_overrides": [{"channel_id": GENERAL.to_string(), "muted": true}]}),
        );
        assert_eq!(should_notify(&s, &m, NOW, false, None), None, "channel mute");
    }

    #[test]
    fn dm_mute_and_window_lookup() {
        let mut s = fixture_state();
        update_settings(&mut s, None, json!({"channel_overrides": [{"channel_id": DM.to_string(), "muted": true}]}));
        let dm = msg_from(OTHER, DM, json!({}));
        assert_eq!(should_notify(&s, &dm, NOW, false, None), None);

        let mut s = fixture_state();
        let m = msg_from(OTHER, GENERAL, json!({}));
        s.messages.load(sf(GENERAL), vec![m.clone()], crate::state::LoadMode::Replace);
        assert_eq!(s.should_notify_for(sf(GENERAL), m.id, NOW, false, None), Some(NotificationKind::Message));
        assert_eq!(s.should_notify_for(sf(GENERAL), mid(999), NOW, false, None), None);
    }
}
