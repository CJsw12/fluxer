//! READY: replaces everything except message windows of the same session.
//! Domain parts (read states, relationships, presences, settings) are
//! loaded by their own modules.

use serde_json::Value;

use super::{parse, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::model::{Channel, User};

impl ClientState {
    pub(in crate::state) fn apply_ready(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(user) = d.get("user").and_then(|u| parse::<User>(u, "READY.user")) else {
            tracing::warn!("READY without a valid user; ignored");
            return;
        };
        let session_id = d.get("session_id").and_then(Value::as_str).map(str::to_owned);
        // Messages: new session clears windows, same session only invalidates.
        if session_id.is_some() && session_id == self.session_id {
            self.messages.invalidate();
        } else {
            self.messages.clear();
        }
        let messages = std::mem::take(&mut self.messages);
        *self = Self {
            session_id,
            messages,
            ..Self::default()
        };

        for u in parse_list::<User>(d.get("users"), "READY.users") {
            self.users.insert(u.id, u);
        }
        self.users.insert(user.id, user.clone());
        self.current_user = Some(user);

        for g in d.get("guilds").and_then(Value::as_array).into_iter().flatten() {
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
        self.load_ready_presences(d);
        self.load_ready_user_settings(d);
        self.load_ready_guild_settings(d);
        out.push(StateChange::Ready);
    }
}
