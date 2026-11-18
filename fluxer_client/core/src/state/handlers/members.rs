//! Guild members: GUILD_MEMBER_ADD/UPDATE/REMOVE, plus member hydration
//! from other events (`GuildMembers.hydrateIfMissing`).

use serde_json::Value;

use super::{id_field, parse};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::Member;

impl ClientState {
    pub(in crate::state) fn apply_member_upsert(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(guild_id) = id_field(d, "guild_id") else { return };
        let Some(member) = parse::<Member>(d, "GUILD_MEMBER") else { return };
        if !self.guilds.contains_key(&guild_id) {
            return;
        }
        let user_id = member.user.id;
        self.users.insert(user_id, member.user.clone());
        self.members.entry(guild_id).or_default().insert(user_id, member);
        out.push(StateChange::MemberUpdated { guild_id, user_id });
        if Some(user_id) == self.current_user_id() {
            out.push(StateChange::PermissionsChanged { guild_id });
        }
    }

    pub(in crate::state) fn apply_member_remove(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(guild_id) = id_field(d, "guild_id") else { return };
        let Some(user_id) = d.get("user").and_then(|u| id_field(u, "id")) else { return };
        let removed = self
            .members
            .get_mut(&guild_id)
            .and_then(|m| m.remove(&user_id))
            .is_some();
        if removed {
            out.push(StateChange::MemberUpdated { guild_id, user_id });
            if Some(user_id) == self.current_user_id() {
                out.push(StateChange::PermissionsChanged { guild_id });
            }
        }
    }

    /// Store a member (and its user) carried by another event (typing,
    /// reactions, member list) unless one is already known. Silent: the
    /// carrying event reports its own change.
    pub(in crate::state) fn hydrate_member(&mut self, guild_id: Snowflake, member: &Value) {
        if !self.guilds.contains_key(&guild_id) {
            return;
        }
        let Some(member) = parse::<Member>(member, "member") else { return };
        let user_id = member.user.id;
        self.users.entry(user_id).or_insert_with(|| member.user.clone());
        self.members.entry(guild_id).or_default().entry(user_id).or_insert(member);
    }
}
