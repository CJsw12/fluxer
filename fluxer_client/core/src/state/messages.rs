//! Per-channel message windows (`messaging/state/ChannelMessages.ts`,
//! `ChannelMessagesLoadStateMachine.ts`, `ChannelIncomingMessageStateMachine.ts`).
//! Trimmed server history is dropped and must be re-fetched.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use serde::Serialize;

use super::content::{MESSAGE_TYPE_REPLY, MessageReference};
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
}

#[derive(Debug, Clone, Default)]
struct OutgoingChannel {
    /// Whether this channel's history was dropped or visited at a historical
    /// slice since the first pending row was queued (requires `r.id > m.id`
    /// for content-based fallback reconciliation).
    history_discarded: bool,
    parked_unsent: Vec<Message>,
    recent_confirmations: VecDeque<(Snowflake, Snowflake)>,
}

impl OutgoingChannel {
    fn record_confirmation(&mut self, author: Snowflake, nonce: Snowflake) {
        if self.recent_confirmations.len() == MAX_LOADED_MESSAGES {
            self.recent_confirmations.pop_front();
        }
        self.recent_confirmations.push_back((author, nonce));
    }

    fn is_empty(&self) -> bool {
        !self.history_discarded && self.parked_unsent.is_empty() && self.recent_confirmations.is_empty()
    }
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
}

