//! State module tests: permissions against `PermissionUtils.computePermissions`,
//! message windows (paging, trimming, LRU, nonce reconciliation), unread
//! tracking and a JSON snapshot of the state after a fixture dispatch sequence.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;
use crate::Dispatch;

mod handlers;

// ---- fixture ids ----------------------------------------------------------

const ME: u64 = 100;
const OTHER: u64 = 200;
const OWNER: u64 = 300;

const GUILD: u64 = 1000; // also the @everyone role id
const ROLE_MOD: u64 = 1001;
const ROLE_HELPER: u64 = 1002;
const ROLE_ADMIN: u64 = 1003;

const CATEGORY: u64 = 2000;
const GENERAL: u64 = 2001;
const STAFF: u64 = 2002;
const ANNOUNCE: u64 = 2003;
const VOICE: u64 = 2004;
const EMPTY_CATEGORY: u64 = 2005;
const DM: u64 = 3000;

fn sf(v: u64) -> Snowflake {
    Snowflake(v)
}

/// Message ids with real timestamps (2025-06-15 + n seconds).
fn mid(n: u64) -> Snowflake {
    Snowflake::from_unix_ms(1_750_000_000_000 + n * 1000)
}

fn dispatch(t: &str, d: Value) -> Dispatch {
    Dispatch { t: t.to_owned(), s: 0, d }
}

fn user_json(id: u64, name: &str) -> Value {
    json!({"id": id.to_string(), "username": name, "discriminator": "0000", "global_name": null, "avatar": null})
}

fn message_json(id: Snowflake, channel: u64, author: u64, content: &str) -> Value {
    json!({
        "id": id.to_string(),
        "channel_id": channel.to_string(),
        "author": user_json(author, "u"),
        "content": content,
        "timestamp": "2025-06-15T00:00:00+00:00",
        "type": 0,
        "mentions": [],
        "mention_roles": [],
        "attachments": [],
        "embeds": [],
    })
}

fn message(id: Snowflake, channel: u64, author: u64, content: &str) -> Message {
    serde_json::from_value(message_json(id, channel, author, content)).unwrap()
}

/// A newest-first REST page of message ids `from..to` (oldest `from`).
fn page(channel: u64, from: u64, to: u64) -> Vec<Message> {
    (from..to).rev().map(|n| message(mid(n), channel, OTHER, &format!("m{n}"))).collect()
}

fn overwrite(id: u64, kind: u8, allow: u64, deny: u64) -> Value {
    json!({"id": id.to_string(), "type": kind, "allow": allow.to_string(), "deny": deny.to_string()})
}

fn role_json(id: u64, name: &str, position: i32, permissions: u64) -> Value {
    json!({"id": id.to_string(), "name": name, "position": position, "permissions": permissions.to_string()})
}

fn ready_payload() -> Value {
    let everyone = bits::DEFAULT;
    json!({
        "session_id": "session-a",
        "user": user_json(ME, "me"),
        "users": [user_json(OTHER, "other")],
        "user_settings": {"guild_folders": [{"guild_ids": ["5000", GUILD.to_string()]}]},
        "guilds": [
            {
                "id": GUILD.to_string(),
                "properties": {"name": "Alpha", "icon": null, "owner_id": OWNER.to_string()},
                "joined_at": "2015-01-01T00:00:00+00:00",
                "member_count": 3,
                "roles": [
                    role_json(GUILD, "@everyone", 0, everyone),
                    role_json(ROLE_MOD, "mod", 2, bits::MANAGE_MESSAGES),
                    role_json(ROLE_HELPER, "helper", 1, 0),
                    role_json(ROLE_ADMIN, "admin", 3, bits::ADMINISTRATOR),
                ],
                "members": [
                    {"user": user_json(ME, "me"), "roles": [ROLE_HELPER.to_string()], "joined_at": "2015-01-01T00:00:00+00:00"},
                    {"user": user_json(OTHER, "other"), "roles": [], "joined_at": "2015-01-01T00:00:00+00:00"},
                ],
                "channels": [
                    {"id": CATEGORY.to_string(), "type": 4, "name": "Text", "position": 0},
                    {"id": GENERAL.to_string(), "type": 0, "name": "general", "position": 1, "parent_id": CATEGORY.to_string(),
                     "last_message_id": mid(10).to_string()},
                    {"id": STAFF.to_string(), "type": 0, "name": "staff", "position": 0, "parent_id": CATEGORY.to_string(),
                     "last_message_id": mid(5).to_string(),
                     "permission_overwrites": [
                        overwrite(GUILD, 0, 0, bits::VIEW_CHANNEL),
                        overwrite(ROLE_MOD, 0, bits::VIEW_CHANNEL, 0),
                     ]},
                    {"id": ANNOUNCE.to_string(), "type": 5, "name": "announcements", "position": 2,
                     "last_message_id": mid(3).to_string(),
                     "permission_overwrites": [overwrite(GUILD, 0, 0, bits::SEND_MESSAGES)]},
                    {"id": VOICE.to_string(), "type": 2, "name": "Voice", "position": 3},
                    {"id": EMPTY_CATEGORY.to_string(), "type": 4, "name": "Empty", "position": 5},
                ],
            },
            {"id": "5000", "unavailable": true},
            {
                "id": "6000",
                "properties": {"name": "beta", "owner_id": ME.to_string()},
                "joined_at": "2015-01-01T00:00:00+00:00",
                "roles": [role_json(6000, "@everyone", 0, everyone)],
                "members": [],
                "channels": [{"id": "6001", "type": 0, "name": "lobby", "position": 0}],
            },
        ],
        "private_channels": [
            {"id": DM.to_string(), "type": 1, "recipients": [user_json(OTHER, "other")],
             "last_message_id": mid(20).to_string()},
        ],
        "read_states": [
            {"id": GENERAL.to_string(), "last_message_id": mid(8).to_string(), "mention_count": 1, "version": "5"},
            {"id": ANNOUNCE.to_string(), "last_message_id": mid(3).to_string(), "mention_count": 0},
            {"id": DM.to_string(), "last_message_id": mid(20).to_string(), "mention_count": 0},
        ],
    })
}

fn ready_state() -> ClientState {
    let mut state = ClientState::new();
    let changes = state.apply(&dispatch("READY", ready_payload()));
    assert_eq!(changes, vec![StateChange::Ready]);
    state
}

// ---- permissions: pure compute_permissions --------------------------------

fn guild(owner: Option<u64>, roles: &[(u64, u64)]) -> Guild {
    Guild {
        id: sf(GUILD),
        name: "g".into(),
        icon: None,
        owner_id: owner.map(sf),
        mfa_level: None,
        joined_at: None,
        member_count: None,
        visiting: false,
        default_message_notifications: 0,
        large_feature: false,
        roles: roles
            .iter()
            .map(|&(id, permissions)| {
                (
                    sf(id),
                    Role {
                        id: sf(id),
                        name: String::new(),
                        color: 0,
                        position: 0,
                        permissions,
                        hoist: false,
                        mentionable: false,
                    },
                )
            })
            .collect(),
    }
}

fn member(user: u64, roles: &[u64]) -> Member {
    serde_json::from_value(json!({
        "user": user_json(user, "x"),
        "roles": roles.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
    }))
    .unwrap()
}

fn channel(overwrites: Vec<Value>) -> Channel {
    serde_json::from_value(json!({
        "id": GENERAL.to_string(), "guild_id": GUILD.to_string(), "type": 0,
        "permission_overwrites": overwrites,
    }))
    .unwrap()
}

#[test]
fn perm_owner_gets_all_despite_deny_overwrites() {
    let g = guild(Some(ME), &[(GUILD, 0)]);
    let c = channel(vec![overwrite(GUILD, 0, 0, bits::ALL), overwrite(ME, 1, 0, bits::ALL)]);
    assert_eq!(compute_permissions(&g, sf(ME), None, Some(&c)), bits::ALL);
}

