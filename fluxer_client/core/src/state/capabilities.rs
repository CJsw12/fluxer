//! Per-channel capability helpers for the current user. Each method mirrors a
//! TS client rule and is cited inline; unknown channels yield `false`.

use super::client::{ClientState, now_ms};
use super::ids::Snowflake;
use super::model::{Channel, ChannelKind};
use super::permissions::{bits, has};

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
}
