//! Per-channel message windows (`messaging/state/ChannelMessages.ts`,
//! `ChannelMessagesLoadStateMachine.ts`, `ChannelIncomingMessageStateMachine.ts`).
//! Jump/around loads and the before/after side buffers are not ported:
//! trimmed messages are dropped and must be re-fetched.

use std::collections::HashMap;

use serde::Serialize;

use super::ids::Snowflake;
use super::model::{Message, MessageState, User, overlay};

/// `MAX_MESSAGES_PER_CHANNEL`: REST page size.
pub const PAGE_SIZE: usize = 50;
/// `MAX_LOADED_MESSAGES`: window size that triggers a trim.
pub const MAX_LOADED_MESSAGES: usize = 200;
/// `TRUNCATED_MESSAGE_VIEW_SIZE`: window size after a trim (desktop value).
pub const TRUNCATED_MESSAGE_VIEW_SIZE: usize = 100;
/// Channel windows kept in memory (`ChannelMessages.ts:256-258`).
pub const MAX_CACHED_CHANNELS: usize = 50;

/// How a REST page is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadMode {
    /// Latest page (`GET .../messages?limit=50`): replace the window.
    Replace,
    /// Older page (`?before=<oldest id>`): prepend.
    Before,
    /// Newer page (`?after=<newest id>`): append.
    After,
    /// Historical slice centered on an anchor (`?around=<id>`): replace
    /// the window with pagination possible on both sides.
    Around,
}

/// One channel's loaded messages, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MessageWindow {
    pub messages: Vec<Message>,
    pub has_more_before: bool,
    pub has_more_after: bool,
    /// A page has been applied since the window was created or invalidated.
    /// Incoming gateway messages are ignored until then.
    pub ready: bool,
    /// The view is scrolled to the newest message (TS
    /// `Dimension.channelPinnedToEnd`). Incoming messages trim the oldest
    /// side only while pinned; otherwise the window grows up to
    /// `MAX_LOADED_MESSAGES`. Defaults to true.
    pub pinned_to_end: bool,
    #[serde(skip)]
    history_discarded: bool,
}

impl MessageWindow {
    fn new() -> Self {
        Self {
            has_more_before: true,
            pinned_to_end: true,
            ..Self::default()
        }
    }

    fn position(&self, id: Snowflake) -> Option<usize> {
        self.messages.iter().position(|m| m.id == id)
    }

    /// Insert or replace by id, keeping id order.
    fn merge(&mut self, records: Vec<Message>) {
        for record in records {
            match self.position(record.id) {
                Some(i) => self.messages[i] = record,
                None => self.messages.push(record),
            }
        }
        self.messages.sort_by_key(|m| m.id);
    }

    /// TS `trimOldest`: keep the newest `max` messages.
    fn trim_oldest(&mut self, max: usize) {
        if self.messages.len() > max {
            let overflow = self.messages.len() - max;
            self.messages.drain(..overflow);
            self.has_more_before = true;
        }
    }

    /// TS `trimNewest`: keep the oldest `max` messages.
    fn trim_newest(&mut self, max: usize) {
        if self.messages.len() > max {
            self.messages.truncate(max);
            self.has_more_after = true;
        }
    }

    /// Unsent messages to keep across a replace (`collectUnsentMessages`):
    /// Prefer a matching nonce. After dropping history, content matching
    /// also requires a server ID newer than the pending message's nonce.
    fn take_unsent(&mut self, records: &[Message]) -> Vec<Message> {
        let mut unmatched: Vec<&Message> = records
            .iter()
            .filter(|r| self.position(r.id).is_none())
            .collect();
        let reply_to = |m: &Message| m.message_reference.as_ref().and_then(|r| r.message_id);
        let history_discarded = self.history_discarded;
        let mut kept = Vec::new();
        for m in self.messages.drain(..) {
            if m.state == MessageState::Sent {
                continue;
            }
            let hit = unmatched.iter().position(|r| r.author.id == m.author.id && r.nonce_id() == Some(m.id))
                .or_else(|| {
                    unmatched.iter().position(|r| {
                        r.nonce_id().is_none()
                            && (!history_discarded || r.id > m.id)
                            && r.author.id == m.author.id
                            && reply_to(r) == reply_to(&m)
                            && r.content == m.content
                    })
                });
            match hit {
                Some(i) => {
                    unmatched.swap_remove(i);
                }
                None => kept.push(m),
            }
        }
        kept
    }

    fn discard_remote_history(&mut self) {
        self.messages.retain(|message| message.state != MessageState::Sent);
        let messages = std::mem::take(&mut self.messages);
        *self = Self { messages, history_discarded: true, ..Self::new() };
    }
}

