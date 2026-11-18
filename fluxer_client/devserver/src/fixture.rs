//! Fixture data and mutable server state: users, guilds, channels, message
//! history, read states, tokens and gateway sessions.

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// Fluxer epoch in Unix ms (`packages/constants/src/Core.ts`).
const FLUXER_EPOCH_MS: u64 = 1_420_070_400_000;
/// 2025-01-01T00:00:00Z: creation time of the fixture entities.
const ENTITY_EPOCH_MS: u64 = 1_735_689_600_000;
/// Spacing of generated history messages.
const HISTORY_STEP_MS: u64 = 90_000;

/// Accepted by every account and printed at startup, so the app can be run
/// with `--token-env` and skip the login screen.
pub const DEV_TOKEN: &str = "flx_devserverfixturetoken000000000000000";
pub const PASSWORD: &str = "password";
pub const EMAIL_USER: &str = "user@test";
pub const EMAIL_MFA: &str = "mfa@test";
pub const EMAIL_CAPTCHA: &str = "captcha@test";
pub const MFA_CODE: &str = "123456";

/// Permission bits (`ChannelConstants.ts`).
mod perm {
    pub const CREATE_INSTANT_INVITE: u64 = 1 << 0;
    pub const KICK_MEMBERS: u64 = 1 << 1;
    pub const ADD_REACTIONS: u64 = 1 << 6;
    pub const VIEW_CHANNEL: u64 = 1 << 10;
    pub const SEND_MESSAGES: u64 = 1 << 11;
    pub const MANAGE_MESSAGES: u64 = 1 << 13;
    pub const EMBED_LINKS: u64 = 1 << 14;
    pub const ATTACH_FILES: u64 = 1 << 15;
    pub const READ_MESSAGE_HISTORY: u64 = 1 << 16;
    pub const USE_EXTERNAL_EMOJIS: u64 = 1 << 18;
    pub const CONNECT: u64 = 1 << 20;
    pub const SPEAK: u64 = 1 << 21;
    pub const CHANGE_NICKNAME: u64 = 1 << 26;
}

const EVERYONE_PERMISSIONS: u64 = perm::CREATE_INSTANT_INVITE
    | perm::ADD_REACTIONS
    | perm::VIEW_CHANNEL
    | perm::SEND_MESSAGES
    | perm::EMBED_LINKS
    | perm::ATTACH_FILES
    | perm::READ_MESSAGE_HISTORY
    | perm::USE_EXTERNAL_EMOJIS
    | perm::CONNECT
    | perm::SPEAK
    | perm::CHANGE_NICKNAME;

const MIXED_TEMPLATES: &[&str] = &[
    "Good morning everyone! Message #{n}",
    "Has anyone tried the new **Rust client** build yet? #{n}",
    "Here is the snippet: `cargo test -p fluxer_client_core` (#{n})",
    "```rust\nfn main() {\n    println!(\"hello from #{n}\");\n}\n```",
    "> quoting the spec: *heartbeat every 41.25 s*\nagreed (#{n})",
    "- item one\n- item two\n- item three (#{n})",
    "Check https://fluxer.app for details #{n}",
    "||spoiler|| alert, sorry #{n}",
    "안녕하세요! 오늘 회의는 오후 3시입니다. (#{n})",
    "Mixed 텍스트 with __underline__ and ~~strike~~ #{n}",
    "This is a deliberately longer message to exercise wrapping in the message list. It keeps going for a while so the layout has to break it over several lines, which is exactly what we want to see. #{n}",
    "lol 😂 #{n}",
    "# Heading\nbody text under a heading #{n}",
    "ok #{n}",
];

