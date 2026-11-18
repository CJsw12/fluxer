//! Lazy member lists: GUILD_MEMBER_LIST_UPDATE, the answer to a
//! LAZY_REQUEST (op 14, see `gateway::member_list_subscription`)
//! (`GuildMemberListUpdate.ts`, `MemberSidebar.ts`,
//! `fluxer_gateway/src/guild/guild_member_list_read.erl`).
//!
//! A list is identified by the server's list `id` (one list per permission
//! set); updates sent for a subscription carry `channel_id`, which maps the
//! channel to its list. Rows are sparse: only subscribed ranges are filled.
//!
//! The server currently sends only SYNC (and TS only handles SYNC); INSERT,
//! UPDATE, DELETE and INVALIDATE follow the usual lazy-list semantics so a
//! server that starts sending them is handled. Not ported: the TS 1 s update
//! batching, boot-state seeding and per-list custom-status bookkeeping.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::presence::{CustomStatus, Status};
use super::{id_field, parse};
use crate::gateway::{MEMBER_LIST_RANGE_MAX_END, MEMBER_LIST_RANGE_MAX_SPAN};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::{Member, null_default};

/// A list section header: a hoisted role id, `online` or `offline`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberListGroup {
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub count: u32,
}

/// The presence carried on a member row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListPresence {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub mobile: Option<bool>,
    #[serde(default)]
    pub custom_status: Option<CustomStatus>,
}

impl ListPresence {
    pub fn status(&self) -> Status {
        Status::parse(self.status.as_deref())
    }
}

/// One member row: the member (user, nick, roles) and its presence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberListEntry {
    pub member: Member,
    pub presence: Option<ListPresence>,
}

/// A row of the list: a group header or a member.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum MemberListRow {
    Group(MemberListGroup),
    Member(Box<MemberListEntry>),
}

/// One lazy member list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberList {
    pub guild_id: Snowflake,
    pub list_id: String,
    /// The last channel the server attached to an update of this list.
    pub channel_id: Option<Snowflake>,
    pub member_count: u32,
    pub online_count: u32,
    pub groups: Vec<MemberListGroup>,
    /// Row index -> row; only synced ranges are present.
    pub rows: BTreeMap<u32, MemberListRow>,
}

impl MemberList {
    fn new(guild_id: Snowflake, list_id: String) -> Self {
        Self {
            guild_id,
            list_id,
            channel_id: None,
            member_count: 0,
            online_count: 0,
            groups: Vec::new(),
            rows: BTreeMap::new(),
        }
    }

    /// Total rows for the scroll height: one header per non-empty group
    /// plus its members (`getTotalRowsFromLayout`), else `member_count`.
    pub fn row_count(&self) -> u32 {
        if self.groups.is_empty() {
            return self.member_count;
        }
        self.groups
            .iter()
            .filter(|g| g.count > 0)
            .fold(0u32, |total, g| total.saturating_add(g.count).saturating_add(1))
    }

    pub fn row(&self, index: u32) -> Option<&MemberListRow> {
        self.rows.get(&index)
    }

    /// Insert at `index`, shifting later rows down by one.
    fn insert_row(&mut self, index: u32, row: MemberListRow) {
        let tail = self.rows.split_off(&index);
        for (i, r) in tail {
            self.rows.insert(i.saturating_add(1), r);
        }
        self.rows.insert(index, row);
    }

    /// Remove `index`, shifting later rows up by one.
    fn delete_row(&mut self, index: u32) {
        let tail = self.rows.split_off(&index);
        for (i, r) in tail {
            if i != index {
                self.rows.insert(i - 1, r);
            }
        }
    }
}

/// All member lists of the session.
#[derive(Debug, Clone, Default)]
pub struct MemberLists {
    lists: BTreeMap<(Snowflake, String), MemberList>,
    /// (guild, channel) -> list id, from updates that carry `channel_id`.
    channels: BTreeMap<(Snowflake, Snowflake), String>,
}

impl MemberLists {
    pub fn get(&self, guild_id: Snowflake, list_id: &str) -> Option<&MemberList> {
        self.lists.get(&(guild_id, list_id.to_owned()))
    }

    /// The list shown for a channel, once the server answered a
    /// subscription for it.
    pub fn for_channel(&self, guild_id: Snowflake, channel_id: Snowflake) -> Option<&MemberList> {
        let list_id = self.channels.get(&(guild_id, channel_id))?;
        self.get(guild_id, list_id)
    }

    pub(in crate::state) fn remove_guild(&mut self, guild_id: Snowflake) {
        self.lists.retain(|(g, _), _| *g != guild_id);
        self.channels.retain(|(g, _), _| *g != guild_id);
    }
}

/// `isValidRange` (`GuildMemberListUpdate.ts`).
fn valid_range(range: Option<&Value>) -> Option<(u32, u32)> {
    let items = range?.as_array()?;
    let [start, end] = items.as_slice() else { return None };
    let start = u32::try_from(start.as_u64()?).ok()?;
    let end = u32::try_from(end.as_u64()?).ok()?;
    (end >= start && end <= MEMBER_LIST_RANGE_MAX_END && end - start <= MEMBER_LIST_RANGE_MAX_SPAN)
        .then_some((start, end))
}