/// All channel windows plus LRU bookkeeping.
#[derive(Debug, Clone, Default)]
pub struct MessageStore {
    windows: HashMap<Snowflake, MessageWindow>,
    /// Least recently used first.
    lru: Vec<Snowflake>,
    selected: Option<Snowflake>,
}

impl MessageStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn window(&self, channel_id: Snowflake) -> Option<&MessageWindow> {
        self.windows.get(&channel_id)
    }

    /// Mark the channel the user is viewing; it is never evicted.
    pub fn select(&mut self, channel_id: Option<Snowflake>) {
        self.selected = channel_id;
        if let Some(id) = channel_id
            && self.windows.contains_key(&id)
        {
            self.touch(id);
        }
    }

    /// Apply a REST page fetched with `limit = PAGE_SIZE`. `newest_first` is
    /// the response order of `GET /channels/{id}/messages`.
    ///
    /// `hasMore*` follows `MessagePageStateMachine.getBasePageState`; the
    /// merge follows `ChannelMessagesLoadStateMachine`: a window that is not
    /// ready is replaced whatever `mode` says, but unsent messages survive
    /// only a real latest-page replace (`isWindowReplacement`). Before/after
    /// merges keep the other side's flag and trim the far side to
    /// `TRUNCATED_MESSAGE_VIEW_SIZE` once over `MAX_LOADED_MESSAGES`.
    pub fn load(&mut self, channel_id: Snowflake, newest_first: Vec<Message>, mode: LoadMode) {
        let full_page = newest_first.len() == PAGE_SIZE;
        let page_more_before = full_page && mode != LoadMode::After;
        let page_more_after = full_page && mode == LoadMode::After;
        let mut records = newest_first;
        records.reverse();
        let window = self.entry(channel_id);
        let effective = if window.ready || mode == LoadMode::Around { mode } else { LoadMode::Replace };
        match effective {
            LoadMode::Replace => {
                let unsent = if mode == LoadMode::Replace {
                    window.take_unsent(&records)
                } else {
                    Vec::new()
                };
                window.messages.clear();
                window.merge(records);
                window.merge(unsent);
                window.has_more_before = page_more_before;
                window.has_more_after = page_more_after;
                if mode == LoadMode::Replace {
                    window.pinned_to_end = true;
                }
            }
            LoadMode::Before => {
                window.merge(records);
                if window.messages.len() > MAX_LOADED_MESSAGES {
                    window.trim_newest(TRUNCATED_MESSAGE_VIEW_SIZE);
                }
                window.has_more_before = page_more_before;
            }
            LoadMode::After => {
                window.merge(records);
                if window.messages.len() > MAX_LOADED_MESSAGES {
                    window.trim_oldest(TRUNCATED_MESSAGE_VIEW_SIZE);
                }
                window.has_more_after = page_more_after;
            }
            LoadMode::Around => {
                let unsent = if !full_page {
                    window.take_unsent(&records)
                } else {
                    Vec::new()
                };
                window.messages.clear();
                window.merge(records);
                window.merge(unsent);
                window.has_more_before = full_page;
                window.has_more_after = full_page;
                window.pinned_to_end = !full_page;
            }
        }
        window.ready = true;
    }

    /// Replaces the channel window with an `?around=<target>` slice using
    /// `MessagePaginationUtils.ts` / `MessagePageStateMachine.ts` boundary rules.
    pub fn load_around(
        &mut self,
        channel_id: Snowflake,
        newest_first: Vec<Message>,
        target: Snowflake,
        known_latest: Option<Snowflake>,
    ) {
        let expected_newer = PAGE_SIZE / 2;
        let expected_older = PAGE_SIZE.saturating_sub(1 + expected_newer);
        let is_at_known_latest =
            known_latest.is_some() && newest_first.first().map(|m| m.id) == known_latest;
        // A read cursor can be synthetic or refer to a deleted message.
        // The server still fills separate slices on either side of it.
        let messages_newer = newest_first.iter().filter(|m| m.id > target).count();
        let messages_older = newest_first.iter().filter(|m| m.id < target).count();
        let has_more_before = expected_older > 0 && messages_older >= expected_older;
        let has_more_after = expected_newer > 0 && messages_newer >= expected_newer && !is_at_known_latest;
        self.apply_around_slice(channel_id, newest_first, has_more_before, has_more_after);
    }

    fn apply_around_slice(
        &mut self,
        channel_id: Snowflake,
        mut newest_first: Vec<Message>,
        has_more_before: bool,
        has_more_after: bool,
    ) {
        newest_first.reverse();
        let window = self.entry(channel_id);
        let unsent = if !has_more_after {
            window.take_unsent(&newest_first)
        } else {
            Vec::new()
        };
        window.messages.clear();
        window.merge(newest_first);
        window.merge(unsent);
        window.has_more_before = has_more_before;
        window.has_more_after = has_more_after;
        window.pinned_to_end = !has_more_after;
        window.ready = true;
    }

    /// Record whether the channel's view is scrolled to the newest message.
    pub fn set_pinned_to_end(&mut self, channel_id: Snowflake, pinned: bool) {
        if let Some(window) = self.windows.get_mut(&channel_id) {
            window.pinned_to_end = pinned;
        }
    }

    /// Optimistic send (`createOptimistic`): a `Sending` message whose id is
    /// the nonce. Create the nonce with `Snowflake::from_unix_ms(now_ms)` and
    /// send it as `nonce` in `POST /channels/{id}/messages`.
    pub fn insert_pending(
        &mut self,
        channel_id: Snowflake,
        nonce: Snowflake,
        content: String,
        author: User,
    ) {
        let message = Message {
            id: nonce,
            channel_id,
            guild_id: None,
            author,
            content,
            timestamp: String::new(),
            edited_timestamp: None,
            kind: 0,
            flags: 0,
            pinned: false,
            mention_everyone: false,
            mentions: Vec::new(),
            mention_roles: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            stickers: Vec::new(),
            reactions: Vec::new(),
            message_reference: None,
            referenced_message: None,
            webhook_id: None,
            nonce: Some(nonce.to_string()),
            state: MessageState::Sending,
        };
        self.entry(channel_id).merge(vec![message]);
    }

    /// A server message from `MESSAGE_CREATE` or the `POST` response.
    /// Returns true if the window changed.
    ///
    /// Order (`ChannelIncomingMessageStateMachine`): a pending message with
    /// id == nonce and the same author is replaced; otherwise ignored if the
    /// window is not ready or newer pages exist (`has_more_after`);
    /// otherwise appended (`appendIncomingMessage`). While pinned to the
    /// end the window is trimmed to `TRUNCATED_MESSAGE_VIEW_SIZE` from the
    /// top; when scrolled up and already over `MAX_LOADED_MESSAGES` it is
    /// trimmed from the bottom instead.
    ///
    /// Deviation: TS ignores a nonce match when the window is not ready
    /// (`MessagingMessages.handleIncomingMessage`); here the pending message
    /// is still confirmed so it does not linger as `Sending`.
    pub fn apply_incoming(&mut self, mut message: Message) -> bool {
        message.state = MessageState::Sent;
        let channel_id = message.channel_id;
        let Some(window) = self.windows.get_mut(&channel_id) else {
            return false;
        };
        if let Some(nonce) = message.nonce_id()
            && let Some(i) = window.position(nonce)
            && window.messages[i].author.id == message.author.id
        {
            // Re-sort: the server id may differ from the nonce's position.
            window.messages.remove(i);
            window.merge(vec![message]);
            return true;
        }
        if !window.ready || window.has_more_after {
            return false;
        }
        let previous_len = window.messages.len();
        window.merge(vec![message]);
        if window.pinned_to_end {
            window.trim_oldest(TRUNCATED_MESSAGE_VIEW_SIZE);
        } else if previous_len > MAX_LOADED_MESSAGES {
            window.trim_newest(TRUNCATED_MESSAGE_VIEW_SIZE);
        }
        true
    }

    /// Send failed: `Sending` -> `Failed`. Returns the channel to redraw.
    pub fn mark_failed(&mut self, nonce: Snowflake) -> Option<Snowflake> {
        self.set_pending_state(nonce, MessageState::Sending, MessageState::Failed)
    }

    pub fn has_confirmed_nonce(&self, channel: Snowflake, author: Snowflake, nonce: Snowflake) -> bool {
        self.window(channel).is_some_and(|window| window.messages.iter().any(|message| {
            message.state == MessageState::Sent && message.author.id == author && message.nonce_id() == Some(nonce)
        }))
    }

    /// Retry: `Failed` -> `Sending`. Returns the channel to redraw.
    pub fn mark_sending(&mut self, nonce: Snowflake) -> Option<Snowflake> {
        self.set_pending_state(nonce, MessageState::Failed, MessageState::Sending)
    }

    /// Drop a local pending/failed message (user discarded it).
    pub fn remove_pending(&mut self, channel_id: Snowflake, nonce: Snowflake) -> bool {
        let Some(window) = self.windows.get_mut(&channel_id) else {
            return false;
        };
        match window.position(nonce) {
            Some(i) if window.messages[i].state != MessageState::Sent => {
                window.messages.remove(i);
                true
            }
            _ => false,
        }
    }

    /// `MESSAGE_UPDATE`: shallow merge of a possibly partial payload.
    pub(crate) fn apply_update(&mut self, channel_id: Snowflake, id: Snowflake, patch: &serde_json::Value) -> bool {
        let Some(window) = self.windows.get_mut(&channel_id) else {
            return false;
        };
        let Some(i) = window.position(id) else {
            return false;
        };
        match overlay(&window.messages[i], patch) {
            Ok(mut merged) => {
                merged.state = MessageState::Sent;
                window.messages[i] = merged;
                true
            }
            Err(err) => {
                tracing::debug!(%id, %err, "MESSAGE_UPDATE merge failed");
                false
            }
        }
    }

    /// Removes `ids` from the channel's window. Replies left in the window
    /// whose target was removed get `referenced_message: null` (target gone).
    pub(crate) fn remove(&mut self, channel_id: Snowflake, ids: &[Snowflake]) -> bool {
        let Some(window) = self.windows.get_mut(&channel_id) else {
            return false;
        };
        let before = window.messages.len();
        window.messages.retain(|m| !ids.contains(&m.id));
        let mut changed = window.messages.len() != before;
        // The target may be gone even when it was not loaded itself.
        for m in &mut window.messages {
            if m.reply_to().is_some_and(|target| ids.contains(&target)) && m.referenced_message != Some(None) {
                m.referenced_message = Some(None);
                changed = true;
            }
        }
        changed
    }

    /// Runs `edit` on one loaded message; returns its result (true = changed).
    pub(crate) fn edit_message(
        &mut self,
        channel_id: Snowflake,
        id: Snowflake,
        edit: impl FnOnce(&mut Message) -> bool,
    ) -> bool {
        let Some(window) = self.windows.get_mut(&channel_id) else {
            return false;
        };
        match window.messages.iter_mut().find(|m| m.id == id) {
            Some(message) => edit(message),
            None => false,
        }
    }

    /// A loaded message by id.
    pub fn message(&self, channel_id: Snowflake, id: Snowflake) -> Option<&Message> {
        self.windows.get(&channel_id)?.messages.iter().find(|m| m.id == id)
    }

    pub(crate) fn remove_channel(&mut self, channel_id: Snowflake) {
        self.windows.remove(&channel_id);
        self.lru.retain(|id| *id != channel_id);
    }

    pub(crate) fn discard_channel_history(&mut self, channel_id: Snowflake) {
        if let Some(window) = self.windows.get_mut(&channel_id) {
            window.discard_remote_history();
            if window.messages.is_empty() {
                self.remove_channel(channel_id);
            }
        }
    }

    pub(crate) fn discard_remote_history(&mut self) {
        self.windows.retain(|_, window| {
            window.discard_remote_history();
            !window.messages.is_empty()
        });
        self.lru.retain(|id| self.windows.contains_key(id));
    }

    pub(crate) fn retain_channels(&mut self, keep: impl Fn(Snowflake) -> bool) {
        self.windows.retain(|id, _| keep(*id));
        self.lru.retain(|id| self.windows.contains_key(id));
    }

    /// Drop every window when the account changes.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Same session reconnected: keep messages but require a reload
    /// before incoming messages apply (`ready:false`, `:703-755`).
    pub(crate) fn invalidate(&mut self) {
        for window in self.windows.values_mut() {
            window.ready = false;
        }
    }

    fn set_pending_state(&mut self, nonce: Snowflake, from: MessageState, to: MessageState) -> Option<Snowflake> {
        for (channel_id, window) in &mut self.windows {
            if let Some(m) = window.messages.iter_mut().find(|m| m.id == nonce && m.state == from) {
                m.state = to;
                return Some(*channel_id);
            }
        }
        None
    }

    fn touch(&mut self, channel_id: Snowflake) {
        self.lru.retain(|id| *id != channel_id);
        self.lru.push(channel_id);
    }

    /// Get or create a window, update LRU, evict over `MAX_CACHED_CHANNELS`.
    fn entry(&mut self, channel_id: Snowflake) -> &mut MessageWindow {
        self.touch(channel_id);
        if !self.windows.contains_key(&channel_id) {
            self.windows.insert(channel_id, MessageWindow::new());
            while self.windows.len() > MAX_CACHED_CHANNELS {
                let victim = self
                    .lru
                    .iter()
                    .position(|id| Some(*id) != self.selected && *id != channel_id);
                match victim {
                    Some(i) => {
                        let id = self.lru.remove(i);
                        self.windows.remove(&id);
                    }
                    None => break,
                }
            }
        }
        self.windows.entry(channel_id).or_insert_with(MessageWindow::new)
    }
}

