//! Per-channel capability helpers for the current user. Each method mirrors a
//! TS client rule and is cited inline; unknown channels yield `false`.

use super::client::{ClientState, now_ms};
use super::ids::Snowflake;
use super::model::{Channel, ChannelKind, Guild, PermissionOverwrite};
use super::permissions::{bits, can_manage_target_member, compute_permissions, has, highest_role, is_role_higher};

impl ClientState {
    /// `AttachmentPermissionUtils.ts:8-11` and `ChannelMessages.tsx:105`:
    /// DMs and group DMs can always attach files; guild channels need
    /// ATTACH_FILES plus the ability to send (VIEW_CHANNEL + SEND_MESSAGES
    /// and not timed out), per [`Self::can_send_messages_at`].
    pub fn can_attach_files(&self, channel_id: Snowflake) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        if channel.kind.is_private() {
            return true;
        }
        has(self.channel_permissions(channel_id), bits::ATTACH_FILES)
            && self.can_send_messages_at(channel_id, now_ms())
    }

    /// `MessageActionUtils.tsx:222-231`: DMs and group DMs allow reactions by
    /// default; guild channels need ADD_REACTIONS and the current user must
    /// not be timed out.
    pub fn can_add_reactions(&self, channel_id: Snowflake) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        if channel.kind.is_private() {
            return true;
        }
        has(self.channel_permissions(channel_id), bits::ADD_REACTIONS)
            && !self.timed_out_in(channel, now_ms())
    }

    /// `ChannelMessages.tsx:106` and `MessageActionUtils.tsx:238`:
    /// MANAGE_MESSAGES in the channel; private channels never allow it.
    pub fn can_manage_messages(&self, channel_id: Snowflake) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        if channel.kind.is_private() {
            return false;
        }
        has(self.channel_permissions(channel_id), bits::MANAGE_MESSAGES)
    }

    /// `ForwardMessagePreview.tsx:78-81`: private channels embed links freely;
    /// guild channels need EMBED_LINKS.
    pub fn can_embed_links(&self, channel_id: Snowflake) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        if channel.kind.is_private() {
            return true;
        }
        has(self.channel_permissions(channel_id), bits::EMBED_LINKS)
    }

    /// `useLexicalAutocomplete.ts:299-300` with `specialMentions.ts:38`: group
    /// DMs allow @everyone/@here, plain DMs do not; guild channels need
    /// MENTION_EVERYONE.
    pub fn can_mention_everyone(&self, channel_id: Snowflake) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        if channel.kind.is_private() {
            return channel.kind == ChannelKind::GroupDm;
        }
        has(self.channel_permissions(channel_id), bits::MENTION_EVERYONE)
    }

    /// `GuildChannelController`: creating uncategorized channels or categories
    /// requires guild-wide `MANAGE_CHANNELS`. Visiting sessions never qualify
    /// because `VISITOR_MASK` excludes `MANAGE_CHANNELS`.
    pub fn can_manage_guild_channels(&self, guild_id: Snowflake) -> bool {
        has(self.guild_permissions(guild_id), bits::MANAGE_CHANNELS)
    }

    /// Editing or deleting a guild channel/category (or creating inside a
    /// category) requires channel-scoped `MANAGE_CHANNELS`. Private channels and
    /// threads are excluded here.
    pub fn can_manage_channel(&self, channel_id: Snowflake) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        if channel.kind.is_private() || channel.kind.is_thread() || channel.guild_id.is_none() {
            return false;
        }
        has(self.channel_permissions(channel_id), bits::MANAGE_CHANNELS)
    }

    fn allows_elevated_role_management(&self, guild: &Guild) -> bool {
        self.current_user_id().is_some_and(|me| {
            !guild.visiting
                && (guild.owner_id == Some(me)
                    || guild.mfa_level != Some(1)
                    || self.current_user_mfa_enabled == Some(true))
        })
    }

    pub fn can_manage_guild_roles(&self, guild_id: Snowflake) -> bool {
        self.guilds.get(&guild_id).is_some_and(|guild| self.allows_elevated_role_management(guild))
            && has(self.guild_permissions(guild_id), bits::MANAGE_ROLES)
    }

    pub fn can_manage_role(&self, guild_id: Snowflake, role_id: Snowflake) -> bool {
        if !self.can_manage_guild_roles(guild_id) {
            return false;
        }
        let Some(me) = self.current_user_id() else { return false };
        let Some(guild) = self.guilds.get(&guild_id) else { return false };
        let Some(role) = guild.roles.get(&role_id) else { return false };
        let member = self.members.get(&guild_id).and_then(|members| members.get(&me));
        is_role_higher(guild, me, highest_role(guild, member), Some(role))
    }

    pub fn can_manage_channel_permissions(&self, channel_id: Snowflake) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        if channel.kind.is_private() || channel.kind.is_thread() {
            return false;
        }
        channel.guild_id.and_then(|id| self.guilds.get(&id)).is_some_and(|guild| {
            self.allows_elevated_role_management(guild)
                && has(self.channel_permissions(channel_id), bits::VIEW_CHANNEL | bits::MANAGE_ROLES)
        })
    }

    pub fn can_write_role_permissions(&self, guild_id: Snowflake, permissions: u64) -> bool {
        let own = self.guild_permissions(guild_id);
        let own = if has(own, bits::ADMINISTRATOR) { own | bits::THREAD_AWARE_ALL } else { own };
        has(own, permissions)
    }

    pub fn can_write_channel_overwrite(
        &self,
        channel_id: Snowflake,
        before: Option<&PermissionOverwrite>,
        after: Option<&PermissionOverwrite>,
    ) -> bool {
        if !self.can_manage_channel_permissions(channel_id) {
            return false;
        }
        let permissions = self.channel_permissions(channel_id);
        let (old_allow, old_deny) = before.map_or((0, 0), |o| (o.allow, o.deny));
        let (new_allow, new_deny) = after.map_or((0, 0), |o| (o.allow, o.deny));
        let granted = (new_allow & !old_allow) | (old_deny & !new_deny);
        has(permissions, bits::ADMINISTRATOR) || has(permissions, granted)
    }

    pub fn can_sync_channel_parent(&self, channel_id: Snowflake, parent_id: Snowflake) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else { return false };
        let Some(parent) = self.channels.get(&parent_id) else { return false };
        let Some(guild) = channel.guild_id else { return false };
        if channel.kind.is_category() || channel.parent_id == Some(parent_id) || parent.guild_id != Some(guild)
            || !parent.kind.is_category() || !self.can_manage_guild_channels(guild)
            || !self.can_manage_channel_permissions(channel_id) || !self.can_manage_channel(parent_id)
        {
            return false;
        }
        let before = &channel.permission_overwrites;
        let after = &parent.permission_overwrites;
        before.iter().all(|item| {
            self.can_write_channel_overwrite(channel_id, Some(item), after.iter().find(|o| o.id == item.id))
        }) && after.iter().filter(|item| !before.iter().any(|o| o.id == item.id)).all(|item| {
            self.can_write_channel_overwrite(channel_id, None, Some(item))
        })
    }

    pub fn can_kick_members(&self, guild_id: Snowflake) -> bool {
        has(self.guild_permissions(guild_id), bits::KICK_MEMBERS)
    }

    pub fn can_ban_members(&self, guild_id: Snowflake) -> bool {
        has(self.guild_permissions(guild_id), bits::BAN_MEMBERS)
    }

    pub fn can_moderate_members(&self, guild_id: Snowflake) -> bool {
        has(self.guild_permissions(guild_id), bits::MODERATE_MEMBERS)
    }

    pub fn can_manage_nicknames(&self, guild_id: Snowflake) -> bool {
        has(self.guild_permissions(guild_id), bits::MANAGE_NICKNAMES)
    }

    pub fn can_change_own_nickname(&self, guild_id: Snowflake) -> bool {
        has(self.guild_permissions(guild_id), bits::CHANGE_NICKNAME)
    }

    pub fn can_create_invite(&self, channel_id: Snowflake) -> bool {
        has(self.channel_permissions(channel_id), bits::CREATE_INSTANT_INVITE)
    }

    /// True when the current user may kick `target_id` from `guild_id`.
    pub fn can_kick_member(&self, guild_id: Snowflake, target_id: Snowflake) -> bool {
        self.can_kick_members(guild_id) && self.can_manage_guild_member(guild_id, target_id)
    }

    /// True when the current user may ban loaded member `target_id` from `guild_id`.
    /// Requires both `BAN_MEMBERS` and hierarchy superiority over the loaded target
    /// so unloaded members in large guilds do not expose a failing Ban button
    /// (`GuildModerationCapabilityUtils.ts:28-35`, `PermissionUtils.ts:228-237`);
    /// ban-by-ID entry itself is governed by [`Self::can_ban_members`].
    pub fn can_ban_member(&self, guild_id: Snowflake, target_id: Snowflake) -> bool {
        self.can_ban_members(guild_id) && self.can_manage_guild_member(guild_id, target_id)
    }

    /// True when the current user may timeout (or remove timeout from) `target_id`.
    /// Guild administrators cannot be timed out.
    pub fn can_timeout_member(&self, guild_id: Snowflake, target_id: Snowflake) -> bool {
        if !self.can_moderate_members(guild_id) || !self.can_manage_guild_member(guild_id, target_id) {
            return false;
        }
        let Some(guild) = self.guilds.get(&guild_id) else { return false };
        let target_member = self.members.get(&guild_id).and_then(|m| m.get(&target_id));
        !has(compute_permissions(guild, target_id, target_member, None), bits::ADMINISTRATOR)
    }

    /// True when the current user may edit `target_id`'s guild nickname at `now_ms`.
    /// Self edits require `CHANGE_NICKNAME` and not being timed out
    /// (`GuildModerationCapabilityUtils.ts:85-89`).
    pub fn can_edit_member_nickname(&self, guild_id: Snowflake, target_id: Snowflake, now_ms: i64) -> bool {
        let Some(me) = self.current_user_id() else { return false };
        let Some(guild) = self.guilds.get(&guild_id) else { return false };
        if guild.visiting {
            return false;
        }
        if me == target_id {
            let Some(me_member) = self.members.get(&guild_id).and_then(|m| m.get(&me)) else {
                return false;
            };
            return self.can_change_own_nickname(guild_id) && !me_member.is_timed_out(now_ms);
        }
        self.can_manage_nicknames(guild_id) && self.can_manage_guild_member(guild_id, target_id)
    }

    fn can_manage_guild_member(&self, guild_id: Snowflake, target_id: Snowflake) -> bool {
        let Some(me) = self.current_user_id() else { return false };
        let Some(guild) = self.guilds.get(&guild_id) else { return false };
        let members = self.members.get(&guild_id);
        can_manage_target_member(
            guild,
            me,
            members.and_then(|m| m.get(&me)),
            target_id,
            members.and_then(|m| m.get(&target_id)),
        )
    }

    /// Whether the current user is timed out in `channel`'s guild
    /// (`GuildMembers.isUserTimedOut`, `GuildMembers.ts:129-142`).
    fn timed_out_in(&self, channel: &Channel, now_ms: i64) -> bool {
        channel
            .guild_id
            .zip(self.current_user_id())
            .and_then(|(g, me)| self.members.get(&g)?.get(&me))
            .is_some_and(|m| m.is_timed_out(now_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dispatch;
    use serde_json::{Value, json};

    const ME: u64 = 100;
    const OWNER: u64 = 300;
    const GUILD: u64 = 1000; // also the @everyone role id
    const ROLE_ADMIN: u64 = 1003;
    const GENERAL: u64 = 2001;
    const DENIED: u64 = 2002;
    const GRANTED: u64 = 2003;
    const DM: u64 = 3000;
    const GROUP_DM: u64 = 3001;

    fn sf(v: u64) -> Snowflake {
        Snowflake(v)
    }

    fn dispatch(t: &str, d: Value) -> Dispatch {
        Dispatch { t: t.to_owned(), s: 0, d }
    }

    fn user_json(id: u64, name: &str) -> Value {
        json!({"id": id.to_string(), "username": name, "discriminator": "0000", "global_name": null, "avatar": null})
    }

    fn role_json(id: u64, name: &str, permissions: u64) -> Value {
        json!({"id": id.to_string(), "name": name, "position": 0, "permissions": permissions.to_string()})
    }

    fn overwrite(id: u64, kind: u8, allow: u64, deny: u64) -> Value {
        json!({"id": id.to_string(), "type": kind, "allow": allow.to_string(), "deny": deny.to_string()})
    }

    fn channel_json(id: u64, kind: u8, overwrites: Vec<Value>) -> Value {
        json!({"id": id.to_string(), "type": kind, "name": id.to_string(), "position": 0,
               "permission_overwrites": overwrites})
    }

    fn ready_payload() -> Value {
        json!({
            "session_id": "session-a",
            "user": user_json(ME, "me"),
            "users": [user_json(OWNER, "owner")],
            "user_settings": {"guild_folders": [{"guild_ids": [GUILD.to_string()]}]},
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": {"name": "Alpha", "icon": null, "owner_id": OWNER.to_string()},
                "joined_at": "2015-01-01T00:00:00+00:00",
                "member_count": 2,
                "roles": [
                    role_json(GUILD, "@everyone", bits::DEFAULT),
                    role_json(ROLE_ADMIN, "admin", bits::ADMINISTRATOR),
                ],
                "members": [
                    {"user": user_json(ME, "me"), "roles": [], "joined_at": "2015-01-01T00:00:00+00:00"},
                ],
                "channels": [
                    channel_json(GENERAL, 0, vec![]),
                    channel_json(DENIED, 0, vec![
                        overwrite(GUILD, 0, 0, bits::ATTACH_FILES | bits::ADD_REACTIONS | bits::EMBED_LINKS),
                    ]),
                    channel_json(GRANTED, 0, vec![
                        overwrite(GUILD, 0, bits::MANAGE_MESSAGES | bits::MENTION_EVERYONE, 0),
                    ]),
                ],
            }],
            "private_channels": [
                {"id": DM.to_string(), "type": 1, "recipients": [user_json(OWNER, "owner")]},
                {"id": GROUP_DM.to_string(), "type": 3,
                 "recipients": [user_json(OWNER, "owner"), user_json(ME, "me")]},
            ],
            "read_states": [],
        })
    }

    fn ready_state() -> ClientState {
        let mut state = ClientState::new();
        let changes = state.apply(&dispatch("READY", ready_payload()));
        assert_eq!(changes, vec![crate::state::StateChange::Ready]);
        state
    }

    #[test]
    fn role_management_respects_mfa_and_equal_position_hierarchy() {
        let mut payload = ready_payload();
        payload["user"]["mfa_enabled"] = json!(false);
        payload["guilds"][0]["properties"]["mfa_level"] = json!(1);
        payload["guilds"][0]["roles"] = json!([
            role_json(GUILD, "@everyone", bits::DEFAULT),
            {"id": "1001", "name": "higher", "position": 5},
            {"id": "1002", "name": "actor", "position": 5, "permissions": bits::MANAGE_ROLES.to_string()},
            {"id": "1003", "name": "lower", "position": 5}
        ]);
        payload["guilds"][0]["members"][0]["roles"] = json!(["1002"]);
        let mut state = ClientState::new();
        state.apply(&dispatch("READY", payload));

        assert!(!state.can_manage_guild_roles(sf(GUILD)));
        assert!(!state.can_manage_channel_permissions(sf(GENERAL)));
        state.apply(&dispatch("USER_UPDATE", json!({ "id": sf(ME), "mfa_enabled": true })));
        assert!(state.can_manage_guild_roles(sf(GUILD)));
        assert!(state.can_manage_channel_permissions(sf(GENERAL)));
        assert!(!state.can_manage_role(sf(GUILD), sf(1001)));
        assert!(!state.can_manage_role(sf(GUILD), sf(1002)));
        assert!(state.can_manage_role(sf(GUILD), sf(1003)));
        assert!(!state.can_write_role_permissions(sf(GUILD), bits::ADMINISTRATOR));
        state.current_user_mfa_enabled = Some(false);
        state.guilds.get_mut(&sf(GUILD)).unwrap().owner_id = Some(sf(ME));
        assert!(state.can_manage_role(sf(GUILD), sf(1001)));
    }

    #[test]
    fn overwrite_grants_include_removed_denies_and_parent_sync() {
        let mut state = ready_state();
        state.guilds.get_mut(&sf(GUILD)).unwrap().roles.get_mut(&sf(GUILD)).unwrap().permissions
            |= bits::MANAGE_ROLES | bits::MANAGE_CHANNELS;
        state.apply(&dispatch("CHANNEL_CREATE", json!({
            "id": "2105", "guild_id": sf(GUILD), "type": 4,
            "permission_overwrites": [overwrite(GUILD, 0, bits::MANAGE_MESSAGES, 0)]
        })));
        let old = PermissionOverwrite { id: sf(GUILD), kind: 0, allow: 0, deny: bits::MANAGE_MESSAGES };
        assert!(!state.can_write_channel_overwrite(sf(GENERAL), Some(&old), None));
        assert!(state.can_write_channel_overwrite(sf(GENERAL), None, Some(&old)));
        assert!(!state.can_sync_channel_parent(sf(GENERAL), sf(2105)));
        state.guilds.get_mut(&sf(GUILD)).unwrap().owner_id = Some(sf(ME));
        assert!(state.can_sync_channel_parent(sf(GENERAL), sf(2105)));
        assert!(!state.can_manage_channel_permissions(sf(DM)));
    }

    #[test]
    fn confirmed_channel_denial_survives_move_batch_without_discarding_channel() {
        let mut state = ready_state();
        let channel = sf(GENERAL);
        assert!(state.can_view_channel(channel));
        state.set_denied_channel_access(channel, true);
        let mut move_update = dispatch("CHANNEL_UPDATE_BULK", json!({
            "channels": [{ "id": channel, "parent_id": "2105", "permission_overwrites": [] }]
        }));
        move_update.s = 2;
        state.apply(&move_update);
        assert_eq!(state.channels[&channel].parent_id, Some(sf(2105)));
        assert!(!state.can_view_channel(channel));
        let mut confirmed = dispatch("CHANNEL_UPDATE", json!({ "id": channel, "permission_overwrites": [] }));
        confirmed.s = 3;
        state.apply(&confirmed);
        assert!(state.can_view_channel(channel));
    }

    #[test]
    fn guild_channel_default_permissions() {
        let state = ready_state();
        // @everyone DEFAULT grants attach, react and embed but not manage or mention.
        assert!(state.can_attach_files(sf(GENERAL)));
        assert!(state.can_add_reactions(sf(GENERAL)));
        assert!(state.can_embed_links(sf(GENERAL)));
        assert!(!state.can_manage_messages(sf(GENERAL)));
        assert!(!state.can_mention_everyone(sf(GENERAL)));
    }

    #[test]
    fn guild_channel_overwrite_denies_permissions() {
        let state = ready_state();
        assert!(!state.can_attach_files(sf(DENIED)));
        assert!(!state.can_add_reactions(sf(DENIED)));
        assert!(!state.can_embed_links(sf(DENIED)));
    }

    #[test]
    fn guild_channel_overwrite_grants_permissions() {
        let state = ready_state();
        assert!(state.can_manage_messages(sf(GRANTED)));
        assert!(state.can_mention_everyone(sf(GRANTED)));
    }

    #[test]
    fn owner_gets_all_capabilities() {
        let mut state = ready_state();
        state.guilds.get_mut(&sf(GUILD)).unwrap().owner_id = Some(sf(ME));
        assert!(state.can_attach_files(sf(DENIED)));
        assert!(state.can_add_reactions(sf(DENIED)));
        assert!(state.can_embed_links(sf(DENIED)));
        assert!(state.can_manage_messages(sf(DENIED)));
        assert!(state.can_mention_everyone(sf(DENIED)));
    }

    #[test]
    fn administrator_gets_all_capabilities() {
        let mut state = ready_state();
        state.members.get_mut(&sf(GUILD)).unwrap().get_mut(&sf(ME)).unwrap().roles = vec![sf(ROLE_ADMIN)];
        assert!(state.can_attach_files(sf(DENIED)));
        assert!(state.can_add_reactions(sf(DENIED)));
        assert!(state.can_embed_links(sf(DENIED)));
        assert!(state.can_manage_messages(sf(DENIED)));
        assert!(state.can_mention_everyone(sf(DENIED)));
    }

    #[test]
    fn dm_allows_attach_react_and_embed_by_default() {
        let state = ready_state();
        assert!(state.can_attach_files(sf(DM)));
        assert!(state.can_add_reactions(sf(DM)));
        assert!(state.can_embed_links(sf(DM)));
        assert!(!state.can_manage_messages(sf(DM)));
        assert!(!state.can_mention_everyone(sf(DM)));
    }

    #[test]
    fn group_dm_allows_mention_everyone() {
        let state = ready_state();
        assert!(state.can_mention_everyone(sf(GROUP_DM)));
        assert!(state.can_attach_files(sf(GROUP_DM)));
        assert!(state.can_add_reactions(sf(GROUP_DM)));
    }

    #[test]
    fn unknown_channel_returns_false() {
        let state = ready_state();
        assert!(!state.can_attach_files(sf(999_999)));
        assert!(!state.can_add_reactions(sf(999_999)));
        assert!(!state.can_manage_messages(sf(999_999)));
        assert!(!state.can_embed_links(sf(999_999)));
        assert!(!state.can_mention_everyone(sf(999_999)));
    }

    #[test]
    fn timeout_blocks_attach_and_reactions_in_guild_only() {
        let mut state = ready_state();
        state.apply(&dispatch(
            "GUILD_MEMBER_UPDATE",
            json!({"guild_id": GUILD.to_string(), "user": user_json(ME, "me"),
                   "roles": [], "communication_disabled_until": "2999-01-01T00:00:00.000Z"}),
        ));
        // Attach and reactions respect the communication timeout in guilds.
        assert!(!state.can_attach_files(sf(GRANTED)));
        assert!(!state.can_add_reactions(sf(GRANTED)));
        // Embed, manage and mention are not gated by the timeout.
        assert!(state.can_embed_links(sf(GRANTED)));
        assert!(state.can_manage_messages(sf(GRANTED)));
        assert!(state.can_mention_everyone(sf(GRANTED)));
        // DMs stay allowed.
        assert!(state.can_attach_files(sf(DM)));
        assert!(state.can_add_reactions(sf(DM)));
    }

    #[test]
    fn manage_channels_permissions_and_category_scope() {
        let mut state = ready_state();
        assert!(!state.can_manage_guild_channels(sf(GUILD)));
        assert!(!state.can_manage_channel(sf(GENERAL)));

        // Category-scoped overwrite grants MANAGE_CHANNELS only on that category.
        let cat_managed = 2100;
        let cat_other = 2101;
        state.apply(&dispatch(
            "CHANNEL_CREATE",
            json!({
                "id": cat_managed.to_string(),
                "guild_id": GUILD.to_string(),
                "type": 4,
                "name": "managed-cat",
                "position": 0,
                "permission_overwrites": [
                    overwrite(GUILD, 0, bits::VIEW_CHANNEL | bits::MANAGE_CHANNELS, 0)
                ]
            }),
        ));
        state.apply(&dispatch(
            "CHANNEL_CREATE",
            json!({
                "id": cat_other.to_string(),
                "guild_id": GUILD.to_string(),
                "type": 4,
                "name": "other-cat",
                "position": 1,
                "permission_overwrites": []
            }),
        ));
        assert!(!state.can_manage_guild_channels(sf(GUILD)));
        assert!(state.can_manage_channel(sf(cat_managed)));
        assert!(!state.can_manage_channel(sf(cat_other)));
        assert!(!state.can_manage_channel(sf(GENERAL)));

        // Guild-wide MANAGE_CHANNELS allows root and categories unless denied on a category.
        let cat_denied = 2102;
        state.apply(&dispatch(
            "GUILD_ROLE_UPDATE",
            json!({
                "guild_id": GUILD.to_string(),
                "role": role_json(GUILD, "@everyone", bits::DEFAULT | bits::MANAGE_CHANNELS),
            }),
        ));
        state.apply(&dispatch(
            "CHANNEL_CREATE",
            json!({
                "id": cat_denied.to_string(),
                "guild_id": GUILD.to_string(),
                "type": 4,
                "name": "locked-cat",
                "position": 2,
                "permission_overwrites": [overwrite(GUILD, 0, 0, bits::MANAGE_CHANNELS)]
            }),
        ));
        assert!(state.can_manage_guild_channels(sf(GUILD)));
        assert!(state.can_manage_channel(sf(cat_other)));
        assert!(!state.can_manage_channel(sf(cat_denied)));

        // Administrator / owner bypass denies.
        state.members.get_mut(&sf(GUILD)).unwrap().get_mut(&sf(ME)).unwrap().roles = vec![sf(ROLE_ADMIN)];
        assert!(state.can_manage_channel(sf(cat_denied)));
        state.members.get_mut(&sf(GUILD)).unwrap().get_mut(&sf(ME)).unwrap().roles = Vec::new();
        state.guilds.get_mut(&sf(GUILD)).unwrap().owner_id = Some(sf(ME));
        assert!(state.can_manage_guild_channels(sf(GUILD)));
        assert!(state.can_manage_channel(sf(cat_denied)));
    }

    #[test]
    fn visiting_dm_and_thread_cannot_manage_channels() {
        let mut state = ready_state();
        state.guilds.get_mut(&sf(GUILD)).unwrap().owner_id = Some(sf(ME));
        state.apply(&dispatch(
            "CHANNEL_CREATE",
            json!({"id": "2200", "guild_id": GUILD.to_string(), "type": 11, "parent_id": GENERAL.to_string()}),
        ));
        assert!(!state.can_manage_channel(sf(DM)));
        assert!(!state.can_manage_channel(sf(GROUP_DM)));
        assert!(!state.can_manage_channel(sf(2200)));
        assert!(!state.can_manage_channel(sf(999_999)));

        state.guilds.get_mut(&sf(GUILD)).unwrap().visiting = true;
        assert!(!state.can_manage_guild_channels(sf(GUILD)));
        assert!(!state.can_manage_channel(sf(GENERAL)));
    }

    #[test]
    fn empty_category_visibility_follows_manage_permission() {
        let mut state = ready_state();
        let empty_cat = 2300;
        state.apply(&dispatch(
            "CHANNEL_CREATE",
            json!({"id": empty_cat.to_string(), "guild_id": GUILD.to_string(), "type": 4, "name": "Empty", "position": 9}),
        ));
        assert!(state.channel_tree(sf(GUILD)).iter().all(|n| n.channel.id != sf(empty_cat)));

        state.apply(&dispatch(
            "CHANNEL_UPDATE",
            json!({
                "id": empty_cat.to_string(),
                "permission_overwrites": [overwrite(GUILD, 0, bits::VIEW_CHANNEL | bits::MANAGE_CHANNELS, 0)]
            }),
        ));
        assert!(state.channel_tree(sf(GUILD)).iter().any(|n| n.channel.id == sf(empty_cat)));
    }

    #[test]
    fn member_moderation_hierarchy_and_timeout_protection() {
        let mut state = ready_state();
        let mod_user = 401;
        let peer_mod = 402;
        let low_user = 403;
        let admin_user = 404;
        let non_member = 405;
        let role_mod_high = 1010;
        let role_mod_tie_lower_id = 1009; // same position as role_mod_high -> lower id wins
        let role_low = 1011;

        for r in [
            json!({
                "id": role_mod_high.to_string(),
                "name": "mod-high",
                "position": 5,
                "permissions": (bits::KICK_MEMBERS | bits::BAN_MEMBERS | bits::MODERATE_MEMBERS | bits::MANAGE_NICKNAMES).to_string()
            }),
            json!({
                "id": role_mod_tie_lower_id.to_string(),
                "name": "mod-tie",
                "position": 5,
                "permissions": (bits::KICK_MEMBERS | bits::BAN_MEMBERS).to_string()
            }),
            json!({
                "id": role_low.to_string(),
                "name": "low",
                "position": 1,
                "permissions": "0"
            }),
        ] {
            state.apply(&dispatch("GUILD_ROLE_CREATE", json!({ "guild_id": GUILD.to_string(), "role": r })));
        }
        for (uid, roles) in [
            (mod_user, vec![role_mod_high]),
            (peer_mod, vec![role_mod_tie_lower_id]),
            (low_user, vec![role_low]),
            (admin_user, vec![ROLE_ADMIN]),
            (OWNER, vec![]),
        ] {
            let role_strs: Vec<String> = roles.into_iter().map(|id| id.to_string()).collect();
            state.apply(&dispatch(
                "GUILD_MEMBER_ADD",
                json!({
                    "guild_id": GUILD.to_string(),
                    "user": user_json(uid, "u"),
                    "roles": role_strs,
                    "joined_at": "2020-01-01T00:00:00+00:00"
                }),
            ));
        }

        // Plain ME cannot moderate others, can change own nick via DEFAULT permissions.
        let now_ms = 1_000_000_i64;
        assert!(!state.can_kick_member(sf(GUILD), sf(low_user)));
        assert!(!state.can_ban_member(sf(GUILD), sf(low_user)));
        assert!(!state.can_timeout_member(sf(GUILD), sf(low_user)));
        assert!(state.can_edit_member_nickname(sf(GUILD), sf(ME), now_ms));
        assert!(!state.can_edit_member_nickname(sf(GUILD), sf(low_user), now_ms));

        // ME gets mod_high (pos 5, id 1010): beats low_user (pos 1), loses tie to peer_mod (pos 5, id 1009).
        state.members.get_mut(&sf(GUILD)).unwrap().get_mut(&sf(ME)).unwrap().roles = vec![sf(role_mod_high)];
        assert!(state.can_kick_member(sf(GUILD), sf(low_user)));
        assert!(state.can_ban_member(sf(GUILD), sf(low_user)));
        assert!(state.can_timeout_member(sf(GUILD), sf(low_user)));
        assert!(state.can_edit_member_nickname(sf(GUILD), sf(low_user), now_ms));
        // Tie-breaker & owner & self protection.
        assert!(!state.can_kick_member(sf(GUILD), sf(peer_mod)));
        assert!(!state.can_ban_member(sf(GUILD), sf(peer_mod)));
        assert!(!state.can_kick_member(sf(GUILD), sf(OWNER)));
        assert!(!state.can_ban_member(sf(GUILD), sf(OWNER)));
        assert!(!state.can_kick_member(sf(GUILD), sf(ME)));
        assert!(!state.can_ban_member(sf(GUILD), sf(ME)));
        assert!(!state.can_timeout_member(sf(GUILD), sf(ME)));
        // Unloaded/unknown targets do not expose can_ban_member (can_ban_members still gates Bans entry).
        assert!(state.can_ban_members(sf(GUILD)));
        assert!(!state.can_ban_member(sf(GUILD), sf(non_member)));
        assert!(!state.can_kick_member(sf(GUILD), sf(non_member)));
        assert!(!state.can_timeout_member(sf(GUILD), sf(non_member)));
        assert!(!state.can_edit_member_nickname(sf(GUILD), sf(non_member), now_ms));

        // MANAGE_NICKNAMES without CHANGE_NICKNAME does not allow self nickname edits;
        // timed-out self also cannot change own nickname even with CHANGE_NICKNAME.
        {
            let everyone = state.guilds.get_mut(&sf(GUILD)).unwrap().roles.get_mut(&sf(GUILD)).unwrap();
            everyone.permissions &= !bits::CHANGE_NICKNAME;
        }
        assert!(!state.can_edit_member_nickname(sf(GUILD), sf(ME), now_ms));
        {
            let everyone = state.guilds.get_mut(&sf(GUILD)).unwrap().roles.get_mut(&sf(GUILD)).unwrap();
            everyone.permissions |= bits::CHANGE_NICKNAME;
        }
        assert!(state.can_edit_member_nickname(sf(GUILD), sf(ME), now_ms));
        state
            .members
            .get_mut(&sf(GUILD))
            .unwrap()
            .get_mut(&sf(ME))
            .unwrap()
            .communication_disabled_until = Some("2099-01-01T00:00:00Z".to_owned());
        assert!(!state.can_edit_member_nickname(sf(GUILD), sf(ME), now_ms));
        state
            .members
            .get_mut(&sf(GUILD))
            .unwrap()
            .get_mut(&sf(ME))
            .unwrap()
            .communication_disabled_until = None;

        // Even as owner, cannot timeout an administrator or self, and visiting blocks all.
        state.guilds.get_mut(&sf(GUILD)).unwrap().owner_id = Some(sf(ME));
        assert!(state.can_kick_member(sf(GUILD), sf(admin_user)));
        assert!(state.can_ban_member(sf(GUILD), sf(admin_user)));
        assert!(!state.can_timeout_member(sf(GUILD), sf(admin_user)));
        assert!(state.can_edit_member_nickname(sf(GUILD), sf(admin_user), now_ms));
        assert!(!state.can_kick_member(sf(GUILD), sf(ME)));

        state.guilds.get_mut(&sf(GUILD)).unwrap().visiting = true;
        assert!(!state.can_kick_member(sf(GUILD), sf(low_user)));
        assert!(!state.can_ban_member(sf(GUILD), sf(low_user)));
        assert!(!state.can_edit_member_nickname(sf(GUILD), sf(ME), now_ms));
    }
}
