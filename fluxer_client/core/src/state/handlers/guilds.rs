//! Guilds and roles: GUILD_CREATE/SYNC/UPDATE/DELETE, GUILD_ROLE_*.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{id_field, parse, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::{Channel, Guild, GuildProperties, Member, Role};

impl ClientState {
    /// READY guild entry or GUILD_CREATE payload (`GuildReadyData`).
    /// Returns the guild id and whether it is available.
    pub(in crate::state) fn insert_guild_payload(&mut self, g: &Value) -> Option<(Snowflake, bool)> {
        let id = id_field(g, "id")?;
        if g.get("unavailable").and_then(Value::as_bool) == Some(true) {
            self.remove_guild_data(id);
            self.unavailable_guilds.insert(id);
            return Some((id, false));
        }
        let props: GuildProperties = g
            .get("properties")
            .and_then(|p| parse(p, "guild.properties"))
            .unwrap_or_default();
        let roles = parse_list::<Role>(g.get("roles"), "guild.roles")
            .into_iter()
            .map(|r| (r.id, r))
            .collect();
        let large_feature = props.large_feature();
        let guild = Guild {
            id,
            default_message_notifications: props.default_message_notifications,
            large_feature,
            name: props.name,
            icon: props.icon,
            owner_id: props.owner_id,
            mfa_level: props.mfa_level,
            joined_at: g.get("joined_at").and_then(Value::as_str).map(str::to_owned),
            member_count: g.get("member_count").and_then(Value::as_u64),
            visiting: g.get("visiting").and_then(Value::as_bool).unwrap_or(false),
            roles,
        };
        // GUILD_CREATE replaces the guild's channels (threads included) and
        // members wholesale. Channels missing from the payload are deleted
        // with their read state (`removeMissingGuildChannels`); read states
        // and message windows of the remaining channels survive.
        let mut channels = parse_list::<Channel>(g.get("channels"), "guild.channels");
        channels.extend(parse_list::<Channel>(g.get("threads"), "guild.threads"));
        let keep: BTreeSet<Snowflake> = channels.iter().map(|c| c.id).collect();
        self.remove_guild_channels(id, &keep, true);
        self.members.remove(&id);
        self.unavailable_guilds.remove(&id);
        for mut c in channels {
            c.guild_id = Some(id);
            self.denied_channel_access.remove(&c.id);
            self.channels.insert(c.id, c);
        }
        let members = self.members.entry(id).or_default();
        for m in parse_list::<Member>(g.get("members"), "guild.members") {
            self.users.entry(m.user.id).or_insert_with(|| m.user.clone());
            members.insert(m.user.id, m);
        }
        self.guilds.insert(id, guild);
        self.load_guild_emojis(id, g);
        self.load_guild_presences(id, g);
        Some((id, true))
    }

    /// Drop a guild with its channels, members, emojis and member lists.
    /// Read states stay, like the snapshot `GUILD_DELETE` (they are keyed by
    /// channel and cheap; a guild coming back from an outage keeps its
    /// unread markers).
    pub(in crate::state) fn remove_guild_data(&mut self, guild_id: Snowflake) {
        self.guilds.remove(&guild_id);
        self.members.remove(&guild_id);
        self.emojis.remove(&guild_id);
        self.member_lists.remove_guild(guild_id);
        self.remove_guild_channels(guild_id, &BTreeSet::new(), false);
    }

    /// Remove the guild's channels not in `keep`, with their message windows
    /// and typing, and, if `drop_read_states`, their read states.
    fn remove_guild_channels(&mut self, guild_id: Snowflake, keep: &BTreeSet<Snowflake>, drop_read_states: bool) {
        let gone: Vec<Snowflake> = self
            .channels
            .values()
            .filter(|c| c.guild_id == Some(guild_id) && !keep.contains(&c.id))
            .map(|c| c.id)
            .collect();
        for id in gone {
            self.denied_channel_access.remove(&id);
            self.channels.remove(&id);
            if drop_read_states {
                self.read_states.remove(&id);
            }
            self.messages.remove_channel(id);
            self.typing.remove(&id);
        }
    }

    pub(in crate::state) fn apply_guild_create(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        match self.insert_guild_payload(d) {
            Some((guild_id, true)) => out.push(StateChange::GuildUpserted { guild_id }),
            Some((guild_id, false)) => out.push(StateChange::GuildRemoved { guild_id, unavailable: true }),
            None => tracing::debug!("GUILD_CREATE without id"),
        }
    }

    /// GUILD_UPDATE carries the flat wire `Guild`. Roles are kept; the
    /// role events maintain them.
    pub(in crate::state) fn apply_guild_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(id) = id_field(d, "id") else { return };
        let Some(props) = parse::<GuildProperties>(d, "GUILD_UPDATE") else { return };
        let Some(guild) = self.guilds.get_mut(&id) else { return };
        let owner_changed = guild.owner_id != props.owner_id;
        guild.default_message_notifications = props.default_message_notifications;
        guild.large_feature = props.large_feature();
        guild.name = props.name;
        guild.icon = props.icon;
        guild.owner_id = props.owner_id;
        if props.mfa_level.is_some() {
            guild.mfa_level = props.mfa_level;
        }
        if let Some(n) = d.get("member_count").and_then(Value::as_u64) {
            guild.member_count = Some(n);
        }
        out.push(StateChange::GuildUpserted { guild_id: id });
        if owner_changed {
            out.push(StateChange::PermissionsChanged { guild_id: id });
        }
    }

    pub(in crate::state) fn apply_guild_delete(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(id) = id_field(d, "id") else { return };
        let unavailable = d.get("unavailable").and_then(Value::as_bool).unwrap_or(false);
        self.remove_guild_data(id);
        if unavailable {
            self.unavailable_guilds.insert(id);
        } else {
            self.unavailable_guilds.remove(&id);
        }
        out.push(StateChange::GuildRemoved { guild_id: id, unavailable });
    }

    pub(in crate::state) fn apply_role_upsert(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(guild_id) = id_field(d, "guild_id") else { return };
        let Some(role) = d.get("role").and_then(|r| parse::<Role>(r, "role")) else { return };
        let Some(guild) = self.guilds.get_mut(&guild_id) else { return };
        guild.roles.insert(role.id, role);
        out.push(StateChange::PermissionsChanged { guild_id });
    }

    pub(in crate::state) fn apply_role_bulk(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(guild_id) = id_field(d, "guild_id") else { return };
        let roles = parse_list::<Role>(d.get("roles"), "roles");
        let Some(guild) = self.guilds.get_mut(&guild_id) else { return };
        if roles.is_empty() {
            return;
        }
        for role in roles {
            guild.roles.insert(role.id, role);
        }
        out.push(StateChange::PermissionsChanged { guild_id });
    }

    pub(in crate::state) fn apply_role_delete(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(guild_id) = id_field(d, "guild_id") else { return };
        let Some(role_id) = id_field(d, "role_id") else { return };
        let Some(guild) = self.guilds.get_mut(&guild_id) else { return };
        guild.roles.remove(&role_id);
        // TS GuildMembers.handleGuildRoleDelete drops the role from members.
        if let Some(members) = self.members.get_mut(&guild_id) {
            for m in members.values_mut() {
                m.roles.retain(|r| *r != role_id);
            }
        }
        out.push(StateChange::PermissionsChanged { guild_id });
    }
}