#[test]
fn perm_administrator_role_bypasses_overwrites() {
    let g = guild(None, &[(GUILD, bits::VIEW_CHANNEL), (ROLE_ADMIN, bits::ADMINISTRATOR)]);
    let m = member(ME, &[ROLE_ADMIN]);
    let c = channel(vec![overwrite(ME, 1, 0, bits::VIEW_CHANNEL)]);
    assert_eq!(compute_permissions(&g, sf(ME), Some(&m), Some(&c)), bits::ALL);
}

#[test]
fn perm_administrator_on_everyone_role_counts() {
    let g = guild(None, &[(GUILD, bits::ADMINISTRATOR)]);
    assert_eq!(compute_permissions(&g, sf(ME), None, None), bits::ALL);
}

#[test]
fn perm_missing_everyone_role_falls_back_to_default() {
    let g = guild(None, &[(ROLE_MOD, bits::MANAGE_MESSAGES)]);
    let m = member(ME, &[ROLE_MOD]);
    assert_eq!(compute_permissions(&g, sf(ME), Some(&m), None), bits::DEFAULT | bits::MANAGE_MESSAGES);
}

#[test]
fn perm_unknown_member_roles_are_ignored() {
    let g = guild(None, &[(GUILD, bits::VIEW_CHANNEL)]);
    let m = member(ME, &[9999]);
    assert_eq!(compute_permissions(&g, sf(ME), Some(&m), None), bits::VIEW_CHANNEL);
}

#[test]
fn perm_overwrite_order_everyone_roles_member() {
    let base = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    let g = guild(None, &[(GUILD, base), (ROLE_MOD, 0), (ROLE_HELPER, 0)]);
    let m = member(ME, &[ROLE_MOD, ROLE_HELPER]);

    // @everyone denies SEND, a role allows it back.
    let c = channel(vec![
        overwrite(GUILD, 0, 0, bits::SEND_MESSAGES),
        overwrite(ROLE_MOD, 0, bits::SEND_MESSAGES, 0),
    ]);
    assert_eq!(compute_permissions(&g, sf(ME), Some(&m), Some(&c)), base);

    // Role allow and deny of the same bit: TS applies the summed deny first,
    // then the summed allow, so allow wins.
    let c = channel(vec![
        overwrite(ROLE_MOD, 0, 0, bits::SEND_MESSAGES),
        overwrite(ROLE_HELPER, 0, bits::SEND_MESSAGES, 0),
    ]);
    assert_eq!(compute_permissions(&g, sf(ME), Some(&m), Some(&c)), base);

    // Member deny beats role allow.
    let c = channel(vec![
        overwrite(ROLE_MOD, 0, bits::MANAGE_MESSAGES, 0),
        overwrite(ME, 1, 0, bits::SEND_MESSAGES),
    ]);
    assert_eq!(
        compute_permissions(&g, sf(ME), Some(&m), Some(&c)),
        (base & !bits::SEND_MESSAGES) | bits::MANAGE_MESSAGES
    );

    // @everyone allow adds a bit the roles lack.
    let c = channel(vec![overwrite(GUILD, 0, bits::ATTACH_FILES, bits::VIEW_CHANNEL)]);
    assert_eq!(
        compute_permissions(&g, sf(ME), Some(&m), Some(&c)),
        (base & !bits::VIEW_CHANNEL) | bits::ATTACH_FILES
    );
}

#[test]
fn perm_role_and_member_overwrites_need_a_member() {
    // TS applies role and member overwrites only when the member is known.
    let g = guild(None, &[(GUILD, bits::VIEW_CHANNEL)]);
    let c = channel(vec![overwrite(ME, 1, bits::SEND_MESSAGES, 0)]);
    assert_eq!(compute_permissions(&g, sf(ME), None, Some(&c)), bits::VIEW_CHANNEL);
    let m = member(ME, &[]);
    assert_eq!(
        compute_permissions(&g, sf(ME), Some(&m), Some(&c)),
        bits::VIEW_CHANNEL | bits::SEND_MESSAGES
    );
}

#[test]
fn perm_visitor_mask_strips_admin_and_applies_everyone_overwrite() {
    let mut g = guild(Some(ME), &[(GUILD, bits::ADMINISTRATOR | bits::VIEW_CHANNEL | bits::SEND_MESSAGES)]);
    g.visiting = true;
    // Visiting wins over ownership; ADMINISTRATOR is removed, then masked.
    assert_eq!(compute_permissions(&g, sf(ME), None, None), bits::VIEW_CHANNEL);
    let c = channel(vec![overwrite(GUILD, 0, bits::READ_MESSAGE_HISTORY, bits::VIEW_CHANNEL)]);
    assert_eq!(compute_permissions(&g, sf(ME), None, Some(&c)), bits::READ_MESSAGE_HISTORY);
}

#[test]
fn perm_channel_of_another_guild_is_zero() {
    let g = guild(Some(ME), &[(GUILD, bits::ALL)]);
    let mut c = channel(vec![]);
    c.guild_id = Some(sf(42));
    assert_eq!(compute_permissions(&g, sf(ME), None, Some(&c)), 0);
    c.guild_id = None;
    assert_eq!(compute_permissions(&g, sf(ME), None, Some(&c)), 0);
}

#[test]
fn perm_thread_view_permissions() {
    let parent = bits::VIEW_CHANNEL | bits::SEND_MESSAGES;
    assert_eq!(thread_view_permissions(parent), bits::VIEW_CHANNEL);
    assert_eq!(
        thread_view_permissions(bits::VIEW_CHANNEL | bits::SEND_MESSAGES_IN_THREADS),
        bits::VIEW_CHANNEL | bits::SEND_MESSAGES_IN_THREADS | bits::SEND_MESSAGES
    );
    assert_eq!(thread_view_permissions(bits::ALL), bits::THREAD_AWARE_ALL);
}

#[test]
fn perm_bit_constants_match_ts() {
    // 37 entries in TS `Permissions` (bits 0-18, 20-30, 37, 40, 43, 51-54).
    assert_eq!(bits::ALL.count_ones(), 37);
    assert_eq!(bits::DEFAULT.count_ones(), 15);
    assert_eq!(bits::VIEW_CHANNEL_MEMBERS, 1 << 54);
    assert_eq!(bits::ALL & bits::SEND_MESSAGES_IN_THREADS, 0);
}

// ---- permissions through ClientState ---------------------------------------

#[test]
fn state_permissions_view_and_send() {
    let mut state = ready_state();
    assert!(state.can_view_channel(sf(GENERAL)));
    assert!(!state.can_view_channel(sf(STAFF)));
    assert!(state.can_send_messages(sf(GENERAL)));
    assert!(state.can_view_channel(sf(ANNOUNCE)));
    assert!(!state.can_send_messages(sf(ANNOUNCE)));
    assert!(!state.can_send_messages(sf(CATEGORY)), "categories have no messages");
    assert!(state.can_view_channel(sf(DM)));
    assert!(state.can_send_messages(sf(DM)));
    assert!(!state.can_send_messages(sf(424242)));

    // Gaining the mod role through GUILD_MEMBER_UPDATE opens the staff channel.
    let changes = state.apply(&dispatch(
        "GUILD_MEMBER_UPDATE",
        json!({"guild_id": GUILD.to_string(), "user": user_json(ME, "me"),
               "roles": [ROLE_HELPER.to_string(), ROLE_MOD.to_string()]}),
    ));
    assert!(changes.contains(&StateChange::PermissionsChanged { guild_id: sf(GUILD) }));
    assert!(state.can_view_channel(sf(STAFF)));

    // Deleting the role removes it from members.
    state.apply(&dispatch(
        "GUILD_ROLE_DELETE",
        json!({"guild_id": GUILD.to_string(), "role_id": ROLE_MOD.to_string()}),
    ));
    assert!(!state.can_view_channel(sf(STAFF)));
    assert_eq!(state.members[&sf(GUILD)][&sf(ME)].roles, vec![sf(ROLE_HELPER)]);

    // A role update granting ADMINISTRATOR bypasses every overwrite.
    state.apply(&dispatch(
        "GUILD_ROLE_UPDATE",
        json!({"guild_id": GUILD.to_string(), "role": role_json(ROLE_HELPER, "helper", 1, bits::ADMINISTRATOR)}),
    ));
    assert!(state.can_view_channel(sf(STAFF)));
    assert!(state.can_send_messages(sf(ANNOUNCE)));
}