const KOREAN_TEMPLATES: &[&str] = &[
    "안녕하세요, 반갑습니다! ({n}번째 메시지)",
    "오늘 날씨가 정말 좋네요. 점심은 뭐 드실 건가요? ({n})",
    "**굵은 글씨**와 *기울임*도 잘 보이나요? ({n})",
    "한글 입력기(IME) 조합 테스트: 가나다라마바사 ({n})",
    "`코드 블록` 안의 한글도 확인해 주세요 ({n})",
    "> 인용문: 천 리 길도 한 걸음부터\n좋은 말이네요 ({n})",
    "긴 문장 줄바꿈 테스트입니다. 이 메시지는 일부러 길게 작성해서 메시지 목록에서 여러 줄로 나뉘어 표시되는지 확인하기 위한 것입니다. ({n})",
    "ㅋㅋㅋㅋ 😄 ({n})",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Templates {
    Mixed,
    Korean,
}

#[derive(Debug, Clone, Default)]
pub struct ReadState {
    pub last_message_id: Option<u64>,
    pub mention_count: u32,
    pub version: u64,
}

pub struct Guild {
    pub id: u64,
    pub properties: Value,
    pub roles: Vec<Value>,
    pub members: Vec<Value>,
    pub channel_ids: Vec<u64>,
}

pub struct State {
    pub me: Value,
    pub me_id: u64,
    pub others: Vec<Value>,
    pub guilds: Vec<Guild>,
    /// Channel JSON (guild channels carry `guild_id`); `last_message_id`
    /// is kept current.
    pub channels: BTreeMap<u64, Value>,
    pub dm_ids: Vec<u64>,
    /// Channels where `@everyone` is denied SEND_MESSAGES.
    pub send_denied: HashSet<u64>,
    /// Message JSON per channel, ascending by id.
    pub messages: HashMap<u64, Vec<(u64, Value)>>,
    pub read_states: BTreeMap<u64, ReadState>,
    pub tokens: HashSet<String>,
    pub mfa_tickets: HashSet<String>,
    pub sessions: HashSet<String>,
    /// Channels the `--chatter` task posts into.
    pub chatter_channels: Vec<u64>,
    pub chatter_count: u64,
    last_id: u64,
    secret_counter: u64,
    random: RandomState,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(FLUXER_EPOCH_MS, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn snowflake(unix_ms: u64, seq: u64) -> u64 {
    (unix_ms.saturating_sub(FLUXER_EPOCH_MS) << 22) | (seq & 0x3f_ffff)
}

fn snowflake_ms(id: u64) -> u64 {
    (id >> 22) + FLUXER_EPOCH_MS
}

/// `2026-11-18T12:34:56.789000+00:00` (server message timestamp format).
pub fn iso8601(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let millis = unix_ms % 1000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}000+00:00",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn user(id: u64, username: &str, global_name: Option<&str>, bot: bool) -> Value {
    json!({
        "id": id.to_string(),
        "username": username,
        "discriminator": "0000",
        "global_name": global_name,
        "avatar": null,
        "avatar_color": null,
        "bot": bot,
        "system": false,
        "flags": 0,
    })
}

fn role(id: u64, name: &str, color: u32, position: i32, permissions: u64, hoist: bool) -> Value {
    json!({
        "id": id.to_string(),
        "name": name,
        "color": color,
        "position": position,
        "hoist_position": null,
        "permissions": permissions.to_string(),
        "hoist": hoist,
        "mentionable": false,
        "unicode_emoji": null,
        "icon": null,
    })
}

fn member(user: &Value, roles: &[u64]) -> Value {
    json!({
        "user": user,
        "nick": null,
        "avatar": null,
        "banner": null,
        "roles": roles.iter().map(u64::to_string).collect::<Vec<_>>(),
        "joined_at": iso8601(ENTITY_EPOCH_MS),
        "mute": false,
        "deaf": false,
        "communication_disabled_until": null,
    })
}

fn guild_channel(id: u64, guild_id: u64, kind: u16, name: &str, position: i32, parent: Option<u64>) -> Value {
    json!({
        "id": id.to_string(),
        "guild_id": guild_id.to_string(),
        "type": kind,
        "name": name,
        "topic": null,
        "position": position,
        "parent_id": parent.map(|p| p.to_string()),
        "last_message_id": null,
        "permission_overwrites": [],
        "nsfw": false,
        "rate_limit_per_user": 0,
    })
}

/// Builds a message object (`MessageResponseSchemas.ts` subset).
pub fn message_json(id: u64, channel_id: u64, author: &Value, content: &str, mentions: Vec<Value>, nonce: Option<&str>) -> Value {
    let mut message = json!({
        "id": id.to_string(),
        "channel_id": channel_id.to_string(),
        "author": author,
        "type": 0,
        "flags": 0,
        "content": content,
        "timestamp": iso8601(snowflake_ms(id)),
        "edited_timestamp": null,
        "pinned": false,
        "mention_everyone": false,
        "tts": false,
        "mentions": mentions,
        "mention_roles": [],
        "embeds": [],
        "attachments": [],
        "stickers": [],
        "reactions": [],
        "message_reference": null,
    });
    if let Some(nonce) = nonce {
        message["nonce"] = json!(nonce);
    }
    message
}

/// History spec for one channel.
struct History {
    channel_id: u64,
    count: usize,
    templates: Templates,
    /// Messages after the read marker (0 = fully read, `None` = no read state).
    unread_tail: Option<usize>,
    /// Indices (from the start) whose message mentions the current user.
    mention_every: Option<usize>,
    /// Author pool: indices into `others`, plus the current user when `true`.
    with_me: bool,
}

impl State {
    pub fn new() -> Self {
        let me_id = snowflake(ENTITY_EPOCH_MS, 1);
        let me = user(me_id, "devuser", Some("Dev User"), false);
        let alice = user(snowflake(ENTITY_EPOCH_MS, 2), "alice", Some("Alice"), false);
        let bob = user(snowflake(ENTITY_EPOCH_MS, 3), "bob", None, false);
        let minsu = user(snowflake(ENTITY_EPOCH_MS, 4), "minsu", Some("김민수"), false);
        let bot = user(snowflake(ENTITY_EPOCH_MS, 5), "fixturebot", Some("Fixture Bot"), true);
        let alice_id = snowflake(ENTITY_EPOCH_MS, 2);
        let others = vec![alice.clone(), bob.clone(), minsu.clone(), bot.clone()];

        let id = |seq: u64| snowflake(ENTITY_EPOCH_MS + 1000, seq);
        let mut state = Self {
            me: me.clone(),
            me_id,
            others,
            guilds: Vec::new(),
            channels: BTreeMap::new(),
            dm_ids: Vec::new(),
            send_denied: HashSet::new(),
            messages: HashMap::new(),
            read_states: BTreeMap::new(),
            tokens: HashSet::from([DEV_TOKEN.to_owned()]),
            mfa_tickets: HashSet::new(),
            sessions: HashSet::new(),
            chatter_channels: Vec::new(),
            chatter_count: 0,
            last_id: 0,
            secret_counter: 0,
            random: RandomState::new(),
        };

        // Guild A: categories, a read-only announcements channel, Korean channel.
        let ga = id(10);
        let (cat_text, cat_kr) = (id(11), id(12));
        let (welcome, announcements, general, random, korean, lounge) = (id(13), id(14), id(15), id(16), id(17), id(18));
        let (mod_role, tester_role) = (id(19), id(20));
        let mut announcements_json = guild_channel(announcements, ga, 0, "announcements", 1, Some(cat_text));
        announcements_json["topic"] = json!("Read-only: @everyone cannot send here");
        announcements_json["permission_overwrites"] = json!([{
            "id": ga.to_string(),
            "type": 0,
            "allow": "0",
            "deny": perm::SEND_MESSAGES.to_string(),
        }]);
        let mut general_json = guild_channel(general, ga, 0, "general", 2, Some(cat_text));
        general_json["topic"] = json!("Long history for paging (400 messages)");
        let channels_a = vec![
            guild_channel(welcome, ga, 0, "welcome", 0, None),
            guild_channel(cat_text, ga, 4, "Text Channels", 1, None),
            announcements_json,
            general_json,
            guild_channel(random, ga, 0, "random", 3, Some(cat_text)),
            guild_channel(cat_kr, ga, 4, "커뮤니티", 2, None),
            guild_channel(korean, ga, 0, "한국어", 0, Some(cat_kr)),
            guild_channel(lounge, ga, 2, "Lounge", 1, Some(cat_kr)),
        ];
        state.send_denied.insert(announcements);
        state.add_guild(
            ga,
            "Fluxer Dev",
            alice_id,
            vec![
                role(ga, "@everyone", 0, 0, EVERYONE_PERMISSIONS, false),
                role(mod_role, "Moderator", 0x3498db, 2, perm::MANAGE_MESSAGES | perm::KICK_MEMBERS, true),
                role(tester_role, "Tester", 0x2ecc71, 1, 0, false),
            ],
            vec![
                member(&me, &[tester_role]),
                member(&alice, &[mod_role]),
                member(&bob, &[]),
                member(&minsu, &[tester_role]),
                member(&bot, &[]),
            ],
            channels_a,
        );

        // Guild B: one unread channel with mentions.
        let gb = id(30);
        let (lobby, offtopic) = (id(31), id(32));
        state.add_guild(
            gb,
            "Rust Port Lab",
            alice_id,
            vec![role(gb, "@everyone", 0, 0, EVERYONE_PERMISSIONS, false)],
            vec![member(&me, &[]), member(&alice, &[]), member(&bob, &[]), member(&minsu, &[])],
            vec![
                guild_channel(lobby, gb, 0, "lobby", 0, None),
                guild_channel(offtopic, gb, 0, "off-topic", 1, None),
            ],
        );

        // DMs.
        let (dm_alice, dm_minsu) = (id(40), id(41));
        for (dm, recipient) in [(dm_alice, &alice), (dm_minsu, &minsu)] {
            state.channels.insert(
                dm,
                json!({
                    "id": dm.to_string(),
                    "type": 1,
                    "recipients": [recipient],
                    "last_message_id": null,
                    "flags": 0,
                }),
            );
            state.dm_ids.push(dm);
        }

        let histories = [
            History { channel_id: welcome, count: 15, templates: Templates::Mixed, unread_tail: Some(0), mention_every: None, with_me: false },
            History { channel_id: announcements, count: 25, templates: Templates::Mixed, unread_tail: Some(3), mention_every: None, with_me: false },
            History { channel_id: general, count: 400, templates: Templates::Mixed, unread_tail: Some(12), mention_every: None, with_me: true },
            History { channel_id: random, count: 80, templates: Templates::Mixed, unread_tail: Some(0), mention_every: None, with_me: true },
            History { channel_id: korean, count: 120, templates: Templates::Korean, unread_tail: Some(5), mention_every: None, with_me: true },
            History { channel_id: lobby, count: 60, templates: Templates::Mixed, unread_tail: Some(0), mention_every: None, with_me: true },
            History { channel_id: offtopic, count: 90, templates: Templates::Mixed, unread_tail: Some(6), mention_every: Some(43), with_me: true },
            History { channel_id: dm_alice, count: 40, templates: Templates::Mixed, unread_tail: Some(1), mention_every: None, with_me: true },
            History { channel_id: dm_minsu, count: 20, templates: Templates::Korean, unread_tail: Some(0), mention_every: None, with_me: true },
        ];
        let now = now_ms();
        for (index, history) in histories.iter().enumerate() {
            state.generate_history(history, index as u64, now);
        }
        state.chatter_channels = vec![general, random, offtopic, korean, dm_alice];
        state
    }

    fn add_guild(&mut self, id: u64, name: &str, owner: u64, roles: Vec<Value>, members: Vec<Value>, channels: Vec<Value>) {
        let mut channel_ids = Vec::new();
        for channel in channels {
            let Some(channel_id) = channel["id"].as_str().and_then(|s| s.parse::<u64>().ok()) else {
                continue;
            };
            channel_ids.push(channel_id);
            self.channels.insert(channel_id, channel);
        }
        let member_count = members.len();
        self.guilds.push(Guild {
            id,
            properties: json!({
                "id": id.to_string(),
                "name": name,
                "icon": null,
                "banner": null,
                "splash": null,
                "owner_id": owner.to_string(),
                "features": [],
                "verification_level": 0,
                "mfa_level": 0,
                "nsfw_level": 0,
                "explicit_content_filter": 0,
                "default_message_notifications": 0,
                "system_channel_id": null,
                "afk_channel_id": null,
                "afk_timeout": 300,
                "member_count": member_count,
            }),
            roles,
            members,
            channel_ids,
        });
    }

    /// Author for history message `i`; the tail is never authored by the
    /// current user (that would ack the channel).
    fn history_author(&self, history: &History, i: usize, is_dm: bool) -> Value {
        let in_tail = history.unread_tail.is_some_and(|tail| i + tail >= history.count);
        if history.with_me && !in_tail && i % 4 == 3 {
            return self.me.clone();
        }
        if is_dm {
            // DM recipients: the channel's single recipient.
            if let Some(recipient) = self.channels.get(&history.channel_id).map(|c| c["recipients"][0].clone()) {
                return recipient;
            }
        }
        self.others[i % 3].clone()
    }

    fn generate_history(&mut self, history: &History, channel_seq: u64, now: u64) {
        let is_dm = self.dm_ids.contains(&history.channel_id);
        let start = now.saturating_sub(10 * 60_000 + HISTORY_STEP_MS * history.count as u64);
        let mut list = Vec::with_capacity(history.count);
        let mut mentions_in_tail = 0u32;
        for i in 0..history.count {
            let id = snowflake(start + HISTORY_STEP_MS * i as u64, channel_seq);
            let author = self.history_author(history, i, is_dm);
            let template = match history.templates {
                Templates::Mixed => MIXED_TEMPLATES[(i * 7 + channel_seq as usize) % MIXED_TEMPLATES.len()],
                Templates::Korean => KOREAN_TEMPLATES[(i * 5 + channel_seq as usize) % KOREAN_TEMPLATES.len()],
            };
            let mut content = template.replace("{n}", &(i + 1).to_string());
            let mut mentions = Vec::new();
            // Every `mention_every`-th message, plus the newest one, mentions me.
            let mentions_me = history
                .mention_every
                .is_some_and(|every| i % every == every - 1 || i + 1 == history.count);
            if mentions_me {
                content = format!("<@{}> {content}", self.me_id);
                mentions.push(self.me.clone());
            }
            let in_tail = history.unread_tail.is_some_and(|tail| i + tail >= history.count);
            if in_tail && (mentions_me || is_dm) {
                mentions_in_tail += 1;
            }
            list.push((id, message_json(id, history.channel_id, &author, &content, mentions, None)));
        }
        if let Some((last_id, _)) = list.last() {
            let last_id = *last_id;
            self.last_id = self.last_id.max(last_id);
            if let Some(channel) = self.channels.get_mut(&history.channel_id) {
                channel["last_message_id"] = json!(last_id.to_string());
            }
        }
        if let Some(tail) = history.unread_tail {
            let ack = list.len().checked_sub(tail + 1).and_then(|i| list.get(i)).map(|(id, _)| *id);
            self.read_states.insert(
                history.channel_id,
                ReadState { last_message_id: ack, mention_count: mentions_in_tail, version: 1 },
            );
        }
        self.messages.insert(history.channel_id, list);
    }

    /// Monotonic snowflake for new messages.
    pub fn next_id(&mut self) -> u64 {
        self.last_id = snowflake(now_ms(), 0).max(self.last_id + 1);
        self.last_id
    }

    /// Unguessable-enough hex for tokens, tickets and session ids (dev only).
    pub fn secret(&mut self, len: usize) -> String {
        let mut out = String::new();
        while out.len() < len {
            self.secret_counter += 1;
            let mut hasher = self.random.build_hasher();
            hasher.write_u64(self.secret_counter);
            hasher.write_u64(now_ms());
            out.push_str(&format!("{:016x}", hasher.finish()));
        }
        out.truncate(len);
        out
    }

    pub fn new_token(&mut self) -> String {
        let token = format!("flx_{}", self.secret(36));
        self.tokens.insert(token.clone());
        token
    }

    pub fn guild_id_of(&self, channel_id: u64) -> Option<u64> {
        self.channels
            .get(&channel_id)?
            .get("guild_id")?
            .as_str()?
            .parse()
            .ok()
    }

    /// Stores a new message, bumps `last_message_id` and the current user's
    /// read state, and returns the gateway MESSAGE_CREATE payload.
    pub fn post_message(&mut self, channel_id: u64, author: &Value, content: &str, mentions: Vec<Value>, nonce: Option<&str>) -> (Value, Value) {
        let id = self.next_id();
        let message = message_json(id, channel_id, author, content, mentions, nonce);
        self.messages.entry(channel_id).or_default().push((id, message.clone()));
        if let Some(channel) = self.channels.get_mut(&channel_id) {
            channel["last_message_id"] = json!(id.to_string());
        }
        let own = author["id"].as_str() == Some(self.me_id.to_string().as_str());
        let mentions_me = message["mentions"]
            .as_array()
            .is_some_and(|m| m.iter().any(|u| u["id"].as_str() == Some(self.me_id.to_string().as_str())));
        let is_dm = self.dm_ids.contains(&channel_id);
        let rs = self.read_states.entry(channel_id).or_default();
        if own {
            rs.last_message_id = Some(id);
            rs.mention_count = 0;
            rs.version += 1;
        } else if mentions_me || is_dm {
            rs.mention_count += 1;
        }
        let mut event = message.clone();
        if let Some(guild_id) = self.guild_id_of(channel_id) {
            event["guild_id"] = json!(guild_id.to_string());
        }
        (message, event)
    }

    pub fn read_state_json(&self, channel_id: u64) -> Value {
        let rs = self.read_states.get(&channel_id).cloned().unwrap_or_default();
        json!({
            "id": channel_id.to_string(),
            "mention_count": rs.mention_count,
            "last_message_id": rs.last_message_id.map(|id| id.to_string()),
            "last_pin_timestamp": null,
            "version": rs.version.to_string(),
        })
    }

    /// `UserPrivate` for `/users/@me` and READY.
    pub fn me_private(&self) -> Value {
        let mut me = self.me.clone();
        for (key, value) in [
            ("email", json!(EMAIL_USER)),
            ("verified", json!(true)),
            ("mfa_enabled", json!(false)),
            ("phone", json!(null)),
            ("locale", json!("ko")),
            ("premium_type", json!(0)),
            ("nsfw_allowed", json!(false)),
            ("bio", json!("Fixture account served by fluxer_devserver")),
        ] {
            me[key] = value;
        }
        me
    }

    pub fn ready_payload(&self, session_id: &str) -> Value {
        let guilds: Vec<Value> = self
            .guilds
            .iter()
            .map(|g| {
                let channels: Vec<&Value> = g.channel_ids.iter().filter_map(|id| self.channels.get(id)).collect();
                json!({
                    "id": g.id.to_string(),
                    "properties": g.properties,
                    "channels": channels,
                    "threads": [],
                    "emojis": [],
                    "stickers": [],
                    "members": g.members,
                    "member_count": g.members.len(),
                    "online_count": 1,
                    "presences": [],
                    "voice_states": [],
                    "roles": g.roles,
                    "joined_at": iso8601(ENTITY_EPOCH_MS),
                })
            })
            .collect();
        let private_channels: Vec<&Value> = self.dm_ids.iter().filter_map(|id| self.channels.get(id)).collect();
        let read_states: Vec<Value> = self.read_states.keys().map(|id| self.read_state_json(*id)).collect();
        json!({
            "v": 1,
            "session_id": session_id,
            "user": self.me_private(),
            "users": self.others,
            "guilds": guilds,
            "private_channels": private_channels,
            "read_states": read_states,
            "relationships": [],
            "presences": [],
            "user_settings": {
                "status": "online",
                "locale": "ko",
                "theme": "dark",
                "guild_folders": [{
                    "id": null,
                    "name": null,
                    "color": null,
                    "guild_ids": self.guilds.iter().map(|g| g.id.to_string()).collect::<Vec<_>>(),
                }],
            },
            "user_guild_settings": [],
            "notes": {},
            "pinned_dms": [],
            "favorite_memes": [],
            "rtc_regions": [],
            "sessions": [],
            "country_code": "KR",
        })
    }
}
