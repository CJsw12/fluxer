//! Channels: CHANNEL_CREATE/UPDATE/UPDATE_BULK/DELETE and PASSIVE_UPDATES.

use serde_json::Value;

use super::{id_field, is_newer, parse};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::{Channel, overlay};

impl ClientState {
    pub(in crate::state) fn apply_channel_create(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(channel) = parse::<Channel>(d, "CHANNEL_CREATE") else { return };
        if let Some(g) = channel.guild_id
            && !self.guilds.contains_key(&g)
        {
            return;
        }
        for r in &channel.recipients {
            self.users.entry(r.id).or_insert_with(|| r.clone());
        }
        let (channel_id, guild_id) = (channel.id, channel.guild_id);
        self.channels.insert(channel_id, channel);
        out.push(StateChange::ChannelUpserted { channel_id, guild_id });
    }

    /// Shallow merge into an existing channel; unknown channels are ignored
    /// (`SnapshotChannelReducer.ts:261-270`).
    pub(in crate::state) fn apply_channel_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(id) = id_field(d, "id") else { return };
        let Some(existing) = self.channels.get(&id) else { return };
        match overlay(existing, d) {
            Ok(mut merged) => {
                merged.id = id;
                let guild_id = merged.guild_id;
                self.channels.insert(id, merged);
                out.push(StateChange::ChannelUpserted { channel_id: id, guild_id });
            }
            Err(err) => tracing::debug!(%id, %err, "CHANNEL_UPDATE merge failed"),
        }
    }

    pub(in crate::state) fn apply_channel_update_bulk(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        for item in d.get("channels").and_then(Value::as_array).into_iter().flatten() {
            self.apply_channel_update(item, out);
        }
    }

    pub(in crate::state) fn apply_channel_delete(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(id) = id_field(d, "id") else { return };
        let Some(channel) = self.channels.remove(&id) else { return };
        self.read_states.remove(&id);
        self.messages.remove_channel(id);
        self.typing.remove(&id);
        // Child threads go with their parent.
        let threads: Vec<Snowflake> = self
            .channels
            .values()
            .filter(|c| c.kind.is_thread() && c.parent_id == Some(id))
            .map(|c| c.id)
            .collect();
        for t in threads {
            self.channels.remove(&t);
            self.read_states.remove(&t);
            self.messages.remove_channel(t);
            self.typing.remove(&t);
            out.push(StateChange::ChannelRemoved { channel_id: t, guild_id: channel.guild_id });
        }
        out.push(StateChange::ChannelRemoved { channel_id: id, guild_id: channel.guild_id });
    }

    /// Passive (large, not subscribed) guild: only newest message ids
    /// (`SnapshotChannelReducer.applyPassiveUpdates`).
    pub(in crate::state) fn apply_passive_updates(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(guild_id) = id_field(d, "guild_id") else { return };
        let Some(map) = d.get("channels").and_then(Value::as_object) else { return };
        for (channel_key, last) in map {
            let (Some(channel_id), Some(last)) = (Snowflake::parse(channel_key), last.as_str().and_then(Snowflake::parse))
            else {
                continue;
            };
            let Some(channel) = self.channels.get_mut(&channel_id) else { continue };
            if channel.guild_id != Some(guild_id) || !channel.kind.has_messages() || !is_newer(last, channel.last_message_id) {
                continue;
            }
            channel.last_message_id = Some(last);
            out.push(StateChange::ReadStateChanged { channel_id });
        }
    }
}