#[test]
fn state_owner_sees_everything() {
    let state = ready_state();
    assert_eq!(state.channel_permissions(sf(6001)), bits::ALL);
}

#[test]
fn state_timeout_blocks_sending_until_it_expires() {
    let mut state = ready_state();
    state.apply(&dispatch(
        "GUILD_MEMBER_UPDATE",
        json!({"guild_id": GUILD.to_string(), "user": user_json(ME, "me"),
               "roles": [ROLE_HELPER.to_string()],
               "communication_disabled_until": "2026-01-01T00:00:00.000Z"}),
    ));
    let until = parse_timestamp_ms("2026-01-01T00:00:00Z").unwrap();
    assert!(!state.can_send_messages_at(sf(GENERAL), until - 1));
    assert!(state.can_send_messages_at(sf(GENERAL), until));
    // Timeout does not affect viewing or DMs.
    assert!(state.can_view_channel(sf(GENERAL)));
    assert!(state.can_send_messages_at(sf(DM), until - 1));
}

#[test]
fn state_thread_permissions_derive_from_parent() {
    let mut state = ready_state();
    for (id, parent) in [("2100", GENERAL), ("2101", STAFF), ("2102", 999_999)] {
        state.apply(&dispatch(
            "CHANNEL_CREATE",
            json!({"id": id, "guild_id": GUILD.to_string(), "type": 11, "parent_id": parent.to_string()}),
        ));
    }
    // Parent allows SEND_MESSAGES but not SEND_MESSAGES_IN_THREADS.
    assert!(state.can_view_channel(sf(2100)));
    assert!(!state.can_send_messages(sf(2100)));
    assert!(!state.can_view_channel(sf(2101)));
    assert_eq!(state.channel_permissions(sf(2102)), 0, "unknown parent");

    state.apply(&dispatch(
        "GUILD_ROLE_UPDATE",
        json!({"guild_id": GUILD.to_string(),
               "role": role_json(GUILD, "@everyone", 0, bits::DEFAULT | bits::SEND_MESSAGES_IN_THREADS)}),
    ));
    assert!(state.can_send_messages(sf(2100)));

    // Deleting the parent deletes its threads.
    let changes = state.apply(&dispatch("CHANNEL_DELETE", json!({"id": GENERAL.to_string()})));
    assert_eq!(
        changes,
        vec![
            StateChange::ChannelRemoved { channel_id: sf(2100), guild_id: Some(sf(GUILD)) },
            StateChange::ChannelRemoved { channel_id: sf(GENERAL), guild_id: Some(sf(GUILD)) },
        ]
    );
    assert!(!state.channels.contains_key(&sf(2100)));
    assert!(state.channels.contains_key(&sf(2101)));
}

#[test]
fn deleting_category_detaches_children_to_root() {
    let mut state = ready_state();
    assert_eq!(state.channels.get(&sf(GENERAL)).and_then(|c| c.parent_id), Some(sf(CATEGORY)));
    assert_eq!(state.channels.get(&sf(STAFF)).and_then(|c| c.parent_id), Some(sf(CATEGORY)));

    let changes = state.apply(&dispatch("CHANNEL_DELETE", json!({"id": CATEGORY.to_string()})));
    assert!(!state.channels.contains_key(&sf(CATEGORY)));
    assert_eq!(state.channels.get(&sf(GENERAL)).and_then(|c| c.parent_id), None);
    assert_eq!(state.channels.get(&sf(STAFF)).and_then(|c| c.parent_id), None);
    assert_eq!(
        changes,
        vec![
            StateChange::ChannelUpserted { channel_id: sf(GENERAL), guild_id: Some(sf(GUILD)) },
            StateChange::ChannelUpserted { channel_id: sf(STAFF), guild_id: Some(sf(GUILD)) },
            StateChange::ChannelRemoved { channel_id: sf(CATEGORY), guild_id: Some(sf(GUILD)) },
        ]
    );
}

// ---- timestamps -------------------------------------------------------------

#[test]
fn timestamp_parsing() {
    assert_eq!(parse_timestamp_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_timestamp_ms("2015-01-01T00:00:00+00:00"), Some(FLUXER_EPOCH_MS as i64));
    assert_eq!(parse_timestamp_ms("2015-01-01T00:00:00.123456+00:00"), Some(FLUXER_EPOCH_MS as i64 + 123));
    assert_eq!(parse_timestamp_ms("2015-01-01T00:00:00.5Z"), Some(FLUXER_EPOCH_MS as i64 + 500));
    assert_eq!(parse_timestamp_ms("2015-01-01T09:00:00+09:00"), Some(FLUXER_EPOCH_MS as i64));
    assert_eq!(parse_timestamp_ms("2014-12-31T23:00:00-01:00"), Some(FLUXER_EPOCH_MS as i64));
    assert_eq!(parse_timestamp_ms("2024-02-29T00:00:00"), Some(1_709_164_800_000));
    for bad in ["", "2015-01-01", "2015-13-01T00:00:00Z", "2015-01-01T00:00:00+0900", "2015-01-01T00:00:00Zjunk", "２０15-01-01T00:00:00Z"] {
        assert_eq!(parse_timestamp_ms(bad), None, "{bad}");
    }
}

#[test]
fn snowflake_round_trip() {
    let id = Snowflake::from_unix_ms(FLUXER_EPOCH_MS + 1234);
    assert_eq!(id.timestamp_ms(), FLUXER_EPOCH_MS + 1234);
    assert_eq!(serde_json::to_value(id).unwrap(), json!(id.0.to_string()));
    assert_eq!(serde_json::from_value::<Snowflake>(json!(42)).unwrap(), sf(42));
    assert_eq!(serde_json::from_value::<Snowflake>(json!("42")).unwrap(), sf(42));
    assert!(serde_json::from_value::<Snowflake>(json!(-1)).is_err());
}

// ---- message windows ----------------------------------------------------------

const CH: u64 = 7000;

fn ids(window: &MessageWindow) -> (Snowflake, Snowflake, usize) {
    let first = window.messages.first().unwrap().id;
    let last = window.messages.last().unwrap().id;
    (first, last, window.messages.len())
}

#[test]
fn window_replace_flags_follow_page_fill() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 100, 150), LoadMode::Replace);
    let w = store.window(sf(CH)).unwrap();
    assert!(w.ready && w.has_more_before && !w.has_more_after);
    assert_eq!(ids(w), (mid(100), mid(149), 50));

    store.load(sf(CH), page(CH, 140, 150), LoadMode::Replace);
    let w = store.window(sf(CH)).unwrap();
    assert!(!w.has_more_before && !w.has_more_after);
    assert_eq!(w.messages.len(), 10);
}

