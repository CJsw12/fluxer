//! Shared test fixture: one guild with a category, text/voice/announcement
//! channels, a DM, a group DM and a few users.

use serde_json::{Value, json};

pub use crate::state::bits;
use crate::state::{ClientState, Snowflake};
use crate::Dispatch;

pub const ME: u64 = 100;
pub const OTHER: u64 = 200; // "bob", mod role
pub const CAROL: u64 = 201; // friend, group DM member
pub const DAVE: u64 = 202; // plain user, no relationship
pub const OWNER: u64 = 300;
pub const GUILD: u64 = 1000;
pub const ROLE_MOD: u64 = 1001;
pub const ROLE_HELPER: u64 = 1002;
pub const CATEGORY: u64 = 2000;
pub const GENERAL: u64 = 2001;
pub const STAFF: u64 = 2002;
pub const ANNOUNCE: u64 = 2003;
pub const VOICE: u64 = 2004;
pub const DM: u64 = 3000;
pub const GROUP_DM: u64 = 3001;
pub const GUILD2: u64 = 6000;

pub fn sf(v: u64) -> Snowflake {
    Snowflake(v)
}

pub fn mid(n: u64) -> Snowflake {
    Snowflake::from_unix_ms(1_750_000_000_000 + n * 1000)
}

pub fn dispatch(t: &str, d: Value) -> Dispatch {
    Dispatch { t: t.to_owned(), s: 0, d }
}

pub fn user_json(id: u64, name: &str) -> Value {
    json!({"id": id.to_string(), "username": name, "discriminator": "0000", "global_name": null, "avatar": null})
}

fn role_json(id: u64, name: &str, position: i32, permissions: u64, mentionable: bool) -> Value {
    json!({"id": id.to_string(), "name": name, "position": position, "permissions": permissions.to_string(), "mentionable": mentionable})
}

pub fn fixture_state() -> ClientState {
    let ready = json!({
        "session_id": "s",
        "user": user_json(ME, "me"),
        "users": [user_json(OTHER, "bob"), user_json(CAROL, "carol"), user_json(DAVE, "dave")],
        "relationships": [{"id": CAROL.to_string(), "type": 1, "since": "2024-01-01T00:00:00+00:00", "user": user_json(CAROL, "carol")}],
        "guilds": [
            {
                "id": GUILD.to_string(),
                "properties": {"name": "Alpha", "owner_id": OWNER.to_string()},
                "joined_at": "2015-01-01T00:00:00+00:00",
                "roles": [
                    role_json(GUILD, "@everyone", 0, bits::DEFAULT, false),
                    role_json(ROLE_MOD, "mod", 2, 0, true),
                    role_json(ROLE_HELPER, "helper", 1, 0, false),
                ],
                "members": [
                    {"user": user_json(ME, "me"), "roles": [ROLE_HELPER.to_string()], "joined_at": "2015-01-01T00:00:00+00:00"},
                    {"user": user_json(OTHER, "bob"), "nick": "Bobby", "roles": [ROLE_MOD.to_string()], "joined_at": "2016-01-01T00:00:00+00:00"},
                    {"user": user_json(CAROL, "carol"), "roles": [], "joined_at": "2017-01-01T00:00:00+00:00"},
                ],
                "channels": [
                    {"id": CATEGORY.to_string(), "type": 4, "name": "Text", "position": 0},
                    {"id": GENERAL.to_string(), "type": 0, "name": "general", "position": 1, "parent_id": CATEGORY.to_string(),
                     "last_message_id": mid(10).to_string()},
                    {"id": STAFF.to_string(), "type": 0, "name": "staff-room", "position": 0, "parent_id": CATEGORY.to_string(),
                     "last_message_id": mid(5).to_string(),
                     "permission_overwrites": [
                        {"id": GUILD.to_string(), "type": 0, "allow": "0", "deny": bits::VIEW_CHANNEL.to_string()},
                        {"id": ROLE_MOD.to_string(), "type": 0, "allow": bits::VIEW_CHANNEL.to_string(), "deny": "0"}]},
                    {"id": ANNOUNCE.to_string(), "type": 5, "name": "announcements", "position": 2,
                     "last_message_id": mid(3).to_string()},
                    {"id": VOICE.to_string(), "type": 2, "name": "Voice Lounge", "position": 3},
                ],
                "emojis": [{"id": "9001", "name": "partyparrot", "animated": true}, {"id": "9002", "name": "pepe", "animated": false}],
            },
            {
                "id": GUILD2.to_string(),
                "properties": {"name": "Beta Club", "owner_id": ME.to_string()},
                "joined_at": "2020-01-01T00:00:00+00:00",
                "roles": [role_json(GUILD2, "@everyone", 0, bits::DEFAULT, false)],
                "members": [],
                "channels": [{"id": "6001", "type": 0, "name": "lobby", "position": 0}],
                "emojis": [{"id": "9003", "name": "betaemoji", "animated": false}],
            },
        ],
        "private_channels": [
            {"id": DM.to_string(), "type": 1, "recipients": [user_json(OTHER, "bob")], "last_message_id": mid(20).to_string()},
            {"id": GROUP_DM.to_string(), "type": 3, "name": "Study Group",
             "recipients": [user_json(OTHER, "bob"), user_json(CAROL, "carol")], "last_message_id": mid(15).to_string()},
        ],
        "read_states": [
            {"id": GENERAL.to_string(), "last_message_id": mid(8).to_string(), "mention_count": 1, "version": "5"},
            {"id": ANNOUNCE.to_string(), "last_message_id": mid(3).to_string(), "mention_count": 0},
            {"id": DM.to_string(), "last_message_id": mid(20).to_string(), "mention_count": 0},
            {"id": GROUP_DM.to_string(), "last_message_id": mid(15).to_string(), "mention_count": 0},
        ],
    });
    let mut state = ClientState::new();
    state.apply(&dispatch("READY", ready));
    state
}
