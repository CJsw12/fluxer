//! Messages: MESSAGE_CREATE/UPDATE/DELETE/DELETE_BULK, the read-state side
//! of an incoming message, and reply lookups.

use serde_json::Value;

use super::{id_field, is_newer, parse, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::Message;

/// What a reply's header shows (`ReplyPreview` in the TS message view).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReplyPreview<'a> {
    /// Not a reply (no default-type `message_reference`).
    NotReply,
    /// The target: the loaded copy when it is in the window (fresh after
    /// edits), else the server's embedded `referenced_message`.
    Loaded(&'a Message),
    /// The server sent `referenced_message: null`, or the target was
    /// deleted while loaded.
    Deleted,
    /// A reply whose target is neither loaded nor embedded; fetch it with
    /// `GET /channels/{channel_id}/messages/{message_id}`.
    NotLoaded { channel_id: Snowflake, message_id: Snowflake },
}

impl ClientState {
    pub(in crate::state) fn apply_message_create(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(message) = parse::<Message>(d, "MESSAGE_CREATE") else { return };
        let channel_id = message.channel_id;
        // `MessageCreate.ts`: the author stops typing.
        self.stop_typing(channel_id, message.author.id, out);
        if self.record_incoming(&message) {
            out.push(StateChange::ReadStateChanged { channel_id });
        }
        if self.messages.apply_incoming(message) {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }

    /// Channel `last_message_id` + read state for an incoming message
    /// (snapshot `applyMessageCreate` + `ReadStates.handleIncomingMessage`).
    /// Own messages ack the channel; others may add a mention.
    ///
    /// A message equal to the channel's `last_message_id` is a duplicate
    /// (POST response plus gateway echo) when it is already in the window
    /// and is not counted again; otherwise, e.g. a DM reopened by
    /// CHANNEL_CREATE carrying this id, it is counted like TS
    /// `reopenedPrivateChannel`.
    fn record_incoming(&mut self, message: &Message) -> bool {
        let self_id = self.current_user_id();
        let authored_by_self = self_id == Some(message.author.id);
        let already_shown = self
            .messages
            .window(message.channel_id)
            .is_some_and(|w| w.messages.iter().any(|m| m.id == message.id));
        let Some(channel) = self.channels.get_mut(&message.channel_id) else {
            return false;
        };
        if !channel.kind.has_messages() {
            return false;
        }
        let previous_last = channel.last_message_id;
        if is_newer(message.id, previous_last) {
            channel.last_message_id = Some(message.id);
        } else if previous_last != Some(message.id) || (already_shown && !authored_by_self) {
            return false;
        }
        let is_private = channel.kind.is_private();
        let guild_id = channel.guild_id;
        let mentions_me = !authored_by_self && self.mentions_current_user(message, guild_id, is_private);
        let rs = self.read_states.entry(message.channel_id).or_default();
        if authored_by_self {
            rs.ack_message_id = Some(message.id);
            rs.mention_count = 0;
            rs.known = true;
        } else {
            if !rs.known {
                rs.ack_message_id = previous_last.filter(|p| *p < message.id);
                rs.known = true;
            }
            let covered = rs.ack_message_id.is_some_and(|ack| message.id <= ack);
            if !covered && mentions_me {
                rs.mention_count = rs.mention_count.saturating_add(1);
            }
        }
        true
    }

    /// `ReadStateEntry.shouldMentionFor` without block/mute/suppress
    /// settings: user mention, @everyone, a role of mine, or any DM message.
    fn mentions_current_user(&self, message: &Message, guild_id: Option<Snowflake>, is_private: bool) -> bool {
        let Some(me) = self.current_user_id() else { return false };
        if message.mentions.iter().any(|u| u.id == me) || message.mention_everyone {
            return true;
        }
        let role_hit = guild_id
            .and_then(|g| self.members.get(&g)?.get(&me))
            .is_some_and(|m| message.mention_roles.iter().any(|r| m.roles.contains(r)));
        role_hit || is_private
    }

    pub(in crate::state) fn apply_message_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let (Some(channel_id), Some(id)) = (id_field(d, "channel_id"), id_field(d, "id")) else {
            return;
        };
        if self.messages.apply_update(channel_id, id, d) {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }

    pub(in crate::state) fn apply_message_delete(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let (Some(channel_id), Some(id)) = (id_field(d, "channel_id"), id_field(d, "id")) else {
            return;
        };
        if self.messages.remove(channel_id, &[id]) {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }

    pub(in crate::state) fn apply_message_delete_bulk(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(channel_id) = id_field(d, "channel_id") else { return };
        let ids = parse_list::<Snowflake>(d.get("ids"), "ids");
        if self.messages.remove(channel_id, &ids) {
            out.push(StateChange::MessagesChanged { channel_id });
        }
    }

    /// The reply header for `message`; see [`ReplyPreview`].
    pub fn reply_preview<'a>(&'a self, message: &'a Message) -> ReplyPreview<'a> {
        let Some(target) = message.reply_to() else {
            return ReplyPreview::NotReply;
        };
        let target_channel = message
            .message_reference
            .as_ref()
            .and_then(|r| r.channel_id)
            .unwrap_or(message.channel_id);
        if let Some(loaded) = self.messages.message(target_channel, target) {
            return ReplyPreview::Loaded(loaded);
        }
        match &message.referenced_message {
            Some(Some(embedded)) => ReplyPreview::Loaded(embedded),
            Some(None) => ReplyPreview::Deleted,
            None => ReplyPreview::NotLoaded { channel_id: target_channel, message_id: target },
        }
    }
}