#[test]
fn window_before_pages_trim_newest_over_cap() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 200, 250), LoadMode::Replace);
    for start in [150, 100, 50] {
        store.load(sf(CH), page(CH, start, start + 50), LoadMode::Before);
    }
    let w = store.window(sf(CH)).unwrap();
    assert_eq!(ids(w), (mid(50), mid(249), MAX_LOADED_MESSAGES));
    assert!(!w.has_more_after, "200 is the cap, not over it");

    store.load(sf(CH), page(CH, 0, 50), LoadMode::Before);
    let w = store.window(sf(CH)).unwrap();
    assert_eq!(ids(w), (mid(0), mid(99), TRUNCATED_MESSAGE_VIEW_SIZE));
    assert!(w.has_more_before, "full before page");
    assert!(w.has_more_after, "newest side was trimmed");

    // A short before page clears has_more_before but keeps has_more_after.
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 200, 250), LoadMode::Replace);
    store.load(sf(CH), page(CH, 190, 200), LoadMode::Before);
    let w = store.window(sf(CH)).unwrap();
    assert!(!w.has_more_before && !w.has_more_after);
    assert_eq!(w.messages.len(), 60);
}

#[test]
fn window_after_pages_trim_oldest_over_cap() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 0, 50), LoadMode::Replace);
    for start in [50, 100, 150, 200] {
        store.load(sf(CH), page(CH, start, start + 50), LoadMode::After);
    }
    let w = store.window(sf(CH)).unwrap();
    assert_eq!(ids(w), (mid(150), mid(249), TRUNCATED_MESSAGE_VIEW_SIZE));
    assert!(w.has_more_before, "oldest side was trimmed");
    assert!(w.has_more_after, "full after page");

    store.load(sf(CH), page(CH, 250, 260), LoadMode::After);
    let w = store.window(sf(CH)).unwrap();
    assert!(!w.has_more_after);
    assert_eq!(w.messages.len(), 110);
}

#[test]
fn window_not_ready_is_replaced_with_requested_side_flags() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 0, 50), LoadMode::After);
    let w = store.window(sf(CH)).unwrap();
    // getBasePageState: isAfter → hasMoreBefore false, hasMoreAfter = filled.
    assert!(w.ready && !w.has_more_before && w.has_more_after);
}

#[test]
fn incoming_appends_and_trims_from_top_when_pinned() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 100, 150), LoadMode::Replace);
    store.load(sf(CH), page(CH, 50, 100), LoadMode::Before);
    store.load(sf(CH), page(CH, 0, 50), LoadMode::Before);
    let w = store.window(sf(CH)).unwrap();
    assert_eq!(w.messages.len(), 150);

    assert!(store.apply_incoming(message(mid(150), CH, OTHER, "new")));
    let w = store.window(sf(CH)).unwrap();
    assert_eq!(ids(w), (mid(51), mid(150), TRUNCATED_MESSAGE_VIEW_SIZE));
    assert!(w.has_more_before);
}

#[test]
fn incoming_while_scrolled_up_grows_window() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 100, 150), LoadMode::Replace);
    store.load(sf(CH), page(CH, 50, 100), LoadMode::Before);
    store.set_pinned_to_end(sf(CH), false);
    assert!(store.apply_incoming(message(mid(150), CH, OTHER, "new")));
    let w = store.window(sf(CH)).unwrap();
    assert_eq!(ids(w), (mid(50), mid(150), 101));
}

#[test]
fn incoming_ignored_without_ready_window_or_with_newer_pages() {
    let mut store = MessageStore::new();
    assert!(!store.apply_incoming(message(mid(1), CH, OTHER, "x")), "no window");

    store.load(sf(CH), page(CH, 0, 50), LoadMode::Replace);
    store.load(sf(CH), page(CH, 50, 100), LoadMode::After);
    assert!(store.window(sf(CH)).unwrap().has_more_after);
    assert!(!store.apply_incoming(message(mid(500), CH, OTHER, "x")), "gap to live edge");
}

#[test]
fn nonce_reconciliation_pending_to_sent() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 0, 10), LoadMode::Replace);
    let nonce = mid(20);
    store.insert_pending(sf(CH), nonce, "hello".into(), serde_json::from_value(user_json(ME, "me")).unwrap());
    let pending = store.window(sf(CH)).unwrap().messages.last().unwrap().clone();
    assert_eq!((pending.id, pending.state, pending.nonce.as_deref()), (nonce, MessageState::Sending, Some(nonce.to_string().as_str())));

    // Server id is older than the nonce (clock skew): still replaced and kept sorted.
    let mut confirmed = message(mid(15), CH, ME, "hello");
    confirmed.nonce = Some(nonce.to_string());
    assert!(store.apply_incoming(confirmed.clone()));
    let w = store.window(sf(CH)).unwrap();
    assert_eq!(w.messages.len(), 11);
    assert_eq!(w.messages.last().unwrap().id, mid(15));
    assert_eq!(w.messages.last().unwrap().state, MessageState::Sent);
    assert!(w.messages.windows(2).all(|p| p[0].id < p[1].id));

    // The gateway echo of the same message changes nothing.
    assert!(store.apply_incoming(confirmed));
    assert_eq!(store.window(sf(CH)).unwrap().messages.len(), 11);
}

#[test]
fn nonce_from_another_author_does_not_replace() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 0, 10), LoadMode::Replace);
    let nonce = mid(20);
    store.insert_pending(sf(CH), nonce, "hello".into(), serde_json::from_value(user_json(ME, "me")).unwrap());
    let mut spoof = message(mid(21), CH, OTHER, "hello");
    spoof.nonce = Some(nonce.to_string());
    assert!(store.apply_incoming(spoof));
    let w = store.window(sf(CH)).unwrap();
    assert_eq!(w.messages.len(), 12);
    assert!(w.messages.iter().any(|m| m.id == nonce && m.state == MessageState::Sending));
}

#[test]
fn send_failure_retry_and_discard() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 0, 1), LoadMode::Replace);
    let nonce = mid(5);
    store.insert_pending(sf(CH), nonce, "x".into(), serde_json::from_value(user_json(ME, "me")).unwrap());
    assert_eq!(store.mark_failed(nonce), Some(sf(CH)));
    assert_eq!(store.mark_failed(nonce), None, "already failed");
    assert_eq!(store.mark_sending(nonce), Some(sf(CH)));
    assert_eq!(store.mark_failed(nonce), Some(sf(CH)));
    assert!(!store.remove_pending(sf(CH), mid(0)), "sent messages are not discardable");
    assert!(store.remove_pending(sf(CH), nonce));
    assert_eq!(store.window(sf(CH)).unwrap().messages.len(), 1);
}

#[test]
fn replace_keeps_unsent_messages_not_in_the_page() {
    let mut store = MessageStore::new();
    store.load(sf(CH), page(CH, 0, 5), LoadMode::Replace);
    let me: User = serde_json::from_value(user_json(ME, "me")).unwrap();
    store.insert_pending(sf(CH), mid(10), "delivered".into(), me.clone());
    store.insert_pending(sf(CH), mid(11), "lost".into(), me);
    store.mark_failed(mid(11));

    // The reload contains "delivered" under its server id but not "lost".
    let mut reload = page(CH, 0, 5);
    reload.insert(0, message(mid(12), CH, ME, "delivered"));
    store.load(sf(CH), reload.clone(), LoadMode::Replace);
    let w = store.window(sf(CH)).unwrap();
    let summary: Vec<_> = w.messages.iter().map(|m| (m.content.as_str(), m.state)).collect();
    assert_eq!(summary[5..], [("lost", MessageState::Failed), ("delivered", MessageState::Sent)]);
    assert_eq!(w.messages.len(), 7);

    // A Before load into a window that is not ready replaces the visible slice.
    store.invalidate();
    store.load(sf(CH), page(CH, 0, 2), LoadMode::Before);
    let w = store.window(sf(CH)).unwrap();
    assert!(w.ready);
    assert!(w.has_more_after);
    assert_eq!(w.messages.iter().map(|m| m.id).collect::<Vec<_>>(), vec![mid(0), mid(1)]);
    assert_eq!(store.message(sf(CH), mid(11)).unwrap().state, MessageState::Failed);
}