/// All channel windows plus LRU bookkeeping and eviction-safe outgoing rows.
#[derive(Debug, Clone, Default)]
pub struct MessageStore {
    windows: HashMap<Snowflake, MessageWindow>,
    /// Least recently used first.
    lru: Vec<Snowflake>,
    selected: Option<Snowflake>,
    /// Per-channel unsent rows and recent nonce confirmations, retained across
    /// LRU window evictions until the channel or account is removed.
    outgoing: HashMap<Snowflake, OutgoingChannel>,
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
    /// ready is replaced whatever `mode` says. Unsent messages are shown
    /// only at the latest edge. Before/after merges keep the other side's
    /// flag and trim the far side once over `MAX_LOADED_MESSAGES`.
    pub fn load(&mut self, channel_id: Snowflake, newest_first: Vec<Message>, mode: LoadMode) {
        let full_page = newest_first.len() == PAGE_SIZE;
        if mode == LoadMode::Around {
            self.apply_around_slice(channel_id, newest_first, full_page, full_page);
            return;
        }
        let page_more_before = full_page && mode != LoadMode::After;
        let page_more_after = full_page && mode == LoadMode::After;
        let mut records = newest_first;
        records.reverse();
        self.touch_window(channel_id);
        self.park_channel_unsent(channel_id);
        let allow_content_match = mode != LoadMode::Before && !page_more_after;
        self.reconcile_channel_unsent(channel_id, &records, allow_content_match);
        let window = self.windows.get_mut(&channel_id).expect("touched window");
        let effective = if window.ready { mode } else { LoadMode::Replace };
        match effective {
            LoadMode::Replace => {
                window.messages.clear();
                window.merge(records);
                window.has_more_before = page_more_before;
                window.has_more_after = page_more_after || mode == LoadMode::Before;
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
            LoadMode::Around => unreachable!(),
        }
        window.ready = true;
        if window.has_more_after {
            self.mark_channel_history_discarded(channel_id);
        } else {
            self.show_channel_unsent_at_latest(channel_id);
        }
        self.cleanup_outgoing(channel_id);
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
        self.touch_window(channel_id);
        self.park_channel_unsent(channel_id);
        self.reconcile_channel_unsent(channel_id, &newest_first, !has_more_after);
        let window = self.windows.get_mut(&channel_id).expect("touched window");
        window.messages.clear();
        window.merge(newest_first);
        window.has_more_before = has_more_before;
        window.has_more_after = has_more_after;
        window.pinned_to_end = !has_more_after;
        window.ready = true;
        if has_more_after {
            self.mark_channel_history_discarded(channel_id);
        } else {
            self.show_channel_unsent_at_latest(channel_id);
        }
        self.cleanup_outgoing(channel_id);
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
        self.insert_pending_with_reply(channel_id, nonce, content, author, None);
    }

    /// Optimistic send with an optional reply target `(target_message_id, mention_replied_user)`.
    /// When `reply` is `Some((target_id, mention))`, sets `kind = MESSAGE_TYPE_REPLY`,
    /// `message_reference`, snapshots the target message into `referenced_message`
    /// if currently loaded, and stores `author` in `mentions` when `mention` is true
    /// so `retry_failed` can reconstruct `allowed_mentions.replied_user`.
    pub fn insert_pending_with_reply(
        &mut self,
        channel_id: Snowflake,
        nonce: Snowflake,
        content: String,
        author: User,
        reply: Option<(Snowflake, bool)>,
    ) {
        self.insert_pending_full(channel_id, nonce, content, author, reply, Vec::new());
    }

    /// Optimistic send with an optional reply target and placeholder attachments
    /// (`attachments` marks that the pending row is an attachment upload so a
    /// failed upload row is not retried as plain text).
    pub fn insert_pending_full(
        &mut self,
        channel_id: Snowflake,
        nonce: Snowflake,
        content: String,
        author: User,
        reply: Option<(Snowflake, bool)>,
        attachments: Vec<super::content::Attachment>,
    ) {
        let (kind, message_reference, referenced_message, mentions) = match reply {
            Some((target_id, mention)) => {
                let target_msg = self.message(channel_id, target_id).cloned();
                let mentions = if mention {
                    vec![author.clone()]
                } else {
                    Vec::new()
                };
                (
                    MESSAGE_TYPE_REPLY,
                    Some(MessageReference {
                        message_id: Some(target_id),
                        channel_id: Some(channel_id),
                        guild_id: None,
                        kind: 0,
                    }),
                    target_msg.map(|m| Some(Box::new(m))),
                    mentions,
                )
            }
            None => (0, None, None, Vec::new()),
        };
        let message = Message {
            id: nonce,
            channel_id,
            guild_id: None,
            author,
            content,
            timestamp: String::new(),
            edited_timestamp: None,
            kind,
            flags: 0,
            pinned: false,
            mention_everyone: false,
            mentions,
            mention_roles: Vec::new(),
            attachments,
            embeds: Vec::new(),
            stickers: Vec::new(),
            reactions: Vec::new(),
            message_reference,
            referenced_message,
            webhook_id: None,
            nonce: Some(nonce.to_string()),
            state: MessageState::Sending,
        };
        self.touch_window(channel_id);
        let has_more_after = self.windows.get(&channel_id).is_some_and(|w| w.has_more_after);
        if has_more_after {
            let out = self.outgoing.entry(channel_id).or_default();
            out.history_discarded = true;
            out.parked_unsent.push(message);
        } else if let Some(window) = self.windows.get_mut(&channel_id) {
            window.merge(vec![message]);
        }
    }

    /// A server message from `MESSAGE_CREATE` or the `POST` response.
    /// Returns true if the channel's state changed.
    ///
    /// Order (`ChannelIncomingMessageStateMachine`): a pending message with
    /// id == nonce and the same author is confirmed. A confirmation beyond
    /// the visible slice is fetched by pagination. Other events are ignored
    /// if the window is not ready or newer pages exist (`has_more_after`);
    /// otherwise appended (`appendIncomingMessage`). While pinned to the
    /// end the window is trimmed to `TRUNCATED_MESSAGE_VIEW_SIZE` from the
    /// top; when scrolled up and already over `MAX_LOADED_MESSAGES` it is
    /// trimmed from the bottom instead.
    ///
    /// Deviation: TS ignores a nonce match when the window is not ready
    /// (`MessagingMessages.handleIncomingMessage`); here the pending message
    /// is still confirmed (even after LRU window eviction) so it does not
    /// linger as `Sending`.
    pub fn apply_incoming(&mut self, mut message: Message) -> bool {
        message.state = MessageState::Sent;
        let channel_id = message.channel_id;
        let author = message.author.id;
        if let Some(nonce) = message.nonce_id()
            && self.remove_channel_pending(channel_id, |m| m.id == nonce && m.author.id == author)
        {
            // A page load or LRU eviction may drop the confirmed row while the upload request still finishes.
            self.outgoing.entry(channel_id).or_default().record_confirmation(author, nonce);
            if let Some(window) = self.windows.get_mut(&channel_id)
                && (!window.has_more_after || window.position(message.id).is_some())
            {
                window.merge(vec![message]);
            }
            return true;
        }
        let Some(window) = self.windows.get_mut(&channel_id) else {
            return false;
        };
        if !window.ready || window.has_more_after {
            return false;
        }
        let scrolled_over_cap = !window.pinned_to_end && window.messages.len() > MAX_LOADED_MESSAGES
            && window.messages.iter().filter(|m| m.state == MessageState::Sent).count() > MAX_LOADED_MESSAGES;
        window.merge(vec![message]);
        if window.pinned_to_end && window.messages.len() > TRUNCATED_MESSAGE_VIEW_SIZE {
            self.park_channel_unsent(channel_id);
            if let Some(window) = self.windows.get_mut(&channel_id) {
                window.trim_oldest(TRUNCATED_MESSAGE_VIEW_SIZE);
            }
        } else if scrolled_over_cap {
            self.park_channel_unsent(channel_id);
            if let Some(window) = self.windows.get_mut(&channel_id) {
                window.trim_newest(TRUNCATED_MESSAGE_VIEW_SIZE);
            }
            self.mark_channel_history_discarded(channel_id);
        }
        self.show_channel_unsent_at_latest(channel_id);
        self.cleanup_outgoing(channel_id);
        true
    }

    /// Send failed: `Sending` -> `Failed`. Returns the channel to redraw.
    pub fn mark_failed(&mut self, nonce: Snowflake) -> Option<Snowflake> {
        self.set_pending_state(nonce, MessageState::Sending, MessageState::Failed)
    }

    pub fn has_confirmed_nonce(&self, channel: Snowflake, author: Snowflake, nonce: Snowflake) -> bool {
        let confirmed_in_outgoing = self
            .outgoing
            .get(&channel)
            .is_some_and(|out| out.recent_confirmations.contains(&(author, nonce)));
        confirmed_in_outgoing
            || self.window(channel).is_some_and(|window| {
                window.messages.iter().any(|message| {
                    message.state == MessageState::Sent
                        && message.author.id == author
                        && message.nonce_id() == Some(nonce)
                })
            })
    }

    /// Retry: `Failed` -> `Sending`. Returns the channel to redraw.
    pub fn mark_sending(&mut self, nonce: Snowflake) -> Option<Snowflake> {
        self.set_pending_state(nonce, MessageState::Failed, MessageState::Sending)
    }

    /// Drop a local pending/failed message (user discarded it).
    pub fn remove_pending(&mut self, channel_id: Snowflake, nonce: Snowflake) -> bool {
        let removed = self.remove_channel_pending(channel_id, |m| m.id == nonce);
        if removed {
            self.cleanup_outgoing(channel_id);
        }
        removed
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

    /// Removes `ids` from the channel's window and marks references to them as deleted.
    pub(crate) fn remove(&mut self, channel_id: Snowflake, ids: &[Snowflake]) -> bool {
        let bulk_ids = (ids.len() > 1).then(|| ids.iter().copied().collect::<HashSet<_>>());
        let is_deleted = |id| bulk_ids.as_ref().map_or_else(|| ids.first() == Some(&id), |set| set.contains(&id));
        let mut changed = false;
        if let Some(window) = self.windows.get_mut(&channel_id) {
            let before = window.messages.len();
            window.messages.retain(|m| !is_deleted(m.id));
            changed |= window.messages.len() != before;
        }
        // The target may be gone even when it was not loaded itself.
        for reply in self.replies_to(channel_id, is_deleted) {
            if reply.referenced_message != Some(None) {
                reply.referenced_message = Some(None);
                changed = true;
            }
        }
        changed
    }

    /// Resolves a single reply target fetched via `GET /channels/{c}/messages/{m}`.
    /// Pass `Some(target)` when found, or `None` when the server confirmed the
    /// target message is gone (404). Updates every loaded or parked reply in
    /// the store that still has an unresolved reference to the target.
    pub fn resolve_reply_target(
        &mut self,
        target_channel: Snowflake,
        target_id: Snowflake,
        resolved: Option<Message>,
    ) -> bool {
        if self.message(target_channel, target_id).is_some()
            || resolved.as_ref().is_some_and(|m| m.id != target_id || m.channel_id != target_channel)
        {
            return false;
        }
        let replacement = Some(resolved.map(Box::new));
        let mut changed = false;
        for reply in self.replies_to(target_channel, |id| id == target_id) {
            if reply.referenced_message.is_none() {
                reply.referenced_message = replacement.clone();
                changed = true;
            }
        }
        changed
    }

    /// A full gateway edit supplies the current text for reply previews even
    /// when the target has fallen out of the message window.
    pub(super) fn update_reply_target(
        &mut self,
        channel: Snowflake,
        id: Snowflake,
        payload: &serde_json::Value,
    ) -> BTreeSet<Snowflake> {
        let mut replies = self
            .replies_to(channel, |target| target == id)
            .filter(|reply| reply.referenced_message != Some(None))
            .peekable();
        let mut changed = BTreeSet::new();
        if replies.peek().is_none() {
            return changed;
        }
        let Ok(message) = serde_json::from_value::<Message>(payload.clone()) else {
            return changed;
        };
        let updated = Some(Some(Box::new(message)));
        for reply in replies {
            if reply.referenced_message != updated {
                reply.referenced_message.clone_from(&updated);
                changed.insert(reply.channel_id);
            }
        }
        changed
    }

    fn replies_to<'a>(
        &'a mut self,
        channel: Snowflake,
        is_target: impl Fn(Snowflake) -> bool + 'a,
    ) -> impl Iterator<Item = &'a mut Message> {
        self.windows
            .values_mut()
            .flat_map(|window| &mut window.messages)
            .chain(self.outgoing.values_mut().flat_map(|out| &mut out.parked_unsent))
            .filter(move |m| {
                m.reply_to().is_some_and(&is_target)
                    && m.message_reference.as_ref().and_then(|r| r.channel_id).unwrap_or(m.channel_id) == channel
            })
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

    /// A loaded message or an unsent row by id.
    pub fn message(&self, channel_id: Snowflake, id: Snowflake) -> Option<&Message> {
        let in_window = self
            .windows
            .get(&channel_id)
            .and_then(|window| window.messages.iter().find(|m| m.id == id));
        in_window.or_else(|| {
            self.outgoing
                .get(&channel_id)
                .and_then(|out| out.parked_unsent.iter().find(|m| m.id == id))
        })
    }

    pub(crate) fn remove_channel(&mut self, channel_id: Snowflake) {
        self.windows.remove(&channel_id);
        self.lru.retain(|id| *id != channel_id);
        self.outgoing.remove(&channel_id);
    }

    pub(crate) fn clear_channel_messages(&mut self, channel_id: Snowflake) -> bool {
        let mut changed = self.outgoing.remove(&channel_id).is_some();
        if let Some(window) = self.windows.get_mut(&channel_id) {
            if !window.messages.is_empty() || window.has_more_before || window.has_more_after {
                changed = true;
            }
            window.messages.clear();
            window.has_more_before = false;
            window.has_more_after = false;
            window.ready = true;
            window.pinned_to_end = true;
        }
        changed
    }

    pub(crate) fn discard_channel_history(&mut self, channel_id: Snowflake) {
        self.park_channel_unsent(channel_id);
        let unsent = self
            .outgoing
            .get_mut(&channel_id)
            .map(|out| {
                out.history_discarded = true;
                std::mem::take(&mut out.parked_unsent)
            })
            .unwrap_or_default();
        if !unsent.is_empty() {
            if let Some(window) = self.windows.get_mut(&channel_id) {
                let mut next = MessageWindow::new();
                next.merge(unsent);
                *window = next;
            } else {
                let out = self.outgoing.entry(channel_id).or_default();
                out.history_discarded = true;
                out.parked_unsent = unsent;
            }
        } else {
            self.windows.remove(&channel_id);
            self.lru.retain(|id| *id != channel_id);
            if let Some(out) = self.outgoing.get_mut(&channel_id)
                && out.recent_confirmations.is_empty()
            {
                self.outgoing.remove(&channel_id);
            }
        }
    }

    pub(crate) fn discard_remote_history(&mut self) {
        let channels: Vec<Snowflake> = self
            .windows
            .keys()
            .chain(self.outgoing.keys())
            .copied()
            .collect();
        for channel_id in channels {
            self.discard_channel_history(channel_id);
        }
    }

    pub(crate) fn retain_channels(&mut self, keep: impl Fn(Snowflake) -> bool) {
        self.windows.retain(|id, _| keep(*id));
        self.lru.retain(|id| self.windows.contains_key(id));
        self.outgoing.retain(|id, _| keep(*id));
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

    fn park_channel_unsent(&mut self, channel_id: Snowflake) {
        let Some(window) = self.windows.get_mut(&channel_id) else {
            return;
        };
        let extracted: Vec<Message> = window.messages.extract_if(.., |m| m.state != MessageState::Sent).collect();
        if !extracted.is_empty() {
            self.outgoing.entry(channel_id).or_default().parked_unsent.extend(extracted);
        }
    }

    fn mark_channel_history_discarded(&mut self, channel_id: Snowflake) {
        if let Some(out) = self.outgoing.get_mut(&channel_id)
            && !out.parked_unsent.is_empty()
        {
            out.history_discarded = true;
        }
    }

    fn show_channel_unsent_at_latest(&mut self, channel_id: Snowflake) {
        let Some(window) = self.windows.get_mut(&channel_id) else {
            return;
        };
        if window.has_more_after {
            return;
        }
        let Some(out) = self.outgoing.get_mut(&channel_id) else {
            return;
        };
        if !out.parked_unsent.is_empty() {
            let unsent = std::mem::take(&mut out.parked_unsent);
            window.merge(unsent);
        }
    }

    /// Only latest pages can infer delivery from content. After dropping history,
    /// visiting an older slice, or evicting the window, require a server ID
    /// newer than the nonce.
    fn reconcile_channel_unsent(&mut self, channel_id: Snowflake, records: &[Message], allow_content_match: bool) {
        let has_more_after = self.windows.get(&channel_id).is_some_and(|w| w.has_more_after);
        let Some(out) = self.outgoing.get_mut(&channel_id) else {
            return;
        };
        if out.parked_unsent.is_empty() {
            return;
        }
        let window_ref = self.windows.get(&channel_id);
        let mut unmatched: Vec<&Message> = records
            .iter()
            .filter(|r| r.nonce_id().is_some() || window_ref.is_none_or(|w| w.position(r.id).is_none()))
            .collect();
        let reply_to = |m: &Message| m.message_reference.as_ref().and_then(|r| r.message_id);
        let require_newer = out.history_discarded || has_more_after;
        out.parked_unsent.retain(|m| {
            let hit = unmatched
                .iter()
                .position(|r| r.author.id == m.author.id && r.nonce_id() == Some(m.id))
                .or_else(|| {
                    unmatched.iter().position(|r| {
                        allow_content_match
                            && r.nonce_id().is_none()
                            && (!require_newer || r.id > m.id)
                            && r.author.id == m.author.id
                            && reply_to(r) == reply_to(m)
                            && r.content == m.content
                    })
                });
            match hit {
                Some(i) => {
                    unmatched.swap_remove(i);
                    false
                }
                None => true,
            }
        });
    }

    fn cleanup_outgoing(&mut self, channel_id: Snowflake) {
        let has_unsent_in_window = self
            .windows
            .get(&channel_id)
            .is_some_and(|w| w.messages.iter().any(|m| m.state != MessageState::Sent));
        if let Some(out) = self.outgoing.get_mut(&channel_id) {
            if out.parked_unsent.is_empty() && !has_unsent_in_window {
                out.history_discarded = false;
            }
            if out.is_empty() {
                self.outgoing.remove(&channel_id);
            }
        }
    }

    fn remove_channel_pending(&mut self, channel_id: Snowflake, matches: impl Fn(&Message) -> bool) -> bool {
        if let Some(out) = self.outgoing.get_mut(&channel_id)
            && let Some(i) = out.parked_unsent.iter().position(|m| m.state != MessageState::Sent && matches(m))
        {
            out.parked_unsent.remove(i);
            return true;
        }
        if let Some(window) = self.windows.get_mut(&channel_id)
            && let Some(i) = window.messages.iter().position(|m| m.state != MessageState::Sent && matches(m))
        {
            window.messages.remove(i);
            return true;
        }
        false
    }

    fn set_pending_state(&mut self, nonce: Snowflake, from: MessageState, to: MessageState) -> Option<Snowflake> {
        for (channel_id, window) in &mut self.windows {
            if let Some(m) = window.messages.iter_mut().find(|m| m.id == nonce && m.state == from) {
                m.state = to;
                return Some(*channel_id);
            }
        }
        for (channel_id, out) in &mut self.outgoing {
            if let Some(m) = out.parked_unsent.iter_mut().find(|m| m.id == nonce && m.state == from) {
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
    /// When a window is evicted, any unsent rows in it are parked in `outgoing`.
    fn touch_window(&mut self, channel_id: Snowflake) {
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
                        if let Some(mut evicted) = self.windows.remove(&id) {
                            let unsent: Vec<Message> =
                                evicted.messages.extract_if(.., |m| m.state != MessageState::Sent).collect();
                            if !unsent.is_empty() {
                                let out = self.outgoing.entry(id).or_default();
                                out.history_discarded = true;
                                out.parked_unsent.extend(unsent);
                            } else {
                                self.cleanup_outgoing(id);
                            }
                        }
                    }
                    None => break,
                }
            }
        }
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

    #[test]
    fn historical_jump_preserves_pending_without_advancing_the_history_cursor() {
        let mut store = MessageStore::new();
        let channel = Snowflake(10);
        let latest = test_msg(300, 10, "latest");
        let author = latest.author.clone();
        store.load(channel, vec![latest], LoadMode::Replace);
        let (failed, confirmed, discarded, while_hidden) = (Snowflake(301), Snowflake(302), Snowflake(303), Snowflake(304));
        store.insert_pending(channel, failed, "same text".to_owned(), author.clone());
        store.insert_pending(channel, confirmed, "delivered".to_owned(), author.clone());
        store.insert_pending(channel, discarded, "discarded".to_owned(), author.clone());

        let older = (51..=100).rev().map(|id| test_msg(id, 10, "same text")).collect();
        store.load_around(channel, older, Snowflake(75), Some(Snowflake(300)));
        store.insert_pending(channel, while_hidden, "still sending".to_owned(), author.clone());
        assert_eq!(store.mark_failed(failed), Some(channel));
        assert_eq!(store.mark_sending(failed), Some(channel));
        assert_eq!(store.mark_failed(failed), Some(channel));
        assert!(store.remove_pending(channel, discarded));

        let mut delivered = test_msg(310, 10, "delivered");
        delivered.nonce = Some(confirmed.to_string());
        delivered.author.id = Snowflake(5);
        assert!(!store.apply_incoming(delivered.clone()));
        assert!(store.message(channel, confirmed).is_some());

        delivered.author = author.clone();
        assert!(store.apply_incoming(delivered.clone()));
        assert!(store.message(channel, confirmed).is_none());
        assert!(store.has_confirmed_nonce(channel, author.id, confirmed));
        let window = store.window(channel).unwrap();
        assert!(window.has_more_after);
        assert_eq!(window.messages.len(), 50);
        assert_eq!(window.messages.last().unwrap().id, Snowflake(100));

        let intervening = (101..=150).rev().map(|id| test_msg(id, 10, "same text")).collect();
        store.load(channel, intervening, LoadMode::After);
        assert_eq!(store.window(channel).unwrap().messages.last().unwrap().id, Snowflake(150));

        delivered.nonce = None;
        let tail = vec![delivered, test_msg(300, 10, "same text")];
        store.load(channel, tail, LoadMode::After);
        let window = store.window(channel).unwrap();
        assert!(!window.has_more_after);
        assert_eq!(window.messages.len(), 104);
        assert_eq!(store.message(channel, failed).unwrap().state, MessageState::Failed);
        assert_eq!(store.message(channel, while_hidden).unwrap().state, MessageState::Sending);
        assert!(store.message(channel, discarded).is_none());
        assert!(store.has_confirmed_nonce(channel, author.id, confirmed));
    }

    #[test]
    fn history_trims_keep_failed_messages_separate_from_the_server_window() {
        let mut store = MessageStore::new();
        let channel = Snowflake(10);
        let latest = (201..=250).rev().map(|id| test_msg(id, 10, "sent")).collect();
        store.load(channel, latest, LoadMode::Replace);
        let author = store.window(channel).unwrap().messages[0].author.clone();
        let failed = Snowflake(1);
        store.insert_pending(channel, failed, "failed".to_owned(), author);
        store.mark_failed(failed);

        let older = (151..=200).rev().map(|id| test_msg(id, 10, "sent")).collect();
        store.load(channel, older, LoadMode::Before);
        assert!(store.apply_incoming(test_msg(251, 10, "sent")));
        let window = store.window(channel).unwrap();
        assert_eq!(window.messages.len(), TRUNCATED_MESSAGE_VIEW_SIZE + 1);
        assert_eq!(window.messages[0].state, MessageState::Failed);

        for start in [102, 52, 2] {
            let page = (start..start + 50).rev().map(|id| test_msg(id, 10, "sent")).collect();
            store.load(channel, page, LoadMode::Before);
        }
        let window = store.window(channel).unwrap();
        assert!(window.has_more_after);
        assert_eq!(window.messages.len(), TRUNCATED_MESSAGE_VIEW_SIZE);
        assert_eq!(window.messages.first().unwrap().id, Snowflake(2));
        assert_eq!(window.messages.last().unwrap().id, Snowflake(101));
        assert_eq!(store.message(channel, failed).unwrap().state, MessageState::Failed);

        store.load(channel, vec![test_msg(251, 10, "sent")], LoadMode::Replace);
        assert_eq!(store.window(channel).unwrap().messages[0].state, MessageState::Failed);
    }

    #[test]
    fn evicted_channel_preserves_unsent_rows_and_confirmations() {
        let mut store = MessageStore::new();
        let evicted = Snowflake(1);
        store.load(evicted, vec![test_msg(100, 1, "same text")], LoadMode::Replace);
        let author = store.window(evicted).unwrap().messages[0].author.clone();
        let (failed, confirmed, discarded, no_nonce) = (Snowflake(101), Snowflake(102), Snowflake(103), Snowflake(104));
        store.insert_pending(evicted, failed, "same text".to_owned(), author.clone());
        store.insert_pending(evicted, confirmed, "confirmed while evicted".to_owned(), author.clone());
        store.insert_pending(evicted, discarded, "discarded while evicted".to_owned(), author.clone());
        store.insert_pending(evicted, no_nonce, "matched without nonce".to_owned(), author.clone());

        // Evict `evicted` by loading MAX_CACHED_CHANNELS other channels while selecting another channel.
        store.select(Some(Snowflake(2)));
        for id in 2..=(MAX_CACHED_CHANNELS as u64 + 1) {
            store.load(Snowflake(id), vec![test_msg(200, id, "other")], LoadMode::Replace);
        }
        assert!(store.window(evicted).is_none(), "window was evicted from LRU");

        // Operations on the evicted channel's pending rows still work.
        assert_eq!(store.mark_failed(failed), Some(evicted));
        assert_eq!(store.mark_sending(failed), Some(evicted));
        assert_eq!(store.mark_failed(failed), Some(evicted));
        assert!(store.remove_pending(evicted, discarded));
        assert!(store.message(evicted, discarded).is_none());
        assert_eq!(store.message(evicted, failed).unwrap().state, MessageState::Failed);

        // Nonce confirmation arrives while the window is evicted.
        let mut delivered = test_msg(110, 1, "confirmed while evicted");
        delivered.nonce = Some(confirmed.to_string());
        delivered.author.id = Snowflake(99);
        assert!(!store.apply_incoming(delivered.clone()), "different author does not confirm");
        assert!(store.message(evicted, confirmed).is_some());

        delivered.author = author.clone();
        assert!(store.apply_incoming(delivered.clone()));
        assert!(store.message(evicted, confirmed).is_none());
        assert!(store.has_confirmed_nonce(evicted, author.id, confirmed));
        assert!(store.window(evicted).is_none(), "confirmation does not resurrect an unready window");

        // Reloading the evicted channel at a historical slice keeps unsent rows parked.
        let older: Vec<Message> = (1..=50).rev().map(|id| test_msg(id, 1, "same text")).collect();
        store.load_around(evicted, older, Snowflake(25), Some(Snowflake(120)));
        assert!(store.window(evicted).unwrap().has_more_after);
        assert!(store.window(evicted).unwrap().messages.iter().all(|m| m.state == MessageState::Sent));
        assert_eq!(store.message(evicted, failed).unwrap().state, MessageState::Failed);

        // Reloading the latest page reconciles newer content-matched messages, avoids matching older history,
        // and restores the remaining failed row.
        let newer_no_nonce = test_msg(120, 1, "matched without nonce");
        delivered.nonce = None;
        store.load(
            evicted,
            vec![newer_no_nonce, delivered, test_msg(100, 1, "same text")],
            LoadMode::Replace,
        );
        let window = store.window(evicted).unwrap();
        assert!(!window.has_more_after);
        assert!(store.message(evicted, no_nonce).is_none(), "newer message without nonce reconciled");
        assert_eq!(store.message(evicted, failed).unwrap().state, MessageState::Failed);
        assert!(store.has_confirmed_nonce(evicted, author.id, confirmed));
    }

    #[test]
    fn resolve_reply_target_updates_loaded_and_parked_replies() {
        let mut store = MessageStore::new();
        let ch = Snowflake(10);
        let mut reply_in_win = test_msg(200, 10, "reply 1");
        reply_in_win.message_reference = Some(MessageReference {
            message_id: Some(Snowflake(50)),
            channel_id: Some(ch),
            guild_id: None,
            kind: 0,
        });
        store.load(ch, vec![reply_in_win], LoadMode::Replace);

        let author = store.window(ch).unwrap().messages[0].author.clone();
        store.insert_pending_with_reply(
            ch,
            Snowflake(201),
            "pending reply to unloaded".to_owned(),
            author,
            Some((Snowflake(50), true)),
        );
        // Jump to an older slice so the pending reply is parked in `outgoing`.
        let older: Vec<Message> = (60..=109).rev().map(|id| test_msg(id, 10, "hist")).collect();
        store.load_around(ch, older, Snowflake(80), Some(Snowflake(200)));
        assert!(store.message(ch, Snowflake(201)).unwrap().referenced_message.is_none());

        // Resolve with `Some(target_msg)` updates the parked reply.
        let target_msg = test_msg(50, 10, "original target");
        assert!(store.resolve_reply_target(ch, Snowflake(50), Some(target_msg.clone())));
        assert!(!store.resolve_reply_target(ch, Snowflake(50), Some(target_msg)), "idempotent second call");
        assert_eq!(
            store.message(ch, Snowflake(201)).unwrap().referenced_message.as_ref().and_then(|r| r.as_ref()).map(|m| m.content.as_str()),
            Some("original target"),
        );

        // Return to latest and resolve with `None` (404 deleted) -> `Some(None)`.
        let mut reply_in_win = test_msg(200, 10, "reply 1");
        reply_in_win.message_reference = Some(MessageReference {
            message_id: Some(Snowflake(50)),
            channel_id: Some(ch),
            guild_id: None,
            kind: 0,
        });
        store.load(ch, vec![reply_in_win], LoadMode::Replace);
        assert!(store.resolve_reply_target(ch, Snowflake(50), None));
        assert_eq!(store.message(ch, Snowflake(200)).unwrap().referenced_message, Some(None));
        assert!(store.message(ch, Snowflake(201)).unwrap().referenced_message.as_ref().unwrap().is_some());
        let mut cross_channel = test_msg(202, 11, "cross-channel reply");
        cross_channel.message_reference = store.message(ch, Snowflake(200)).unwrap().message_reference.clone();
        store.load(Snowflake(11), vec![cross_channel], LoadMode::Replace);
        assert!(store.remove(ch, &[Snowflake(50)]));
        assert_eq!(store.message(ch, Snowflake(201)).unwrap().referenced_message, Some(None));
        assert_eq!(store.message(Snowflake(11), Snowflake(202)).unwrap().referenced_message, Some(None));
    }
}
