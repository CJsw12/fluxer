//! Keyboard navigation over channels and guilds, without GPUI
//! (`KeybindManager.ts:339-433`).

use fluxer_client_core::state::{ChannelKind, ClientState, Snowflake};

use crate::view_model::{Place, opens_message_pane};

/// The guild of a place (`None` = Home): the key for per-place memory and
/// the current-guild slot.
pub fn place_key(place: Place) -> Option<Snowflake> {
    match place {
        Place::Home => None,
        Place::Guild(id) => Some(id),
    }
}

/// `getNextChannel`: the item `delta` steps from `current`, wrapping. When
/// `current` is not in the list, forward starts before the first item and
/// backward at the first one (so it lands on the last). `None` for an empty
/// list.
pub fn cycle(items: &[Snowflake], current: Option<Snowflake>, delta: i32) -> Option<Snowflake> {
    let len = i64::try_from(items.len()).ok().filter(|l| *l > 0)?;
    let idx = current.and_then(|c| items.iter().position(|i| *i == c)).and_then(|i| i64::try_from(i).ok());
    let base = idx.unwrap_or(if delta > 0 { -1 } else { 0 });
    let next = (base + i64::from(delta)).rem_euclid(len);
    usize::try_from(next).ok().and_then(|i| items.get(i)).copied()
}

/// Slot cycling with an extra slot 0 before the items (`cycleGuildLikeSlot`
/// with Home, `cycleDirectMessageContext` with "no channel"). Returns the
/// item, or `None` for slot 0. A `current` not in the list counts as slot 0.
pub fn cycle_slots(items: &[Snowflake], current: Option<Snowflake>, delta: i32) -> Option<Snowflake> {
    let slots = i64::try_from(items.len()).ok()? + 1;
    let index = current
        .and_then(|c| items.iter().position(|i| *i == c))
        .and_then(|i| i64::try_from(i + 1).ok())
        .unwrap_or(0);
    let next = (index + i64::from(delta)).rem_euclid(slots);
    match usize::try_from(next).ok()? {
        0 => None,
        n => items.get(n - 1).copied(),
    }
}

/// `flattenGuildChannelsByDisplayOrder` minus categories and link channels
/// (`isNavigableChannel`). Channels the app cannot open (voice) are skipped
/// too, or Alt+Down would stop on them forever.
pub fn guild_channels(state: &ClientState, guild_id: Snowflake) -> Vec<Snowflake> {
    let mut out = Vec::new();
    for node in state.channel_tree(guild_id) {
        let group = if node.children.is_empty() { vec![node.channel] } else { node.children };
        out.extend(
            group
                .into_iter()
                .filter(|c| !c.kind.is_category() && c.kind != ChannelKind::GuildLink && opens_message_pane(c.kind))
                .map(|c| c.id),
        );
    }
    out
}

/// DM list order (`getSortedDmChannels`: newest activity first).
pub fn dm_channels(state: &ClientState) -> Vec<Snowflake> {
    state.dm_channels().into_iter().map(|c| c.id).collect()
}

/// Guild rail order.
pub fn guilds(state: &ClientState) -> Vec<Snowflake> {
    state.sorted_guilds().into_iter().map(|g| g.id).collect()
}

/// Which channels Alt+Shift / Ctrl+Alt+Shift arrows visit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelFilter {
    Unread,
    Mentioned,
}

/// `cycleFilteredChannelInCurrentGuild`.
pub fn filtered_guild_channels(state: &ClientState, guild_id: Snowflake, filter: ChannelFilter) -> Vec<Snowflake> {
    guild_channels(state, guild_id)
        .into_iter()
        .filter(|id| {
            let u = state.channel_unread(*id);
            match filter {
                ChannelFilter::Unread => u.unread,
                ChannelFilter::Mentioned => u.mention_count > 0,
            }
        })
        .collect()
}

/// `chat_mark_guild_read`: (channel, newest message) of every unread
/// channel in the guild.
pub fn guild_unread_acks(state: &ClientState, guild_id: Snowflake) -> Vec<(Snowflake, Snowflake)> {
    state
        .channels
        .values()
        .filter(|c| c.guild_id == Some(guild_id))
        .filter(|c| state.channel_unread(c.id).unread)
        .filter_map(|c| c.last_message_id.map(|m| (c.id, m)))
        .collect()
}

/// Recent visits, most recent first, without repeats; capped.
pub fn record_visit(visits: &mut Vec<Snowflake>, channel_id: Snowflake) {
    const MAX_VISITS: usize = 50;
    visits.retain(|c| *c != channel_id);
    visits.insert(0, channel_id);
    visits.truncate(MAX_VISITS);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[u64]) -> Vec<Snowflake> {
        v.iter().map(|n| Snowflake(*n)).collect()
    }

    #[test]
    fn cycle_wraps_and_starts_from_the_ends() {
        let list = ids(&[1, 2, 3]);
        assert_eq!(cycle(&list, Some(Snowflake(1)), 1), Some(Snowflake(2)));
        assert_eq!(cycle(&list, Some(Snowflake(3)), 1), Some(Snowflake(1)), "wraps forward");
        assert_eq!(cycle(&list, Some(Snowflake(1)), -1), Some(Snowflake(3)), "wraps backward");
        assert_eq!(cycle(&list, None, 1), Some(Snowflake(1)), "no current: first");
        assert_eq!(cycle(&list, Some(Snowflake(9)), -1), Some(Snowflake(3)), "unknown current: last");
        assert_eq!(cycle(&[], Some(Snowflake(1)), 1), None);
    }

    #[test]
    fn slots_have_a_home_slot() {
        let list = ids(&[5, 6]);
        assert_eq!(cycle_slots(&list, None, 1), Some(Snowflake(5)));
        assert_eq!(cycle_slots(&list, None, -1), Some(Snowflake(6)));
        assert_eq!(cycle_slots(&list, Some(Snowflake(6)), 1), None, "past the last: home");
        assert_eq!(cycle_slots(&list, Some(Snowflake(5)), -1), None);
        assert_eq!(cycle_slots(&list, Some(Snowflake(99)), 1), Some(Snowflake(5)), "unknown counts as home");
        assert_eq!(cycle_slots(&[], None, 1), None);
    }

    #[test]
    fn visits_move_to_front() {
        let mut v = Vec::new();
        for n in [1, 2, 1, 3] {
            record_visit(&mut v, Snowflake(n));
        }
        assert_eq!(v, ids(&[3, 1, 2]));
    }
}
