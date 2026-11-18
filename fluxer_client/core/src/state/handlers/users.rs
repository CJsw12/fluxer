//! Users: USER_UPDATE.

use serde_json::Value;

use super::id_field;
use crate::state::client::{ClientState, StateChange};
use crate::state::model::{User, overlay};

impl ClientState {
    pub(in crate::state) fn apply_user_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(id) = id_field(d, "id") else { return };
        let merged = match self.users.get(&id) {
            Some(existing) => overlay(existing, d),
            None => serde_json::from_value(d.clone()),
        };
        let user: User = match merged {
            Ok(u) => u,
            Err(err) => {
                tracing::debug!(%id, %err, "USER_UPDATE parse failed");
                return;
            }
        };
        if let Some(me) = self.current_user.as_mut()
            && me.id == id
        {
            *me = user.clone();
        }
        self.users.insert(id, user);
        out.push(StateChange::UserUpdated { user_id: id });
    }
}
