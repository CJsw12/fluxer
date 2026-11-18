//! Reactions: MESSAGE_REACTION_ADD/ADD_MANY/REMOVE/REMOVE_ALL/REMOVE_EMOJI
//! on loaded messages (`ReactionStateMachine.ts`).
//!
//! Deviation: TS tracks known/removed reactor ids per emoji to dedupe
//! replays across REST hydration; here only the current user's `me` flag
//! dedupes (an add by me when `me` is already set, or a remove by me when
//! it is not, is ignored). Other users' events always move the count.

use serde_json::Value;

use super::{id_field, id_value, parse};
use crate::state::client::{ClientState, StateChange};
use crate::state::content::{PartialEmoji, Reaction};
use crate::state::ids::Snowflake;
use crate::state::model::Message;

/// `addToMap`: returns true if the message changed.
fn add_reaction(message: &mut Message, emoji: PartialEmoji, is_me: bool) -> bool {
    match message.reactions.iter_mut().find(|r| r.emoji == emoji) {
        None => {
            message.reactions.push(Reaction { emoji, count: 1, me: is_me });
            true
        }
        Some(r) if is_me && r.me => false,
        Some(r) => {
            r.count = r.count.saturating_add(1);
            r.me |= is_me;
            true
        }
    }
}

/// `removeFromMap`: a reaction whose count reaches 0 is dropped.
fn remove_reaction(message: &mut Message, emoji: &PartialEmoji, is_me: bool) -> bool {
    let Some(i) = message.reactions.iter().position(|r| r.emoji == *emoji) else {
        return false;
    };
    let r = &mut message.reactions[i];
    if is_me && !r.me {
        return false;
    }
    r.count = r.count.saturating_sub(1);
    if is_me {
        r.me = false;
    }
    if r.count == 0 {
        message.reactions.remove(i);
    }
    true
}

impl ClientState {
    /// `(channel_id, message_id)` of a reaction event.
    fn reaction_target(d: &Value) -> Option<(Snowflake, Snowflake)> {
        Some((id_field(d, "channel_id")?, id_field(d, "message_id")?))
    }

    /// Hydrate `member` when the event carries one (`MessageReactionAdd.ts`).
    fn hydrate_reaction_member(&mut self, d: &Value, entry: &Value) {
        if let (Some(guild_id), Some(member)) = (id_field(d, "guild_id"), entry.get("member")) {
            self.hydrate_member(guild_id, member);
        }
    }

    pub(in crate::state) fn apply_reaction_add(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some((channel_id, message_id)) = Self::reaction_target(d) else { return };
        self.hydrate_reaction_member(d, d);
        let Some(user_id) = id_field(d, "user_id") else { return };
        let Some(emoji) = d.get("emoji").and_then(|e| parse::<PartialEmoji>(e, "reaction emoji")) else { return };
        let is_me = Some(user_id) == self.current_user_id();
        if self.messages.edit_message(channel_id, message_id, |m| add_reaction(m, emoji, is_me)) {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }

    /// Debounced adds (`FLAG_DEBOUNCE_MESSAGE_REACTIONS`):
    /// `{channel_id, message_id, guild_id?, reactions: [{user_id, emoji, member?}]}`.
    pub(in crate::state) fn apply_reaction_add_many(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some((channel_id, message_id)) = Self::reaction_target(d) else { return };
        let me = self.current_user_id();
        let mut adds = Vec::new();
        for entry in d.get("reactions").and_then(Value::as_array).into_iter().flatten() {
            self.hydrate_reaction_member(d, entry);
            let Some(user_id) = entry.get("user_id").and_then(id_value) else { continue };
            let Some(emoji) = entry.get("emoji").and_then(|e| parse::<PartialEmoji>(e, "reaction emoji")) else {
                continue;
            };
            adds.push((emoji, Some(user_id) == me));
        }
        let changed = self.messages.edit_message(channel_id, message_id, |m| {
            let mut changed = false;
            for (emoji, is_me) in adds {
                changed |= add_reaction(m, emoji, is_me);
            }
            changed
        });
        if changed {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }

    pub(in crate::state) fn apply_reaction_remove(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some((channel_id, message_id)) = Self::reaction_target(d) else { return };
        self.hydrate_reaction_member(d, d);
        let Some(user_id) = id_field(d, "user_id") else { return };
        let Some(emoji) = d.get("emoji").and_then(|e| parse::<PartialEmoji>(e, "reaction emoji")) else { return };
        let is_me = Some(user_id) == self.current_user_id();
        if self.messages.edit_message(channel_id, message_id, |m| remove_reaction(m, &emoji, is_me)) {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }

    /// Optimistic toggle by the current user before the REST call
    /// (`Messages.handleReaction({optimistic: true})`). The gateway echo is
    /// then a no-op; on REST failure call it again with `!add`.
    pub fn apply_local_reaction(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        emoji: PartialEmoji,
        add: bool,
    ) -> Vec<StateChange> {
        let changed = self.messages.edit_message(channel_id, message_id, |m| {
            if add { add_reaction(m, emoji, true) } else { remove_reaction(m, &emoji, true) }
        });
        if changed { vec![StateChange::MessagesChanged { channel_id }] } else { Vec::new() }
    }

    pub(in crate::state) fn apply_reaction_remove_all(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some((channel_id, message_id)) = Self::reaction_target(d) else { return };
        let changed = self.messages.edit_message(channel_id, message_id, |m| {
            let had = !m.reactions.is_empty();
            m.reactions.clear();
            had
        });
        if changed {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }

    pub(in crate::state) fn apply_reaction_remove_emoji(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some((channel_id, message_id)) = Self::reaction_target(d) else { return };
        let Some(emoji) = d.get("emoji").and_then(|e| parse::<PartialEmoji>(e, "reaction emoji")) else { return };
        let changed = self.messages.edit_message(channel_id, message_id, |m| {
            let before = m.reactions.len();
            m.reactions.retain(|r| r.emoji != emoji);
            m.reactions.len() != before
        });
        if changed {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }
}
