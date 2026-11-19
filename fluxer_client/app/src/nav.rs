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

/// One visited destination in the back/forward stack (`Place` + channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryEntry {
    pub place: Place,
    pub channel_id: Snowflake,
}

/// Browser-like back/forward navigation stack for visited channels.
#[derive(Debug, Clone, Default)]
pub struct NavHistory {
    entries: Vec<HistoryEntry>,
    cursor: Option<usize>,
}

impl NavHistory {
    const MAX_HISTORY: usize = 100;

    pub fn current(&self) -> Option<HistoryEntry> {
        self.cursor.and_then(|ix| self.entries.get(ix).copied())
    }

    /// Pushes `entry`, truncating any forward entries after the cursor.
    pub fn push(&mut self, entry: HistoryEntry) {
        if self.current() == Some(entry) {
            return;
        }
        if let Some(ix) = self.cursor {
            self.entries.truncate(ix + 1);
        }
        self.entries.push(entry);
        if self.entries.len() > Self::MAX_HISTORY {
            self.entries.remove(0);
        }
        self.cursor = Some(self.entries.len() - 1);
    }

    /// Moves backward to the first entry satisfying `pred`. Keeps the cursor
    /// unchanged if no earlier entry matches.
    pub fn back_matching(&mut self, mut pred: impl FnMut(HistoryEntry) -> bool) -> Option<HistoryEntry> {
        let cur = self.cursor?;
        for ix in (0..cur).rev() {
            let entry = self.entries[ix];
            if pred(entry) {
                self.cursor = Some(ix);
                return Some(entry);
            }
        }
        None
    }

    /// Moves forward to the first entry satisfying `pred`. Keeps the cursor
    /// unchanged if no later entry matches.
    pub fn forward_matching(&mut self, mut pred: impl FnMut(HistoryEntry) -> bool) -> Option<HistoryEntry> {
        let cur = self.cursor?;
        for ix in (cur + 1)..self.entries.len() {
            let entry = self.entries[ix];
            if pred(entry) {
                self.cursor = Some(ix);
                return Some(entry);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[u64]) -> Vec<Snowflake> {
        v.iter().map(|n| Snowflake(*n)).collect()
    }

    #[test]
    fn history_push_back_forward_truncates_and_skips_invalid() {
        let mut h = NavHistory::default();
        let e1 = HistoryEntry { place: Place::Home, channel_id: Snowflake(1) };
        let e2 = HistoryEntry { place: Place::Guild(Snowflake(10)), channel_id: Snowflake(2) };
        let e3 = HistoryEntry { place: Place::Guild(Snowflake(10)), channel_id: Snowflake(3) };
        let e4 = HistoryEntry { place: Place::Home, channel_id: Snowflake(4) };

        assert_eq!(h.current(), None);
        h.push(e1);
        h.push(e1);
        h.push(e2);
        h.push(e3);
        assert_eq!(h.current(), Some(e3));

        // Skips deleted channel e2 when stepping back.
        assert_eq!(h.back_matching(|e| e.channel_id != Snowflake(2)), Some(e1));
        assert_eq!(h.forward_matching(|e| e.channel_id != Snowflake(2)), Some(e3));

        // Going back to e2 then pushing e4 drops e3 from forward history.
        assert_eq!(h.back_matching(|_| true), Some(e2));
        h.push(e4);
        assert_eq!(h.current(), Some(e4));
        assert_eq!(h.forward_matching(|_| true), None);
        assert_eq!(h.back_matching(|_| true), Some(e2));
    }

    #[test]
    fn history_caps_at_max_entries() {
        let mut h = NavHistory::default();
        for n in 1..=120u64 {
            h.push(HistoryEntry { place: Place::Home, channel_id: Snowflake(n) });
        }
        assert_eq!(h.current().map(|e| e.channel_id), Some(Snowflake(120)));
        let mut count = 0;
        while h.back_matching(|_| true).is_some() {
            count += 1;
        }
        assert_eq!(count, NavHistory::MAX_HISTORY - 1);
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