#[cfg(test)]
mod around_tests {
    use super::*;
    use crate::state::model::User;

    #[test]
    fn selecting_unloaded_channels_does_not_grow_the_cache_order() {
        let mut store = MessageStore::new();
        let capacity = MAX_CACHED_CHANNELS as u64;
        for id in 1..=capacity {
            store.load(Snowflake(id), Vec::new(), LoadMode::Replace);
        }

        store.select(Some(Snowflake(1)));
        for id in capacity + 1..=capacity * 16 {
            store.select(Some(Snowflake(id)));
        }
        assert_eq!(store.lru.len(), MAX_CACHED_CHANNELS);

        let last = Snowflake(capacity * 16);
        store.load(last, Vec::new(), LoadMode::Replace);
        assert_eq!(store.lru.len(), MAX_CACHED_CHANNELS);
        assert!(store.window(Snowflake(1)).is_some(), "the last selected cached window was touched");
        assert!(store.window(Snowflake(2)).is_none(), "the least recent cached window was evicted");
        assert!(store.window(last).is_some());
    }

    fn test_msg(id: u64, channel_id: u64, content: &str) -> Message {
        Message {
            id: Snowflake(id),
            channel_id: Snowflake(channel_id),
            guild_id: None,
            author: User {
                id: Snowflake(1),
                username: "alice".to_owned(),
                discriminator: None,
                global_name: None,
                avatar: None,
                banner: None,
                avatar_color: None,
                bot: false,
            },
            content: content.to_owned(),
            timestamp: "2026-11-19T00:00:00.000Z".to_owned(),
            edited_timestamp: None,
            kind: 0,
            flags: 0,
            pinned: false,
            mention_everyone: false,
            mentions: Vec::new(),
            mention_roles: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            stickers: Vec::new(),
            reactions: Vec::new(),
            message_reference: None,
            referenced_message: None,
            webhook_id: None,
            nonce: None,
            state: MessageState::Sent,
        }
    }

