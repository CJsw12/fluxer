//! Read states: READY `read_states`, MESSAGE_ACK and local acks.

use serde_json::Value;

use super::{id_field, is_newer, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::{ReadState, ReadStateWire};

impl ClientState {
    pub(in crate::state) fn load_ready_read_states(&mut self, d: &Value) {
        for rs in parse_list::<ReadStateWire>(d.get("read_states"), "READY.read_states") {
            self.read_states.insert(
                rs.id,
                ReadState {
                    ack_message_id: rs.last_message_id,
                    mention_count: rs.mention_count.unwrap_or(0),
                    version: rs.version,
                    known: true,
                },
            );
        }
    }

    /// Another session read the channel (`SnapshotChannelReducer.applyMessageAck`).
    /// Older versions are ignored; `message_id == "0"` clears the ack.
    pub(in crate::state) fn apply_message_ack(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(channel_id) = id_field(d, "channel_id") else { return };
        let version = id_field(d, "version");
        let rs = self.read_states.entry(channel_id).or_default();
        if let (Some(new), Some(cur)) = (version, rs.version)
            && new < cur
        {
            return;
        }
        rs.ack_message_id = id_field(d, "message_id").filter(|id| id.0 != 0);
        rs.mention_count = d
            .get("mention_count")
            .and_then(Value::as_u64)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX));
        rs.version = version;
        rs.known = true;
        out.push(StateChange::ReadStateChanged { channel_id });
    }

    /// Local ack after the UI shows the newest message and POSTs
    /// `/channels/{id}/messages/{message_id}/ack`.
    pub fn local_ack(&mut self, channel_id: Snowflake, message_id: Snowflake) -> Vec<StateChange> {
        let rs = self.read_states.entry(channel_id).or_default();
        if rs.ack_message_id.is_some_and(|a| a >= message_id) && rs.mention_count == 0 {
            return Vec::new();
        }
        if is_newer(message_id, rs.ack_message_id) {
            rs.ack_message_id = Some(message_id);
        }
        rs.mention_count = 0;
        rs.known = true;
        vec![StateChange::ReadStateChanged { channel_id }]
    }

    /// Manual ack ("Mark as unread"): sets the read cursor to `ack_message_id`
    /// even when moving backward (`0` clears it) with the unread mention count.
    pub fn local_manual_ack(
        &mut self,
        channel_id: Snowflake,
        ack_message_id: Snowflake,
        mention_count: u32,
    ) -> Vec<StateChange> {
        let rs = self.read_states.entry(channel_id).or_default();
        rs.ack_message_id = (ack_message_id.0 != 0).then_some(ack_message_id);
        rs.mention_count = mention_count;
        rs.known = true;
        vec![StateChange::ReadStateChanged { channel_id }]
    }

    /// Counts loaded messages after `ack_message_id` that mention the current user
    /// (`ReadStates.getManualAckMentionCount`).
    pub fn manual_ack_mention_count(&self, channel_id: Snowflake, ack_message_id: Snowflake) -> u32 {
        let Some(channel) = self.channels.get(&channel_id) else {
            return 0;
        };
        let Some(window) = self.messages.window(channel_id) else {
            return 0;
        };
        let Some(me) = self.current_user_id() else {
            return 0;
        };
        let is_private = channel.kind.is_private();
        let guild_id = channel.guild_id;
        window
            .messages
            .iter()
            .filter(|m| m.id > ack_message_id && m.author.id != me && self.mentions_current_user(m, guild_id, is_private))
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }
}