#[test]
fn reconnect_same_session_invalidates_new_session_clears() {
    let mut state = ready_state();
    state.messages.load(sf(GENERAL), page(GENERAL, 0, 10), LoadMode::Replace);
    state.apply(&dispatch("READY", ready_payload()));
    let w = state.messages.window(sf(GENERAL)).unwrap();
    assert!(!w.ready);
    assert_eq!(w.messages.len(), 10);
    assert!(!state.messages.apply_incoming(message(mid(11), GENERAL, OTHER, "x")));

    let mut next = ready_payload();
    next["session_id"] = json!("session-b");
    state.apply(&dispatch("READY", next));
    assert!(state.messages.window(sf(GENERAL)).is_none());
}

#[test]
fn new_session_retains_local_outgoing_rows_and_avoids_matching_old_history() {
    let mut state = ready_state();
    let channel = sf(GENERAL);
    let me = state.current_user.clone().unwrap();
    let old = message(mid(1), GENERAL, ME, "same text");
    state.messages.load(channel, vec![old.clone()], LoadMode::Replace);
    let failed = mid(10);
    let sending = mid(11);
    let no_nonce = mid(12);
    state.messages.insert_pending(channel, failed, "same text".to_owned(), me.clone());
    state.messages.insert_pending(channel, sending, "sending".to_owned(), me.clone());
    state.messages.insert_pending(channel, no_nonce, "received without nonce".to_owned(), me);
    state.messages.mark_failed(failed);

    let mut next = ready_payload();
    next["session_id"] = json!("new-session");
    state.apply(&dispatch("READY", next.clone()));
    let window = state.messages.window(channel).unwrap();
    assert!(!window.ready);
    assert_eq!(window.messages.iter().map(|m| m.state).collect::<Vec<_>>(), [
        MessageState::Failed, MessageState::Sending, MessageState::Sending,
    ]);

    let mut confirmed = message(mid(13), GENERAL, ME, "sending");
    confirmed.nonce = Some(sending.to_string());
    let newer_no_nonce = message(mid(14), GENERAL, ME, "received without nonce");
    state.messages.load(channel, vec![newer_no_nonce, confirmed.clone(), old], LoadMode::Replace);
    assert_eq!(state.messages.message(channel, failed).unwrap().state, MessageState::Failed);
    assert!(state.messages.message(channel, sending).is_none());
    assert!(state.messages.message(channel, no_nonce).is_none());
    assert!(state.messages.message(channel, confirmed.id).is_some());

    next["user"] = user_json(OTHER, "different user");
    state.apply(&dispatch("READY", next));
    assert!(state.messages.window(channel).is_none());
}

#[test]
fn lru_evicts_least_recent_but_never_selected() {
    let mut store = MessageStore::new();
    store.select(Some(sf(1)));
    for ch in 1..=(MAX_CACHED_CHANNELS as u64 + 2) {
        store.load(sf(ch), page(ch, 0, 1), LoadMode::Replace);
    }
    assert!(store.window(sf(1)).is_some(), "selected");
    assert!(store.window(sf(2)).is_none());
    assert!(store.window(sf(3)).is_none());
    assert!(store.window(sf(4)).is_some());

    // Touching a channel by loading it moves it to the back.
    store.load(sf(4), page(4, 0, 2), LoadMode::Replace);
    store.load(sf(100), page(100, 0, 1), LoadMode::Replace);
    assert!(store.window(sf(4)).is_some());
    assert!(store.window(sf(5)).is_none());
    let live = (1..=200).filter(|c| store.window(sf(*c)).is_some()).count();
    assert_eq!(live, MAX_CACHED_CHANNELS);
}

#[test]
fn message_update_merges_partial_payload_and_delete_removes() {
    let mut state = ready_state();
    let mut first = message_json(mid(1), GENERAL, OTHER, "original");
    first["attachments"] = json!([{"id": "9", "filename": "a.png", "size": 3}]);
    let first: Message = serde_json::from_value(first).unwrap();
    let mut newest_first = page(GENERAL, 2, 5);
    newest_first.push(first);
    state.messages.load(sf(GENERAL), newest_first, LoadMode::Replace);

    let changes = state.apply(&dispatch(
        "MESSAGE_UPDATE",
        json!({"id": mid(1).to_string(), "channel_id": GENERAL.to_string(),
               "content": "edited", "edited_timestamp": "2025-06-15T00:01:00+00:00"}),
    ));
    assert_eq!(changes, vec![StateChange::MessagesChanged { channel_id: sf(GENERAL) }]);
    let m = &state.messages.window(sf(GENERAL)).unwrap().messages[0];
    assert_eq!(m.content, "edited");
    assert!(m.is_edited());
    assert_eq!(m.attachments.len(), 1, "fields absent from the patch are kept");
    assert_eq!(m.author.id, sf(OTHER));

    // Unknown message: no change.
    assert!(state
        .apply(&dispatch("MESSAGE_UPDATE", json!({"id": "1", "channel_id": GENERAL.to_string(), "content": "x"})))
        .is_empty());

    state.apply(&dispatch("MESSAGE_DELETE", json!({"id": mid(1).to_string(), "channel_id": GENERAL.to_string()})));
    assert_eq!(state.messages.window(sf(GENERAL)).unwrap().messages.len(), 3);

    let mut dm_reply = message_json(mid(20), DM, OTHER, "cross-channel reply");
    dm_reply["message_reference"] = json!({
        "type": 0, "channel_id": GENERAL.to_string(), "message_id": mid(2).to_string()
    });
    state.messages.load(sf(DM), vec![serde_json::from_value(dm_reply).unwrap()], LoadMode::Replace);

    let changes = state.apply(&dispatch(
        "MESSAGE_DELETE_BULK",
        json!({"ids": [mid(2).to_string(), mid(3).to_string(), "77"], "channel_id": GENERAL.to_string()}),
    ));
    assert_eq!(changes, vec![StateChange::MessagesChanged { channel_id: sf(GENERAL) }]);
    let left: Vec<_> = state.messages.window(sf(GENERAL)).unwrap().messages.iter().map(|m| m.id).collect();
    assert_eq!(left, vec![mid(4)]);
    let reply = state.messages.message(sf(DM), mid(20)).unwrap();
    assert_eq!(state.reply_preview(reply), ReplyPreview::Deleted);
}

// ---- unread -------------------------------------------------------------------

#[test]
fn unread_from_ready() {
    let state = ready_state();
    assert_eq!(state.channel_unread(sf(GENERAL)), Unread { unread: true, mention_count: 1 });
    assert_eq!(state.channel_unread(sf(ANNOUNCE)), Unread { unread: false, mention_count: 0 });
    assert_eq!(state.channel_unread(sf(DM)), Unread::default());
    assert_eq!(state.channel_unread(sf(CATEGORY)), Unread::default());
    // STAFF has no read state but is not viewable, so the guild only counts GENERAL.
    assert_eq!(state.guild_unread(sf(GUILD)), Unread { unread: true, mention_count: 1 });
}

#[test]
fn unread_without_ack_uses_joined_at_floor() {
    let mut state = ready_state();
    // STAFF: no read state, last message mid(5) (2025) after joined_at (2015).
    assert!(state.channel_unread(sf(STAFF)).unread);
    state.guilds.get_mut(&sf(GUILD)).unwrap().joined_at = Some("2030-01-01T00:00:00Z".into());
    assert!(!state.channel_unread(sf(STAFF)).unread);
    // Unparsable joined_at falls back to the channel creation time.
    state.guilds.get_mut(&sf(GUILD)).unwrap().joined_at = Some("garbage".into());
    assert!(state.channel_unread(sf(STAFF)).unread);
}