    #[test]
    fn around_load_tracks_pagination_known_latest_and_unsent() {
        let mut store = MessageStore::default();
        let ch = Snowflake(10);

        // (a) Full 50-message historical slice centered on index 25 (id 75 out of 100..=51)
        let full_newest_first: Vec<Message> = (51..=100).rev().map(|id| test_msg(id, 10, "hist")).collect();
        store.load_around(ch, full_newest_first, Snowflake(75), Some(Snowflake(200)));
        let win = store.window(ch).expect("window exists");
        assert!(win.ready);
        assert!(win.has_more_before);
        assert!(win.has_more_after);
        assert!(!win.pinned_to_end);

        // (b) Replace resets pinned_to_end and clears has_more_after
        store.load(ch, vec![test_msg(200, 10, "latest")], LoadMode::Replace);
        let win = store.window(ch).expect("window exists");
        assert!(win.pinned_to_end);
        assert!(!win.has_more_after);

        // (c) Small channel load_around keeps has_more_after=false, pinned_to_end=true, preserves Sending, and allows apply_incoming
        let pending_author = store.window(ch).unwrap().messages[0].author.clone();
        store.insert_pending(ch, Snowflake(205), "unsent".to_owned(), pending_author);
        let small: Vec<Message> = vec![test_msg(20, 10, "newer"), test_msg(10, 10, "target")];
        store.load_around(ch, small, Snowflake(10), Some(Snowflake(20)));
        let win = store.window(ch).expect("small window");
        assert!(!win.has_more_before);
        assert!(!win.has_more_after);
        assert!(win.pinned_to_end);
        assert!(win.messages.iter().any(|m| m.id == Snowflake(205) && m.state == MessageState::Sending));
        assert!(store.apply_incoming(test_msg(210, 10, "realtime")));

        // (d) 50-message around reaching known_latest clears has_more_after
        let at_latest: Vec<Message> = (151..=200).rev().map(|id| test_msg(id, 10, "edge")).collect();
        store.load_around(ch, at_latest, Snowflake(175), Some(Snowflake(200)));
        let win = store.window(ch).expect("at latest window");
        assert!(win.has_more_before);
        assert!(!win.has_more_after);
        assert!(win.pinned_to_end);
    }
}
