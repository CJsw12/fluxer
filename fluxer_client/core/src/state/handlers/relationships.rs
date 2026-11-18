//! Relationships: READY `relationships`, RELATIONSHIP_ADD/UPDATE/REMOVE
//! (`Relationships.ts`). The carried `user` goes into `ClientState::users`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{id_field, parse, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::{User, null_default, overlay};

/// `RelationshipTypes` (`UserConstants.ts:401-406`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RelationshipKind {
    Friend,
    Blocked,
    IncomingRequest,
    OutgoingRequest,
    Unknown(u8),
}

impl RelationshipKind {
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Friend,
            2 => Self::Blocked,
            3 => Self::IncomingRequest,
            4 => Self::OutgoingRequest,
            other => Self::Unknown(other),
        }
    }
}

/// `RelationshipWire` without `user` (stored in `users`). `id` is the
/// other user's id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub id: Snowflake,
    /// Raw `RelationshipTypes` value; see [`Relationship::kind`].
    #[serde(rename = "type", default, deserialize_with = "null_default")]
    pub kind_raw: u8,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub share_voice_activity: Option<bool>,
    #[serde(default)]
    pub friend_shares_voice_activity: Option<bool>,
}

impl Relationship {
    pub fn kind(&self) -> RelationshipKind {
        RelationshipKind::from_raw(self.kind_raw)
    }
}

impl ClientState {
    /// `loadRelationships`: READY replaces the set.
    pub(in crate::state) fn load_ready_relationships(&mut self, d: &Value) {
        self.relationships.clear();
        let items = d.get("relationships").and_then(Value::as_array).into_iter().flatten();
        for item in items {
            self.store_relationship_user(item);
        }
        for r in parse_list::<Relationship>(d.get("relationships"), "READY.relationships") {
            self.relationships.insert(r.id, r);
        }
    }

    /// `updateRelationship`: merge into the existing entry
    /// (`withUpdates`) or insert.
    pub(in crate::state) fn apply_relationship_upsert(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(id) = id_field(d, "id") else { return };
        self.store_relationship_user(d);
        let merged = match self.relationships.get(&id) {
            Some(existing) => overlay(existing, d).ok(),
            None => parse::<Relationship>(d, "RELATIONSHIP"),
        };
        let Some(relationship) = merged else {
            tracing::debug!(%id, "RELATIONSHIP payload not applied");
            return;
        };
        self.relationships.insert(id, relationship);
        out.push(StateChange::RelationshipChanged { user_id: id });
    }

    pub(in crate::state) fn apply_relationship_remove(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(id) = id_field(d, "id") else { return };
        if self.relationships.remove(&id).is_some() {
            out.push(StateChange::RelationshipChanged { user_id: id });
        }
    }

    fn store_relationship_user(&mut self, item: &Value) {
        if let Some(user) = item.get("user").and_then(|u| parse::<User>(u, "relationship.user")) {
            self.users.insert(user.id, user);
        }
    }

    pub fn relationship_kind(&self, user_id: Snowflake) -> Option<RelationshipKind> {
        self.relationships.get(&user_id).map(Relationship::kind)
    }

    pub fn is_blocked(&self, user_id: Snowflake) -> bool {
        self.relationship_kind(user_id) == Some(RelationshipKind::Blocked)
    }

    /// Relationships of one kind, by user id.
    pub fn relationships_of(&self, kind: RelationshipKind) -> Vec<&Relationship> {
        self.relationships.values().filter(|r| r.kind() == kind).collect()
    }
}