#[test]
fn unread_anchor_is_the_ack_or_the_floor() {
    let mut state = ready_state();
    // GENERAL: acked at mid(8), last mid(10).
    assert_eq!(state.unread_anchor(sf(GENERAL)), Some(mid(8)));
    // ANNOUNCE: read.
    assert_eq!(state.unread_anchor(sf(ANNOUNCE)), None);
    // STAFF: no ack; the floor is joined_at (2015-01-01). Anything after it
    // is newer, anything in that millisecond is not.
    let floor_ms = 1_420_070_400_000;
    let anchor = state.unread_anchor(sf(STAFF)).unwrap();
    assert!(Snowflake::from_unix_ms(floor_ms + 1) > anchor);
    assert!(Snowflake(Snowflake::from_unix_ms(floor_ms).0 + 5) <= anchor);
    state.local_ack(sf(GENERAL), mid(10));
    assert_eq!(state.unread_anchor(sf(GENERAL)), None);
}

/// Sidebar order: uncategorized channels first (text before voice, by
/// position), then categories with their children (text before voice).
#[test]
fn channel_tree_puts_uncategorized_first_and_text_before_voice() {
    let mut state = ready_state();
    let add = |state: &mut ClientState, id: u64, kind: u8, name: &str, position: i32, parent: Option<u64>| {
        state.apply(&dispatch(
            "CHANNEL_CREATE",
            json!({"id": id.to_string(), "guild_id": GUILD.to_string(), "type": kind, "name": name,
                   "position": position, "parent_id": parent.map(|p| p.to_string())}),
        ));
    };
    // A root voice channel positioned before root text channels and a
    // voice channel inside the category positioned before its text ones.
    add(&mut state, 2010, 2, "root-voice", 0, None);
    add(&mut state, 2011, 0, "root-text", 9, None);
    add(&mut state, 2012, 2, "cat-voice", 0, Some(CATEGORY));
    add(&mut state, 2013, 4, "Later", 1, None);
    add(&mut state, 2014, 0, "later-text", 0, Some(2013));
    let tree: Vec<(String, Vec<String>)> = state
        .channel_tree(sf(GUILD))
        .into_iter()
        .map(|n| {
            (
                n.channel.name.unwrap_or_default(),
                n.children.into_iter().map(|c| c.name.unwrap_or_default()).collect(),
            )
        })
        .collect();
    let expected: Vec<(String, Vec<String>)> = vec![
        ("announcements".into(), vec![]),
        ("root-text".into(), vec![]),
        ("root-voice".into(), vec![]),
        ("Voice".into(), vec![]),
        ("Text".into(), vec!["general".into(), "cat-voice".into()]),
        ("Later".into(), vec!["later-text".into()]),
    ];
    assert_eq!(tree, expected);
}

#[test]
fn unread_message_create_mentions_and_own_ack() {
    let mut state = ready_state();
    state.messages.load(sf(GENERAL), page(GENERAL, 0, 10), LoadMode::Replace);

    // Plain message from someone else: unread, no mention.
    let changes = state.apply(&dispatch("MESSAGE_CREATE", message_json(mid(11), GENERAL, OTHER, "hi")));
    assert_eq!(
        changes,
        vec![
            StateChange::ReadStateChanged { channel_id: sf(GENERAL) },
            StateChange::MessagesChanged { channel_id: sf(GENERAL) },
        ]
    );
    assert_eq!(state.channels[&sf(GENERAL)].last_message_id, Some(mid(11)));
    assert_eq!(state.channel_unread(sf(GENERAL)).mention_count, 1);

    // User mention, role mention, @everyone each count once.
    let mut m = message_json(mid(12), GENERAL, OTHER, "@me");
    m["mentions"] = json!([user_json(ME, "me")]);
    state.apply(&dispatch("MESSAGE_CREATE", m.clone()));
    let mut r = message_json(mid(13), GENERAL, OTHER, "@helper");
    r["mention_roles"] = json!([ROLE_HELPER.to_string()]);
    state.apply(&dispatch("MESSAGE_CREATE", r));
    let mut e = message_json(mid(14), GENERAL, OTHER, "@everyone");
    e["mention_everyone"] = json!(true);
    state.apply(&dispatch("MESSAGE_CREATE", e));
    let mut other_role = message_json(mid(15), GENERAL, OTHER, "@mod");
    other_role["mention_roles"] = json!([ROLE_MOD.to_string()]);
    state.apply(&dispatch("MESSAGE_CREATE", other_role));
    assert_eq!(state.channel_unread(sf(GENERAL)), Unread { unread: true, mention_count: 4 });

    // Duplicate delivery of an already shown message is not counted again.
    let mut dup = message_json(mid(15), GENERAL, OTHER, "@me again");
    dup["mentions"] = json!([user_json(ME, "me")]);
    state.apply(&dispatch("MESSAGE_CREATE", dup));
    assert_eq!(state.channel_unread(sf(GENERAL)).mention_count, 4);

    // Older message than last (late delivery): last_message_id unchanged.
    state.apply(&dispatch("MESSAGE_CREATE", m));
    assert_eq!(state.channels[&sf(GENERAL)].last_message_id, Some(mid(15)));

    // Own message acks the channel and clears mentions.
    state.apply(&dispatch("MESSAGE_CREATE", message_json(mid(16), GENERAL, ME, "mine")));
    assert_eq!(state.channel_unread(sf(GENERAL)), Unread { unread: false, mention_count: 0 });
    assert_eq!(state.read_states[&sf(GENERAL)].ack_message_id, Some(mid(16)));
}

#[test]
fn unread_dm_messages_always_mention() {
    let mut state = ready_state();
    state.apply(&dispatch("MESSAGE_CREATE", message_json(mid(21), DM, OTHER, "psst")));
    assert_eq!(state.channel_unread(sf(DM)), Unread { unread: true, mention_count: 1 });
}

#[test]
fn unread_reopened_dm_counts_its_first_message() {
    let mut state = ready_state();
    // CHANNEL_CREATE for a new DM already names the message as last.
    state.apply(&dispatch(
        "CHANNEL_CREATE",
        json!({"id": "3001", "type": 1, "recipients": [user_json(OTHER, "other")], "last_message_id": mid(30).to_string()}),
    ));
    let changes = state.apply(&dispatch("MESSAGE_CREATE", message_json(mid(30), 3001, OTHER, "hey")));
    assert_eq!(changes, vec![StateChange::ReadStateChanged { channel_id: sf(3001) }]);
    assert_eq!(state.channel_unread(sf(3001)), Unread { unread: true, mention_count: 1 });
    assert_eq!(state.dm_channels().iter().map(|c| c.id).collect::<Vec<_>>(), vec![sf(3001), sf(DM)]);
}

#[test]
fn unread_message_ack_versions_and_zero() {
    let mut state = ready_state();
    let ack = |message_id: &str, version: Option<&str>, mentions: u64| {
        let mut d = json!({"channel_id": GENERAL.to_string(), "message_id": message_id, "mention_count": mentions});
        if let Some(v) = version {
            d["version"] = json!(v);
        }
        dispatch("MESSAGE_ACK", d)
    };
    // Older version than READY's "5" is ignored.
    assert!(state.apply(&ack(&mid(10).to_string(), Some("4"), 0)).is_empty());
    assert!(state.channel_unread(sf(GENERAL)).unread);

    state.apply(&ack(&mid(10).to_string(), Some("6"), 0));
    assert_eq!(state.channel_unread(sf(GENERAL)), Unread::default());
    assert_eq!(state.read_states[&sf(GENERAL)].version, Some(sf(6)));

    // A missing version compares equal and is applied; "0" clears the ack.
    state.apply(&ack("0", None, 2));
    let rs = &state.read_states[&sf(GENERAL)];
    assert_eq!((rs.ack_message_id, rs.mention_count, rs.version), (None, 2, None));
    assert!(state.channel_unread(sf(GENERAL)).unread, "falls back to the joined_at floor");
}