fn op_index(op: &Value) -> Option<u32> {
    u32::try_from(op.get("index")?.as_u64()?)
        .ok()
        .filter(|i| *i <= MEMBER_LIST_RANGE_MAX_END)
}

/// A parsed list item, or `None` when it is malformed.
enum Item {
    Row(MemberListRow),
    /// A group with no members: not shown and takes no index (`isEmptyGroupItem`).
    EmptyGroup,
}

impl ClientState {
    /// `{guild_id, id, channel_id?, member_count, online_count, groups, ops}`.
    pub(in crate::state) fn apply_member_list_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(guild_id) = id_field(d, "guild_id") else { return };
        let Some(list_id) = d.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) else { return };
        let Some(ops) = d.get("ops").and_then(Value::as_array) else { return };
        if !self.guilds.contains_key(&guild_id) {
            return;
        }
        let channel_id = id_field(d, "channel_id");
        let key = (guild_id, list_id.to_owned());
        let mut list = self
            .member_lists
            .lists
            .remove(&key)
            .unwrap_or_else(|| MemberList::new(guild_id, list_id.to_owned()));
        let mut applied = 0usize;
        for op in ops {
            if self.apply_member_list_op(guild_id, &mut list, op) {
                applied += 1;
            }
        }
        // TS drops the whole update when it had ops and none was valid.
        if applied == 0 && !ops.is_empty() {
            self.member_lists.lists.insert(key, list);
            return;
        }
        let count = |k: &str| d.get(k).and_then(Value::as_u64).map(|n| u32::try_from(n).unwrap_or(u32::MAX));
        if let Some(n) = count("member_count") {
            list.member_count = n;
        }
        if let Some(n) = count("online_count") {
            list.online_count = n;
        }
        if let Some(groups) = d.get("groups").and_then(|g| parse::<Vec<MemberListGroup>>(g, "member list groups")) {
            list.groups = groups;
        }
        if !list.groups.is_empty() {
            let total = list.row_count();
            list.rows.retain(|i, _| *i < total);
        }
        if let Some(channel) = channel_id {
            list.channel_id = Some(channel);
            self.member_lists.channels.insert((guild_id, channel), list_id.to_owned());
        }
        let list_channel = list.channel_id;
        self.member_lists.lists.insert(key, list);
        out.push(StateChange::MemberListChanged {
            guild_id,
            list_id: list_id.to_owned(),
            channel_id: list_channel,
        });
    }

    /// Returns true if `op` was valid and applied.
    fn apply_member_list_op(&mut self, guild_id: Snowflake, list: &mut MemberList, op: &Value) -> bool {
        match op.get("op").and_then(Value::as_str) {
            Some("SYNC") => {
                let Some((start, end)) = valid_range(op.get("range")) else { return false };
                let Some(items) = op.get("items").and_then(Value::as_array) else { return false };
                list.rows.retain(|i, _| *i < start || *i > end);
                let mut index = start;
                for item in items {
                    match self.member_list_item(guild_id, item) {
                        Some(Item::EmptyGroup) => continue,
                        Some(Item::Row(row)) => {
                            list.rows.insert(index, row);
                        }
                        None => {}
                    }
                    index = index.saturating_add(1);
                }
                true
            }
            Some("INVALIDATE") => {
                let Some((start, end)) = valid_range(op.get("range")) else { return false };
                list.rows.retain(|i, _| *i < start || *i > end);
                true
            }
            Some("INSERT") => {
                let Some(index) = op_index(op) else { return false };
                match op.get("item").and_then(|item| self.member_list_item(guild_id, item)) {
                    Some(Item::Row(row)) => {
                        list.insert_row(index, row);
                        true
                    }
                    _ => false,
                }
            }
            Some("UPDATE") => {
                let Some(index) = op_index(op) else { return false };
                match op.get("item").and_then(|item| self.member_list_item(guild_id, item)) {
                    Some(Item::Row(row)) => {
                        list.rows.insert(index, row);
                        true
                    }
                    _ => false,
                }
            }
            Some("DELETE") => {
                let Some(index) = op_index(op) else { return false };
                list.delete_row(index);
                true
            }
            _ => false,
        }
    }

    /// `{group: {id, count}}` or `{member: {...GuildMemberData, presence?}}`.
    /// Members are hydrated into `members`/`users` if missing.
    fn member_list_item(&mut self, guild_id: Snowflake, item: &Value) -> Option<Item> {
        if let Some(group) = item.get("group") {
            let group = parse::<MemberListGroup>(group, "member list group")?;
            return Some(if group.count == 0 { Item::EmptyGroup } else { Item::Row(MemberListRow::Group(group)) });
        }
        let raw = item.get("member")?;
        let member = parse::<Member>(raw, "member list member")?;
        let presence = raw
            .get("presence")
            .filter(|p| !p.is_null())
            .and_then(|p| parse::<ListPresence>(p, "member list presence"));
        self.hydrate_member(guild_id, raw);
        Some(Item::Row(MemberListRow::Member(Box::new(MemberListEntry { member, presence }))))
    }

    /// The member list for a guild channel, if the server sent one.
    pub fn member_list(&self, guild_id: Snowflake, channel_id: Snowflake) -> Option<&MemberList> {
        self.member_lists.for_channel(guild_id, channel_id)
    }
}
