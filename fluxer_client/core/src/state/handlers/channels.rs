//! Channels: CHANNEL_CREATE/UPDATE/UPDATE_BULK/DELETE and PASSIVE_UPDATES.

use serde_json::Value;

use super::{id_field, is_newer, parse};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::{Channel, User, overlay};

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
        self.denied_channel_access.remove(&channel_id);
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
        self.denied_channel_access.remove(&id);
        let Some(channel) = self.channels.remove(&id) else {
            if let Some(guild_id) = self.unavailable_channels.remove(&id) {
                self.read_states.remove(&id);
                out.push(StateChange::ChannelRemoved { channel_id: id, guild_id: Some(guild_id) });
            }
            return;
        };
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
        if channel.kind.is_category()
            && let Some(guild_id) = channel.guild_id
        {
            let detached: Vec<Snowflake> = self
                .channels
                .values()
                .filter(|c| c.guild_id == Some(guild_id) && c.parent_id == Some(id))
                .map(|c| c.id)
                .collect();
            for child_id in detached {
                if let Some(child) = self.channels.get_mut(&child_id) {
                    child.parent_id = None;
                    out.push(StateChange::ChannelUpserted { channel_id: child_id, guild_id: Some(guild_id) });
                }
            }
        }
        out.push(StateChange::ChannelRemoved { channel_id: id, guild_id: channel.guild_id });
    }

    pub(in crate::state) fn apply_channel_recipient_add(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(channel_id) = id_field(d, "channel_id") else { return };
        let Some(user_val) = d.get("user") else { return };
        let Some(user) = parse::<User>(user_val, "CHANNEL_RECIPIENT_ADD") else { return };
        self.users.entry(user.id).or_insert_with(|| user.clone());
        let Some(channel) = self.channels.get_mut(&channel_id) else { return };
        if channel.recipients.iter().any(|r| r.id == user.id) {
            return;
        }
        channel.recipients.push(user);
        out.push(StateChange::ChannelUpserted { channel_id, guild_id: channel.guild_id });
    }

    pub(in crate::state) fn apply_channel_recipient_remove(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(channel_id) = id_field(d, "channel_id") else { return };
        let Some(user_id) = d.get("user").and_then(|u| id_field(u, "id")) else { return };
        let new_owner = id_field(d, "owner_id");
        let Some(channel) = self.channels.get_mut(&channel_id) else { return };
        let before = channel.recipients.len();
        channel.recipients.retain(|r| r.id != user_id);
        let owner_changed = new_owner.is_some() && new_owner != channel.owner_id;
        if let Some(owner) = new_owner {
            channel.owner_id = Some(owner);
        }
        if channel.recipients.len() != before || owner_changed {
            out.push(StateChange::ChannelUpserted { channel_id, guild_id: channel.guild_id });
        }
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
