//! Channel visibility for an arbitrary user (not just the current one).

use super::client::ClientState;
use super::ids::Snowflake;
use super::model::ChannelKind;
use super::permissions::{bits, compute_permissions, has};

/// Permissions of `user_id` in a guild channel; 0 for unknown channels,
/// unknown guilds, DMs, and users who are neither a member nor the owner.
/// Threads derive from their parent channel like
/// [`ClientState::channel_permissions`].
pub fn user_channel_permissions(state: &ClientState, user_id: Snowflake, channel_id: Snowflake) -> u64 {
    let Some(channel) = state.channels.get(&channel_id) else { return 0 };
    if channel.kind.is_thread() {
        return match channel.parent_id {
            Some(parent) if state.channels.get(&parent).is_some_and(|p| !p.kind.is_thread()) => {
                super::permissions::thread_view_permissions(user_channel_permissions(state, user_id, parent))
            }
            _ => 0,
        };
    }
    let Some(guild) = channel.guild_id.and_then(|g| state.guilds.get(&g)) else { return 0 };
    let member = state.members.get(&guild.id).and_then(|m| m.get(&user_id));
    if member.is_none() && guild.owner_id != Some(user_id) {
        return 0;
    }
    compute_permissions(guild, user_id, member, Some(channel))
}

/// Whether `user_id` can see `channel_id`. Private channels: the user is a
/// recipient (or the current user, who is not listed in `recipients`).
/// Guild channels: VIEW_CHANNEL from the member's computed permissions;
/// a user whose member is not loaded is treated as not viewing.
pub fn user_can_view_channel(state: &ClientState, user_id: Snowflake, channel_id: Snowflake) -> bool {
    let Some(channel) = state.channels.get(&channel_id) else { return false };
    if channel.kind.is_private() {
        if channel.kind == ChannelKind::DmPersonalNotes {
            return state.current_user_id() == Some(user_id);
        }
        return state.current_user_id() == Some(user_id) || channel.recipients.iter().any(|u| u.id == user_id);
    }
    has(user_channel_permissions(state, user_id, channel_id), bits::VIEW_CHANNEL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::fixture::*;

    #[test]
    fn guild_channel_views() {
        let s = fixture_state();
        // OTHER has the mod role (views staff), CAROL does not, OWNER owns the guild.
        assert!(user_can_view_channel(&s, sf(OTHER), sf(GENERAL)));
        assert!(user_can_view_channel(&s, sf(OTHER), sf(STAFF)));
        assert!(!user_can_view_channel(&s, sf(CAROL), sf(STAFF)));
        assert!(user_can_view_channel(&s, sf(CAROL), sf(GENERAL)));
        assert!(user_can_view_channel(&s, sf(OWNER), sf(STAFF)), "owner needs no member entry");
        assert!(!user_can_view_channel(&s, sf(77), sf(GENERAL)), "not a member");
        assert!(!user_can_view_channel(&s, sf(OTHER), sf(9999)), "unknown channel");
    }

    #[test]
    fn private_channels() {
        let s = fixture_state();
        assert!(user_can_view_channel(&s, sf(OTHER), sf(DM)));
        assert!(user_can_view_channel(&s, sf(ME), sf(DM)));
        assert!(!user_can_view_channel(&s, sf(CAROL), sf(DM)));
        assert!(user_can_view_channel(&s, sf(CAROL), sf(GROUP_DM)));
    }
}