#[test]
fn unread_local_ack() {
    let mut state = ready_state();
    assert_eq!(state.local_ack(sf(GENERAL), mid(10)), vec![StateChange::ReadStateChanged { channel_id: sf(GENERAL) }]);
    assert_eq!(state.channel_unread(sf(GENERAL)), Unread::default());
    assert!(state.local_ack(sf(GENERAL), mid(9)).is_empty(), "older ack is a no-op");
    assert_eq!(state.read_states[&sf(GENERAL)].ack_message_id, Some(mid(10)));
}

#[test]
fn passive_updates_bump_last_message_only_forward() {
    let mut state = ready_state();
    let changes = state.apply(&dispatch(
        "PASSIVE_UPDATES",
        json!({"guild_id": GUILD.to_string(), "channels": {
            ANNOUNCE.to_string(): mid(30).to_string(),
            GENERAL.to_string(): mid(1).to_string(),
            DM.to_string(): mid(99).to_string(),
        }}),
    ));
    assert_eq!(changes, vec![StateChange::ReadStateChanged { channel_id: sf(ANNOUNCE) }]);
    assert!(state.channel_unread(sf(ANNOUNCE)).unread);
    assert_eq!(state.channels[&sf(GENERAL)].last_message_id, Some(mid(10)));
}

// ---- guild lifecycle ------------------------------------------------------------

#[test]
fn guild_outage_and_return_keeps_read_states() {
    let mut state = ready_state();
    state.messages.load(sf(GENERAL), page(GENERAL, 0, 3), LoadMode::Replace);
    let changes = state.apply(&dispatch("GUILD_DELETE", json!({"id": GUILD.to_string(), "unavailable": true})));
    assert_eq!(changes, vec![StateChange::GuildRemoved { guild_id: sf(GUILD), unavailable: true }]);
    assert!(state.unavailable_guilds.contains(&sf(GUILD)));
    assert!(state.is_unavailable_channel(sf(GENERAL)));
    assert!(!state.can_view_channel(sf(GENERAL)));
    assert!(!state.channels.contains_key(&sf(GENERAL)));
    assert!(state.read_states.contains_key(&sf(GENERAL)));
    assert!(state.messages.window(sf(GENERAL)).is_none());

    let guild_payload = ready_payload()["guilds"][0].clone();
    let changes = state.apply(&dispatch("GUILD_CREATE", guild_payload));
    assert_eq!(changes, vec![StateChange::GuildUpserted { guild_id: sf(GUILD) }]);
    assert!(!state.unavailable_guilds.contains(&sf(GUILD)));
    assert!(!state.is_unavailable_channel(sf(GENERAL)));
    assert_eq!(state.channel_unread(sf(GENERAL)), Unread { unread: true, mention_count: 1 });
}

#[test]
fn unavailable_channel_identity_survives_ready_until_a_definitive_removal() {
    let mut state = ready_state();
    let me = state.current_user.clone().unwrap();
    state.messages.insert_pending(sf(GENERAL), mid(10), "sending".to_owned(), me.clone());
    state.messages.insert_pending(sf(ANNOUNCE), mid(11), "also sending".to_owned(), me);
    state.apply(&dispatch("GUILD_DELETE", json!({ "id": sf(GUILD), "unavailable": true })));
    assert!(state.messages.message(sf(GENERAL), mid(10)).is_some());
    let mut snapshot = ready_payload();
    snapshot["session_id"] = json!("another-session");
    snapshot["guilds"] = json!([{ "id": sf(GUILD), "unavailable": true }]);
    state.apply(&dispatch("READY", snapshot));
    assert!(state.is_unavailable_channel(sf(GENERAL)));
    assert!(state.is_unavailable_channel(sf(ANNOUNCE)));
    assert!(!state.can_view_channel(sf(ANNOUNCE)));
    assert_eq!(state.messages.mark_failed(mid(10)), Some(sf(GENERAL)));

    assert_eq!(
        state.apply(&dispatch("CHANNEL_DELETE", json!({ "id": sf(GENERAL) }))),
        [StateChange::ChannelRemoved { channel_id: sf(GENERAL), guild_id: Some(sf(GUILD)) }],
    );
    assert!(!state.is_unavailable_channel(sf(GENERAL)));
    assert!(state.messages.window(sf(GENERAL)).is_none());
    state.apply(&dispatch("GUILD_DELETE", json!({ "id": sf(GUILD) })));
    assert!(!state.is_unavailable_channel(sf(ANNOUNCE)));
    assert!(state.messages.window(sf(ANNOUNCE)).is_none());
}

#[test]
fn guild_create_drops_missing_channels_with_their_read_state() {
    let mut state = ready_state();
    state.messages.load(sf(GENERAL), page(GENERAL, 0, 3), LoadMode::Replace);
    state.messages.load(sf(ANNOUNCE), page(ANNOUNCE, 0, 3), LoadMode::Replace);
    let mut payload = ready_payload()["guilds"][0].clone();
    let channels = payload["channels"].as_array_mut().unwrap();
    channels.retain(|c| c["id"] != json!(ANNOUNCE.to_string()));
    state.apply(&dispatch("GUILD_SYNC", payload));
    assert!(!state.channels.contains_key(&sf(ANNOUNCE)));
    assert!(!state.read_states.contains_key(&sf(ANNOUNCE)));
    assert!(state.messages.window(sf(ANNOUNCE)).is_none());
    assert!(state.read_states.contains_key(&sf(GENERAL)));
    assert!(state.messages.window(sf(GENERAL)).is_some(), "kept channel keeps its window");
}

#[test]
fn guild_update_role_bulk_and_member_remove() {
    let mut state = ready_state();
    let changes = state.apply(&dispatch(
        "GUILD_UPDATE",
        json!({"id": GUILD.to_string(), "name": "Alpha 2", "icon": "abc", "owner_id": ME.to_string()}),
    ));
    assert_eq!(
        changes,
        vec![
            StateChange::GuildUpserted { guild_id: sf(GUILD) },
            StateChange::PermissionsChanged { guild_id: sf(GUILD) },
        ]
    );
    assert_eq!(state.guilds[&sf(GUILD)].member_count, Some(3), "kept when absent");
    assert!(state.can_view_channel(sf(STAFF)), "owner now");

    state.apply(&dispatch(
        "GUILD_ROLE_UPDATE_BULK",
        json!({"guild_id": GUILD.to_string(), "roles": [role_json(ROLE_MOD, "moderator", 9, 0)]}),
    ));
    assert_eq!(state.guilds[&sf(GUILD)].roles[&sf(ROLE_MOD)].name, "moderator");

    let changes = state.apply(&dispatch(
        "GUILD_MEMBER_REMOVE",
        json!({"guild_id": GUILD.to_string(), "user": user_json(OTHER, "other")}),
    ));
    assert_eq!(changes, vec![StateChange::MemberUpdated { guild_id: sf(GUILD), user_id: sf(OTHER) }]);
    assert!(state
        .apply(&dispatch("GUILD_MEMBER_REMOVE", json!({"guild_id": GUILD.to_string(), "user": user_json(OTHER, "other")})))
        .is_empty());

    let changes = state.apply(&dispatch("GUILD_DELETE", json!({"id": GUILD.to_string()})));
    assert_eq!(changes, vec![StateChange::GuildRemoved { guild_id: sf(GUILD), unavailable: false }]);
    assert!(!state.unavailable_guilds.contains(&sf(GUILD)));
}

