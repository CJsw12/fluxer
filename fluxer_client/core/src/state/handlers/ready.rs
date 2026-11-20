//! READY: replaces everything except message windows of the same session.
//! Domain parts (read states, relationships, presences, settings) are
//! loaded by their own modules.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{id_field, parse, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::model::{Channel, User};

impl ClientState {
    pub(in crate::state) fn apply_ready(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(user) = d.get("user").and_then(|u| parse::<User>(u, "READY.user")) else {
            tracing::warn!("READY without a valid user; ignored");
            return;
        };
        let session_id = d.get("session_id").and_then(Value::as_str).map(str::to_owned);
        let guilds = d.get("guilds").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        let mut unavailable_channels = std::mem::take(&mut self.unavailable_channels);
        if self.current_user_id() == Some(user.id) {
            let unavailable: BTreeSet<_> = guilds.iter()
                .filter(|guild| guild.get("unavailable").and_then(Value::as_bool) == Some(true))
                .filter_map(|guild| id_field(guild, "id"))
                .collect();
            unavailable_channels.retain(|_, guild| unavailable.contains(guild));
            unavailable_channels.extend(self.channels.values().filter_map(|channel| {
                let guild = channel.guild_id?;
                unavailable.contains(&guild).then_some((channel.id, guild))
            }));
        } else {
            unavailable_channels.clear();
        }
        // Messages: new session clears windows, same session only invalidates.
        if session_id.is_some() && session_id == self.session_id {
            self.messages.invalidate();
        } else {
            self.messages.clear();
        }
        let messages = std::mem::take(&mut self.messages);
        *self = Self {
            ready_generation: self.ready_generation.saturating_add(1),
            session_id,
            messages,
            unavailable_channels,
            ..Self::default()
        };

        for u in parse_list::<User>(d.get("users"), "READY.users") {
            self.users.insert(u.id, u);
        }
        self.users.insert(user.id, user.clone());
        self.current_user = Some(user);
        self.current_user_mfa_enabled = d
            .get("user")
            .and_then(|user| user.get("mfa_enabled"))
            .and_then(Value::as_bool);

        for g in guilds {
            self.insert_guild_payload(g);
        }
        for c in parse_list::<Channel>(d.get("private_channels"), "READY.private_channels") {
            for r in &c.recipients {
                self.users.entry(r.id).or_insert_with(|| r.clone());
            }
            self.channels.insert(c.id, c);
        }
        self.load_ready_read_states(d);
        self.load_ready_relationships(d);
        self.load_ready_notes(d);
        self.load_ready_presences(d);
        self.load_ready_user_settings(d);
        self.load_ready_guild_settings(d);
        out.push(StateChange::Ready);
    }
}