#[test]
fn channel_update_is_a_shallow_merge_and_ignores_unknown() {
    let mut state = ready_state();
    state.apply(&dispatch("CHANNEL_UPDATE", json!({"id": GENERAL.to_string(), "name": "chat", "topic": "t"})));
    let c = &state.channels[&sf(GENERAL)];
    assert_eq!((c.name.as_deref(), c.topic.as_deref()), (Some("chat"), Some("t")));
    assert_eq!(c.parent_id, Some(sf(CATEGORY)));
    assert_eq!(c.guild_id, Some(sf(GUILD)));
    assert!(state.apply(&dispatch("CHANNEL_UPDATE", json!({"id": "4242", "name": "x"}))).is_empty());

    // Bulk form and a CHANNEL_CREATE for an unknown guild (ignored).
    state.apply(&dispatch(
        "CHANNEL_UPDATE_BULK",
        json!({"channels": [{"id": ANNOUNCE.to_string(), "position": 9}]}),
    ));
    assert_eq!(state.channels[&sf(ANNOUNCE)].position, 9);
    assert!(state
        .apply(&dispatch("CHANNEL_CREATE", json!({"id": "8000", "guild_id": "8888", "type": 0})))
        .is_empty());
}

#[test]
fn user_update_merges_into_current_user() {
    let mut state = ready_state();
    state.apply(&dispatch("USER_UPDATE", json!({"id": ME.to_string(), "global_name": "Me!"})));
    let me = state.current_user.as_ref().unwrap();
    assert_eq!((me.username.as_str(), me.display_name()), ("me", "Me!"));
}

#[test]
fn ready_without_user_is_ignored_and_malformed_items_are_skipped() {
    let mut state = ready_state();
    assert!(state.apply(&dispatch("READY", json!({"session_id": "x"}))).is_empty());
    assert_eq!(state.session_id.as_deref(), Some("session-a"));

    let mut payload = ready_payload();
    payload["private_channels"].as_array_mut().unwrap().push(json!({"id": "not-a-number", "type": 1}));
    let mut state = ClientState::new();
    state.apply(&dispatch("READY", payload));
    assert!(state.channels.contains_key(&sf(DM)));
}

// ---- fixture snapshot -------------------------------------------------------------

/// UI-facing view of the state, stable for snapshotting.
fn view(state: &ClientState) -> Value {
    let guilds: Vec<Value> = state
        .sorted_guilds()
        .into_iter()
        .map(|g| {
            let tree: Vec<Value> = state
                .channel_tree(g.id)
                .iter()
                .map(|n| {
                    json!({
                        "channel": n.channel.name,
                        "children": n.children.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            json!({"id": g.id, "name": g.name, "tree": tree, "unread": state.guild_unread(g.id)})
        })
        .collect();
    let channels: BTreeMap<String, Value> = state
        .channels
        .values()
        .map(|c| {
            (
                c.id.to_string(),
                json!({
                    "name": c.name,
                    "last_message_id": c.last_message_id,
                    "view": state.can_view_channel(c.id),
                    "send": state.can_send_messages_at(c.id, 0),
                    "unread": state.channel_unread(c.id),
                }),
            )
        })
        .collect();
    json!({
        "session_id": state.session_id,
        "current_user": state.current_user,
        "unavailable_guilds": state.unavailable_guilds,
        "guilds": guilds,
        "dms": state.dm_channels().iter().map(|c| c.id).collect::<Vec<_>>(),
        "channels": channels,
        "read_states": state.read_states,
        "general_window": state.messages.window(sf(GENERAL)),
        "typing": state.typing,
        "presences": state.presences,
        "relationships": state.relationships,
        "member_list": state.member_list(sf(GUILD), sf(GENERAL)),
    })
}

#[test]
fn snapshot_after_fixture_dispatch_sequence() {
    let mut state = ready_state();
    let me: User = serde_json::from_value(user_json(ME, "me")).unwrap();
    state.messages.select(Some(sf(GENERAL)));
    state.messages.load(sf(GENERAL), page(GENERAL, 8, 11), LoadMode::Replace);

    let nonce = mid(40);
    state.messages.insert_pending(sf(GENERAL), nonce, "sending".into(), me.clone());
    state.messages.insert_pending(sf(GENERAL), mid(41), "will fail".into(), me);
    state.messages.mark_failed(mid(41));

    let mut events = vec![
        dispatch("MESSAGE_CREATE", message_json(mid(11), GENERAL, OTHER, "hello")),
        dispatch("CHANNEL_CREATE", json!({"id": "2006", "guild_id": GUILD.to_string(), "type": 0,
                                          "name": "new", "position": 4})),
        dispatch("GUILD_ROLE_CREATE", json!({"guild_id": GUILD.to_string(),
                                             "role": role_json(1004, "muted", 0, 0)})),
        dispatch("MESSAGE_UPDATE", json!({"id": mid(9).to_string(), "channel_id": GENERAL.to_string(),
                                          "content": "m9 (edited)", "edited_timestamp": "2025-06-15T00:05:00+00:00"})),
        dispatch("MESSAGE_DELETE", json!({"id": mid(8).to_string(), "channel_id": GENERAL.to_string()})),
        dispatch("MESSAGE_ACK", json!({"channel_id": ANNOUNCE.to_string(), "message_id": mid(2).to_string(),
                                       "mention_count": 0, "version": "7"})),
        dispatch("GUILD_DELETE", json!({"id": "6000"})),
    ];
    let mut confirmed = message_json(mid(42), GENERAL, ME, "sending");
    confirmed["nonce"] = json!(nonce.to_string());
    events.push(dispatch("MESSAGE_CREATE", confirmed));
    let reaction = |user: u64| {
        json!({"user_id": user.to_string(), "channel_id": GENERAL.to_string(), "message_id": mid(10).to_string(),
               "emoji": {"name": "👍"}})
    };
    events.extend([
        dispatch("MESSAGE_REACTION_ADD", reaction(OTHER)),
        dispatch("MESSAGE_REACTION_ADD", reaction(ME)),
        dispatch("TYPING_START", json!({"channel_id": GENERAL.to_string(), "user_id": OTHER.to_string()})),
        dispatch("PRESENCE_UPDATE", json!({"user": {"id": OTHER.to_string()}, "status": "idle",
                                           "custom_status": {"text": "afk"}})),
        dispatch("RELATIONSHIP_ADD", json!({"id": OTHER.to_string(), "type": 1, "since": "2025-01-01T00:00:00Z"})),
        dispatch("GUILD_MEMBER_LIST_UPDATE", json!({
            "guild_id": GUILD.to_string(), "id": "everyone", "channel_id": GENERAL.to_string(),
            "member_count": 2, "online_count": 1, "groups": [{"id": "online", "count": 1}],
            "ops": [{"op": "SYNC", "range": [0, 99], "items": [
                {"group": {"id": "online", "count": 1}},
                {"member": {"user": user_json(OTHER, "other"), "roles": [], "presence": {"status": "idle"}}},
            ]}],
        })),
    ]);

    // Fixed clock: typing expiry is part of the snapshot.
    let mut changes = Vec::new();
    for event in &events {
        changes.extend(state.apply_at(event, 1_750_000_000_000));
    }
    // Sort keys: serde_json's `preserve_order` is enabled by other workspace crates.
    insta::with_settings!({sort_maps => true}, {
        insta::assert_json_snapshot!("fixture_changes", changes);
        insta::assert_json_snapshot!("fixture_state", view(&state));
    });
}
