//! Reducer tests for the per-domain handlers: reactions, replies and
//! message content, typing, presence, relationships, settings, emojis and
//! lazy member lists.

use serde_json::{Value, json};

use super::*;

/// READY state with GENERAL loaded (messages mid(8)..=mid(10), by OTHER).
fn loaded_state() -> ClientState {
    let mut state = ready_state();
    state.messages.load(sf(GENERAL), page(GENERAL, 8, 11), LoadMode::Replace);
    state
}

fn window_message(state: &ClientState, id: Snowflake) -> &Message {
    state.messages.message(sf(GENERAL), id).expect("loaded message")
}

fn reaction_event(t: &str, user: u64, message: Snowflake, emoji: Value) -> Dispatch {
    dispatch(
        t,
        json!({"user_id": user.to_string(), "channel_id": GENERAL.to_string(), "message_id": message.to_string(),
               "guild_id": GUILD.to_string(), "emoji": emoji}),
    )
}

fn counts(state: &ClientState, id: Snowflake) -> Vec<(String, u32, bool)> {
    window_message(state, id)
        .reactions
        .iter()
        .map(|r| (r.emoji.name.clone(), r.count, r.me))
        .collect()
}

const CHANGED: StateChange = StateChange::MessagesChanged { channel_id: Snowflake(GENERAL) };

// ---- reactions ------------------------------------------------------------

#[test]
fn reaction_add_and_remove_track_count_and_me() {
    let mut state = loaded_state();
    let target = mid(9);
    let wave = json!({"name": "👋"});
    assert_eq!(state.apply(&reaction_event("MESSAGE_REACTION_ADD", OTHER, target, wave.clone())), vec![CHANGED]);
    assert_eq!(state.apply(&reaction_event("MESSAGE_REACTION_ADD", OWNER, target, wave.clone())), vec![CHANGED]);
    assert_eq!(state.apply(&reaction_event("MESSAGE_REACTION_ADD", ME, target, wave.clone())), vec![CHANGED]);
    assert_eq!(counts(&state, target), vec![("👋".to_owned(), 3, true)]);
    // Own echo is a no-op.
    assert!(state.apply(&reaction_event("MESSAGE_REACTION_ADD", ME, target, wave.clone())).is_empty());
    assert_eq!(counts(&state, target), vec![("👋".to_owned(), 3, true)]);

    assert_eq!(state.apply(&reaction_event("MESSAGE_REACTION_REMOVE", ME, target, wave.clone())), vec![CHANGED]);
    assert_eq!(counts(&state, target), vec![("👋".to_owned(), 2, false)]);
    assert!(state.apply(&reaction_event("MESSAGE_REACTION_REMOVE", ME, target, wave.clone())).is_empty());
    state.apply(&reaction_event("MESSAGE_REACTION_REMOVE", OTHER, target, wave.clone()));
    state.apply(&reaction_event("MESSAGE_REACTION_REMOVE", OWNER, target, wave));
    assert!(window_message(&state, target).reactions.is_empty());
}

#[test]
fn reaction_emoji_identity_is_id_and_name() {
    let mut state = loaded_state();
    let target = mid(10);
    state.apply(&reaction_event("MESSAGE_REACTION_ADD", OTHER, target, json!({"id": "77", "name": "blob"})));
    // `animated` does not make it a different emoji.
    state.apply(&reaction_event(
        "MESSAGE_REACTION_ADD",
        OWNER,
        target,
        json!({"id": "77", "name": "blob", "animated": true}),
    ));
    // Same name, no id: a different (unicode) reaction.
    state.apply(&reaction_event("MESSAGE_REACTION_ADD", OWNER, target, json!({"name": "blob"})));
    let reactions = &window_message(&state, target).reactions;
    assert_eq!(reactions.len(), 2);
    assert_eq!((reactions[0].emoji.id, reactions[0].count), (Some(sf(77)), 2));
    assert_eq!((reactions[1].emoji.id, reactions[1].count), (None, 1));

    let remove_emoji = dispatch(
        "MESSAGE_REACTION_REMOVE_EMOJI",
        json!({"channel_id": GENERAL.to_string(), "message_id": target.to_string(), "emoji": {"id": "77", "name": "blob"}}),
    );
    assert_eq!(state.apply(&remove_emoji), vec![CHANGED]);
    assert_eq!(counts(&state, target), vec![("blob".to_owned(), 1, false)]);

    let remove_all = dispatch(
        "MESSAGE_REACTION_REMOVE_ALL",
        json!({"channel_id": GENERAL.to_string(), "message_id": target.to_string()}),
    );
    assert_eq!(state.apply(&remove_all), vec![CHANGED]);
    assert!(window_message(&state, target).reactions.is_empty());
    assert!(state.apply(&remove_all).is_empty(), "nothing left to clear");
}

#[test]
fn reaction_add_many_applies_each_entry_and_hydrates_members() {
    let mut state = loaded_state();
    let target = mid(8);
    let event = dispatch(
        "MESSAGE_REACTION_ADD_MANY",
        json!({
            "channel_id": GENERAL.to_string(), "message_id": target.to_string(), "guild_id": GUILD.to_string(),
            "reactions": [
                {"user_id": "400", "emoji": {"name": "🔥"},
                 "member": {"user": user_json(400, "newcomer"), "roles": []}},
                {"user_id": ME.to_string(), "emoji": {"name": "🔥"}},
                {"user_id": "401", "emoji": {"name": "✅"}},
                {"emoji": {"name": "skipped: no user"}},
            ],
        }),
    );
    assert_eq!(state.apply(&event), vec![CHANGED]);
    assert_eq!(counts(&state, target), vec![("🔥".to_owned(), 2, true), ("✅".to_owned(), 1, false)]);
    assert!(state.members[&sf(GUILD)].contains_key(&sf(400)));
    assert_eq!(state.users[&sf(400)].username, "newcomer");
}

#[test]
fn reactions_on_unloaded_messages_are_ignored() {
    let mut state = loaded_state();
    let changes = state.apply(&reaction_event("MESSAGE_REACTION_ADD", OTHER, mid(99), json!({"name": "x"})));
    assert!(changes.is_empty());
}

#[test]
fn local_reaction_then_gateway_echo_counts_once() {
    let mut state = loaded_state();
    let target = mid(9);
    let emoji: PartialEmoji = serde_json::from_value(json!({"name": "👍"})).unwrap();
    assert_eq!(state.apply_local_reaction(sf(GENERAL), target, emoji.clone(), true), vec![CHANGED]);
    assert!(state.apply(&reaction_event("MESSAGE_REACTION_ADD", ME, target, json!({"name": "👍"}))).is_empty());
    assert_eq!(counts(&state, target), vec![("👍".to_owned(), 1, true)]);
    // REST failed: roll back.
    assert_eq!(state.apply_local_reaction(sf(GENERAL), target, emoji, false), vec![CHANGED]);
    assert!(window_message(&state, target).reactions.is_empty());
}

#[test]
fn wire_reactions_parse_with_null_me() {
    let mut json = message_json(mid(1), GENERAL, OTHER, "x");
    json["reactions"] = json!([{"emoji": {"id": null, "name": "🎉"}, "count": 4, "me": null}]);
    let m: Message = serde_json::from_value(json).unwrap();
    assert_eq!(m.reactions[0].count, 4);
    assert!(!m.reactions[0].me);
}

// ---- replies and content -------------------------------------------------

fn reply_json(id: Snowflake, target: Snowflake, referenced: Option<Value>) -> Value {
    let mut json = message_json(id, GENERAL, ME, "reply");
    json["type"] = json!(MESSAGE_TYPE_REPLY);
    json["message_reference"] = json!({"channel_id": GENERAL.to_string(), "message_id": target.to_string(), "type": 0});
    if let Some(referenced) = referenced {
        json["referenced_message"] = referenced;
    }
    json
}

#[test]
fn referenced_message_keeps_absent_apart_from_null() {
    let absent: Message = serde_json::from_value(reply_json(mid(30), mid(1), None)).unwrap();
    assert_eq!(absent.referenced_message, None);
    let gone: Message = serde_json::from_value(reply_json(mid(30), mid(1), Some(Value::Null))).unwrap();
    assert_eq!(gone.referenced_message, Some(None));
    let resolved: Message =
        serde_json::from_value(reply_json(mid(30), mid(1), Some(message_json(mid(1), GENERAL, OTHER, "orig")))).unwrap();
    let target = resolved.referenced_message.clone().flatten().unwrap();
    assert_eq!(target.content, "orig");
    assert_eq!(resolved.reply_to(), Some(mid(1)));

    // MESSAGE_UPDATE's overlay round trip keeps "absent" absent.
    let round_trip: Message = serde_json::from_value(serde_json::to_value(&absent).unwrap()).unwrap();
    assert_eq!(round_trip.referenced_message, None);
    let round_trip: Message = serde_json::from_value(serde_json::to_value(&gone).unwrap()).unwrap();
    assert_eq!(round_trip.referenced_message, Some(None));

    // A forward is not a reply.
    let mut forward = reply_json(mid(31), mid(1), None);
    forward["message_reference"]["type"] = json!(1);
    let forward: Message = serde_json::from_value(forward).unwrap();
    assert_eq!(forward.reply_to(), None);
}

#[test]
fn reply_preview_prefers_the_loaded_target_and_tracks_deletes() {
    let mut state = loaded_state();
    let stale = message_json(mid(9), GENERAL, OTHER, "stale copy");
    state.messages.load(
        sf(GENERAL),
        vec![
            serde_json::from_value(reply_json(mid(12), mid(9), Some(stale))).unwrap(),
            serde_json::from_value(reply_json(mid(11), mid(2), None)).unwrap(),
        ],
        LoadMode::After,
    );
    let reply = window_message(&state, mid(12)).clone();
    match state.reply_preview(&reply) {
        ReplyPreview::Loaded(m) => assert_eq!(m.content, "m9", "window copy wins over the embedded one"),
        other => panic!("unexpected {other:?}"),
    }
    let unresolved = window_message(&state, mid(11)).clone();
    assert_eq!(
        state.reply_preview(&unresolved),
        ReplyPreview::NotLoaded { channel_id: sf(GENERAL), message_id: mid(2) }
    );
    assert_eq!(state.reply_preview(window_message(&state, mid(10))), ReplyPreview::NotReply);

    // Deleting the target (loaded or not) marks replies as pointing at a deleted message.
    let delete = |id: Snowflake| dispatch("MESSAGE_DELETE", json!({"id": id.to_string(), "channel_id": GENERAL.to_string()}));
    assert_eq!(state.apply(&delete(mid(9))), vec![CHANGED]);
    let reply = window_message(&state, mid(12)).clone();
    assert_eq!(state.reply_preview(&reply), ReplyPreview::Deleted);
    assert_eq!(state.apply(&delete(mid(2))), vec![CHANGED]);
    let unresolved = window_message(&state, mid(11)).clone();
    assert_eq!(state.reply_preview(&unresolved), ReplyPreview::Deleted);
}

#[test]
fn full_embeds_attachments_and_stickers_parse() {
    let mut json = message_json(mid(1), GENERAL, OTHER, "content");
    json["flags"] = json!(message_flags::SUPPRESS_NOTIFICATIONS);
    json["mention_everyone"] = json!(true);
    json["mention_roles"] = json!([ROLE_MOD.to_string()]);
    json["edited_timestamp"] = json!("2025-06-15T00:01:00+00:00");
    json["attachments"] = json!([{
        "id": "5", "filename": "secret.png", "size": 1234, "url": "https://cdn/a.png",
        "proxy_url": "https://media/a.png", "content_type": "image/png", "width": 64, "height": 32,
        "flags": ATTACHMENT_FLAG_SPOILER, "placeholder": "abc", "description": null,
    }]);
    json["stickers"] = json!([{"id": "9", "name": "wave", "animated": false}]);
    json["embeds"] = json!([{
        "type": "rich", "url": "https://example.com", "title": "T", "description": "D", "color": 0x00ff_8800u32,
        "timestamp": "2025-06-15T00:00:00Z",
        "author": {"name": "A", "url": "https://a", "icon_url": "https://a/i.png", "proxy_icon_url": null},
        "image": {"url": "https://i/1.png", "proxy_url": "https://p/1.png", "width": 10, "height": 20, "flags": 0},
        "thumbnail": {"url": "https://i/t.png"},
        "video": {"url": "https://v/1.mp4", "content_type": "video/mp4", "duration": 3},
        "footer": {"text": "F", "icon_url": null},
        "fields": [{"name": "n1", "value": "v1", "inline": true}, {"name": "n2", "value": "v2", "inline": false}],
        "provider": {"name": "P", "url": null},
        "children": [{"type": "image", "image": {"url": "https://i/c.png"}}],
    }, {"type": "link", "fields": null}]);
    let m: Message = serde_json::from_value(json).unwrap();
    assert!(m.is_edited() && m.mention_everyone && !m.suppresses_embeds());
    assert_eq!(m.mention_roles, vec![sf(ROLE_MOD)]);
    let a = &m.attachments[0];
    assert!(a.is_spoiler());
    assert_eq!((a.proxy_url.as_deref(), a.size, a.width, a.height), (Some("https://media/a.png"), 1234, Some(64), Some(32)));
    assert_eq!(m.stickers[0].name, "wave");
    let e = &m.embeds[0];
    assert_eq!(e.color, Some(0x00ff_8800));
    assert_eq!(e.author.as_ref().map(|a| a.name.as_str()), Some("A"));
    assert_eq!(e.image.as_ref().map(|i| (i.width, i.height)), Some((Some(10), Some(20))));
    assert_eq!(e.thumbnail.as_ref().map(|t| t.url.as_str()), Some("https://i/t.png"));
    assert_eq!(e.video.as_ref().and_then(|v| v.duration), Some(3));
    assert_eq!(e.footer.as_ref().map(|f| f.text.as_str()), Some("F"));
    assert_eq!(e.fields.len(), 2);
    assert!(e.fields[0].inline);
    assert_eq!(e.provider.as_ref().map(|p| p.name.as_str()), Some("P"));
    assert_eq!(e.children[0].image.as_ref().map(|i| i.url.as_str()), Some("https://i/c.png"));
    assert!(m.embeds[1].fields.is_empty());
}

#[test]
fn message_update_merges_new_fields() {
    let mut state = loaded_state();
    let update = dispatch(
        "MESSAGE_UPDATE",
        json!({"id": mid(10).to_string(), "channel_id": GENERAL.to_string(),
               "embeds": [{"type": "link", "url": "https://x", "title": "X"}],
               "flags": message_flags::SUPPRESS_EMBEDS}),
    );
    assert_eq!(state.apply(&update), vec![CHANGED]);
    let m = window_message(&state, mid(10));
    assert_eq!(m.embeds[0].title.as_deref(), Some("X"));
    assert!(m.suppresses_embeds());
    assert_eq!(m.content, "m10", "fields not in the patch are kept");
}

// ---- typing ---------------------------------------------------------------

fn typing(user: u64, channel: u64) -> Dispatch {
    dispatch(
        "TYPING_START",
        json!({"channel_id": channel.to_string(), "user_id": user.to_string(), "timestamp": 1_750_000_000}),
    )
}

#[test]
fn typing_start_expires_after_ten_seconds() {
    let mut state = ready_state();
    let t0 = 1_000_000;
    let typing_changed = StateChange::TypingChanged { channel_id: sf(GENERAL) };
    assert_eq!(state.apply_at(&typing(OTHER, GENERAL), t0), vec![typing_changed.clone()]);
    assert_eq!(state.apply_at(&typing(OWNER, GENERAL), t0 + 1_000), vec![typing_changed.clone()]);
    assert!(state.apply_at(&typing(ME, GENERAL), t0).is_empty(), "self is never listed");
    assert_eq!(state.typing_users(sf(GENERAL), t0 + 1_000), vec![sf(OTHER), sf(OWNER)]);
    assert_eq!(state.next_typing_expiry_ms(), Some(t0 + TYPING_EXPIRY_MS));

    // A refresh keeps the order and moves the expiry, with no change event.
    assert!(state.apply_at(&typing(OTHER, GENERAL), t0 + 5_000).is_empty());
    assert_eq!(state.next_typing_expiry_ms(), Some(t0 + 1_000 + TYPING_EXPIRY_MS));

    // OWNER expires first; queries already hide expired typers.
    let later = t0 + 1_000 + TYPING_EXPIRY_MS;
    assert_eq!(state.typing_users(sf(GENERAL), later), vec![sf(OTHER)]);
    assert_eq!(state.expire_typing(later), vec![typing_changed.clone()]);
    assert!(state.expire_typing(later).is_empty());
    assert_eq!(state.expire_typing(t0 + 5_000 + TYPING_EXPIRY_MS), vec![typing_changed]);
    assert!(state.typing.is_empty());
    assert_eq!(state.next_typing_expiry_ms(), None);
}

#[test]
fn typing_stops_when_the_typer_sends_and_hydrates_members() {
    let mut state = loaded_state();
    let mut start = typing(400, GENERAL);
    start.d["guild_id"] = json!(GUILD.to_string());
    start.d["member"] = json!({"user": user_json(400, "typist"), "roles": []});
    state.apply_at(&start, 0);
    assert!(state.members[&sf(GUILD)].contains_key(&sf(400)));
    let changes = state.apply_at(&dispatch("MESSAGE_CREATE", message_json(mid(11), GENERAL, 400, "done")), 1);
    assert_eq!(changes[0], StateChange::TypingChanged { channel_id: sf(GENERAL) });
    assert!(state.typing_users(sf(GENERAL), 1).is_empty());
}

// ---- presence -------------------------------------------------------------

fn presence(user: u64, status: &str) -> Value {
    json!({"user": {"id": user.to_string()}, "status": status, "afk": false, "mobile": false})
}

#[test]
fn presences_from_ready_guilds_and_updates() {
    let mut payload = ready_payload();
    payload["presences"] = json!([presence(OTHER, "idle"), presence(ME, "online")]);
    payload["guilds"][0]["presences"] = json!([presence(OWNER, "dnd")]);
    let mut state = ClientState::new();
    state.apply(&dispatch("READY", payload));
    assert_eq!(state.presence_status(sf(OTHER)), Status::Idle);
    assert_eq!(state.presence_status(sf(OWNER)), Status::Dnd);
    assert!(!state.presences.contains_key(&sf(ME)), "own presence is not stored");
    assert!(state.presences[&sf(OWNER)].guild_ids.contains(&Some(sf(GUILD))));

    let mut update = presence(OTHER, "online");
    update["custom_status"] = json!({"text": "busy", "emoji_name": "🔥"});
    update["mobile"] = json!(true);
    assert_eq!(
        state.apply(&dispatch("PRESENCE_UPDATE", update.clone())),
        vec![StateChange::PresenceUpdated { user_id: sf(OTHER) }]
    );
    let p = &state.presences[&sf(OTHER)];
    assert_eq!((p.status, p.mobile), (Status::Online, true));
    assert_eq!(p.custom_status.as_ref().and_then(|c| c.text.as_deref()), Some("busy"));
    assert!(state.apply(&dispatch("PRESENCE_UPDATE", update)).is_empty(), "unchanged");

    assert_eq!(
        state.apply(&dispatch("PRESENCE_UPDATE", presence(OTHER, "offline"))),
        vec![StateChange::PresenceUpdated { user_id: sf(OTHER) }]
    );
    assert_eq!(state.presence_status(sf(OTHER)), Status::Offline);
    assert!(state.apply(&dispatch("PRESENCE_UPDATE", presence(ME, "dnd"))).is_empty());

    let bulk = dispatch(
        "PRESENCE_UPDATE_BULK",
        json!({"guild_id": "6000", "presences": [presence(500, "online"), presence(501, "bogus")]}),
    );
    assert_eq!(state.apply(&bulk), vec![StateChange::PresenceUpdated { user_id: sf(500) }]);
    assert!(state.presences[&sf(500)].guild_ids.contains(&Some(sf(6000))));
    assert_eq!(state.presence_status(sf(501)), Status::Offline, "unknown status normalises to offline");
}

// ---- relationships --------------------------------------------------------

#[test]
fn relationships_load_merge_and_remove() {
    let mut payload = ready_payload();
    payload["relationships"] = json!([
        {"id": "600", "type": 1, "user": user_json(600, "friend"), "since": "2025-01-01T00:00:00Z", "nickname": "pal"},
        {"id": "601", "type": 3, "user": user_json(601, "asker"), "since": "2025-01-02T00:00:00Z"},
    ]);
    let mut state = ClientState::new();
    state.apply(&dispatch("READY", payload));
    assert_eq!(state.relationship_kind(sf(600)), Some(RelationshipKind::Friend));
    assert_eq!(state.users[&sf(601)].username, "asker");
    assert_eq!(state.relationships_of(RelationshipKind::IncomingRequest).len(), 1);

    // Accepting the request: UPDATE carries only id + type; the rest is kept.
    let changes = state.apply(&dispatch("RELATIONSHIP_UPDATE", json!({"id": "601", "type": 1})));
    assert_eq!(changes, vec![StateChange::RelationshipChanged { user_id: sf(601) }]);
    assert_eq!(state.relationships[&sf(601)].since.as_deref(), Some("2025-01-02T00:00:00Z"));
    assert_eq!(state.relationships_of(RelationshipKind::Friend).len(), 2);

    state.apply(&dispatch("RELATIONSHIP_ADD", json!({"id": "602", "type": 2, "user": user_json(602, "troll")})));
    assert!(state.is_blocked(sf(602)));
    assert_eq!(
        state.apply(&dispatch("RELATIONSHIP_REMOVE", json!({"id": "600"}))),
        vec![StateChange::RelationshipChanged { user_id: sf(600) }]
    );
    assert!(state.apply(&dispatch("RELATIONSHIP_REMOVE", json!({"id": "600"}))).is_empty());
    assert_eq!(state.relationship_kind(sf(600)), None);
}

// ---- notes ----------------------------------------------------------------

#[test]
fn ready_notes_load_skips_invalid_keys_and_empty_notes() {
    let mut payload = ready_payload();
    payload["notes"] = json!({
        OTHER.to_string(): "remember",
        "not-a-number": "skipped",
        "999": "",
        "1000": "also kept",
    });
    let mut state = ClientState::new();
    state.apply(&dispatch("READY", payload));
    assert_eq!(state.note(sf(OTHER)), Some("remember"));
    assert_eq!(state.note(sf(999)), None);
    assert_eq!(state.note(sf(1000)), Some("also kept"));
}

#[test]
fn user_note_update_sets_clears_and_local_note_echoes() {
    let mut state = ready_state();
    assert_eq!(
        state.apply(&dispatch("USER_NOTE_UPDATE", json!({"id": OTHER.to_string(), "note": "hi"}))),
        vec![StateChange::NotesChanged]
    );
    assert_eq!(state.note(sf(OTHER)), Some("hi"));

    assert_eq!(
        state.apply(&dispatch("USER_NOTE_UPDATE", json!({"id": OTHER.to_string(), "note": null}))),
        vec![StateChange::NotesChanged]
    );
    assert_eq!(state.note(sf(OTHER)), None);

    state.set_local_note(sf(OTHER), Some("local".into()));
    assert_eq!(state.note(sf(OTHER)), Some("local"));
    state.set_local_note(sf(OTHER), Some(String::new()));
    assert_eq!(state.note(sf(OTHER)), None);

    assert!(state.apply(&dispatch("USER_NOTE_UPDATE", json!({"note": "no id"}))).is_empty());
}

// ---- settings -------------------------------------------------------------

#[test]
fn user_settings_typed_fields_and_partial_updates() {
    let mut payload = ready_payload();
    payload["user_settings"] = json!({
        "status": "dnd", "theme": "dark", "locale": "ko", "flags": 0,
        "custom_status": {"text": "hi"},
        "guild_folders": [{"guild_ids": [GUILD.to_string(), "6000"]}],
    });
    let mut state = ClientState::new();
    state.apply(&dispatch("READY", payload));
    let s = &state.user_settings;
    assert_eq!((s.status.as_deref(), s.theme.as_deref(), s.locale.as_deref()), (Some("dnd"), Some("dark"), Some("ko")));
    assert_eq!(s.custom_status.as_ref().and_then(|c| c.text.as_deref()), Some("hi"));
    assert_eq!(state.guild_order, vec![sf(GUILD), sf(6000)]);

    let changes = state.apply(&dispatch(
        "USER_SETTINGS_UPDATE",
        json!({"theme": "light", "custom_status": null, "guild_folders": [{"guild_ids": ["6000", GUILD.to_string()]}]}),
    ));
    assert_eq!(changes, vec![StateChange::UserSettingsChanged]);
    let s = &state.user_settings;
    assert_eq!((s.theme.as_deref(), s.locale.as_deref()), (Some("light"), Some("ko")), "locale kept");
    assert_eq!(s.custom_status, None);
    assert_eq!(s.raw["flags"], json!(0));
    assert_eq!(state.guild_order, vec![sf(6000), sf(GUILD)]);
}

#[test]
fn guild_settings_mutes_overrides_and_merge() {
    let mut payload = ready_payload();
    payload["user_guild_settings"] = json!([
        {"guild_id": GUILD.to_string(), "muted": true,
         "mute_config": {"end_time": "1970-01-01T00:00:10Z"}, "message_notifications": 1,
         "channel_overrides": [{"channel_id": STAFF.to_string(), "muted": true, "collapsed": false,
                                "message_notifications": 3, "mute_config": null}]},
        {"guild_id": null, "muted": false,
         "channel_overrides": {DM.to_string(): {"muted": true, "collapsed": null, "message_notifications": 2}}},
    ]);
    let mut state = ClientState::new();
    state.apply(&dispatch("READY", payload));
    assert!(state.is_guild_muted(Some(sf(GUILD)), 9_999));
    assert!(!state.is_guild_muted(Some(sf(GUILD)), 10_000), "timed mute ended");
    assert!(state.is_channel_muted(Some(sf(GUILD)), sf(STAFF), i64::MAX), "untimed override");
    assert!(state.is_channel_muted(None, sf(DM), 0), "DM settings keyed by None, record form");
    assert_eq!(state.guild_settings[&Some(sf(GUILD))].message_notifications, message_notifications::ONLY_MENTIONS);

    let changes = state.apply(&dispatch(
        "USER_GUILD_SETTINGS_UPDATE",
        json!({"guild_id": GUILD.to_string(), "muted": false, "mute_config": null}),
    ));
    assert_eq!(changes, vec![StateChange::GuildSettingsChanged { guild_id: Some(sf(GUILD)) }]);
    let g = &state.guild_settings[&Some(sf(GUILD))];
    assert!(!g.muted);
    assert_eq!(g.message_notifications, message_notifications::ONLY_MENTIONS, "merged, not replaced");
    assert!(g.channel_overrides.contains_key(&sf(STAFF)));
    assert!(!state.is_guild_muted(Some(sf(6000)), 0), "no settings = not muted");
}

// ---- emojis ---------------------------------------------------------------

#[test]
fn guild_emojis_load_update_and_drop() {
    let mut payload = ready_payload();
    payload["guilds"][0]["emojis"] = json!([{"id": "70", "name": "blob", "animated": false}]);
    let mut state = ClientState::new();
    state.apply(&dispatch("READY", payload));
    assert_eq!(state.guild_emojis(sf(GUILD))[0].name, "blob");
    assert!(state.guild_emojis(sf(6000)).is_empty());

    let update = dispatch(
        "GUILD_EMOJIS_UPDATE",
        json!({"guild_id": GUILD.to_string(), "emojis": [{"id": "71", "name": "party", "animated": true}]}),
    );
    assert_eq!(state.apply(&update), vec![StateChange::EmojisChanged { guild_id: sf(GUILD) }]);
    assert_eq!(state.find_emoji(sf(71)).map(|(g, e)| (g, e.animated)), Some((sf(GUILD), true)));
    assert!(state.find_emoji(sf(70)).is_none(), "list replaced");
    let unknown = dispatch("GUILD_EMOJIS_UPDATE", json!({"guild_id": "999", "emojis": []}));
    assert!(state.apply(&unknown).is_empty());

    state.apply(&dispatch("GUILD_DELETE", json!({"id": GUILD.to_string()})));
    assert!(state.guild_emojis(sf(GUILD)).is_empty());
}

// ---- lazy member list ---------------------------------------------------

fn list_member(user: u64, name: &str, status: &str) -> Value {
    json!({"member": {"user": user_json(user, name), "roles": [], "presence": {"status": status}}})
}

fn list_update(ops: Value, groups: Value) -> Dispatch {
    dispatch(
        "GUILD_MEMBER_LIST_UPDATE",
        json!({"guild_id": GUILD.to_string(), "id": "everyone", "channel_id": GENERAL.to_string(),
               "member_count": 3, "online_count": 2, "groups": groups, "ops": ops}),
    )
}

/// Rows as short strings: `g:<id>:<count>` or `m:<user id>`.
fn rows(state: &ClientState) -> Vec<(u32, String)> {
    let list = state.member_list(sf(GUILD), sf(GENERAL)).expect("list");
    list.rows
        .iter()
        .map(|(i, row)| {
            let label = match row {
                MemberListRow::Group(g) => format!("g:{}:{}", g.id, g.count),
                MemberListRow::Member(entry) => format!("m:{}", entry.member.user.id),
            };
            (*i, label)
        })
        .collect()
}

#[test]
fn member_list_sync_builds_rows_and_maps_the_channel() {
    let mut state = ready_state();
    let groups = json!([{"id": "online", "count": 2}, {"id": "offline", "count": 1}, {"id": "1001", "count": 0}]);
    let ops = json!([{"op": "SYNC", "range": [0, 99], "items": [
        {"group": {"id": "online", "count": 2}},
        list_member(OTHER, "other", "online"),
        list_member(700, "fresh", "idle"),
        {"group": {"id": "1001", "count": 0}},
        {"group": {"id": "offline", "count": 1}},
        list_member(OWNER, "owner", "offline"),
    ]}]);
    let changes = state.apply(&list_update(ops, groups));
    assert_eq!(
        changes,
        vec![StateChange::MemberListChanged { guild_id: sf(GUILD), list_id: "everyone".into(), channel_id: Some(sf(GENERAL)) }]
    );
    assert_eq!(
        rows(&state),
        vec![
            (0, "g:online:2".into()),
            (1, format!("m:{OTHER}")),
            (2, "m:700".into()),
            (3, "g:offline:1".into()),
            (4, format!("m:{OWNER}")),
        ]
    );
    let list = state.member_list(sf(GUILD), sf(GENERAL)).unwrap();
    assert_eq!((list.member_count, list.online_count, list.row_count()), (3, 2, 5));
    assert_eq!(state.member_lists.get(sf(GUILD), "everyone").map(|l| l.list_id.as_str()), Some("everyone"));
    match list.row(2) {
        Some(MemberListRow::Member(entry)) => {
            assert_eq!(entry.presence.as_ref().map(ListPresence::status), Some(Status::Idle));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(state.members[&sf(GUILD)].contains_key(&sf(700)), "hydrated");
    assert!(state.member_list(sf(GUILD), sf(STAFF)).is_none());
}

#[test]
fn member_list_insert_update_delete_invalidate() {
    let mut state = ready_state();
    let groups = json!([{"id": "online", "count": 3}]);
    state.apply(&list_update(
        json!([{"op": "SYNC", "range": [0, 99], "items": [
            {"group": {"id": "online", "count": 3}},
            list_member(701, "a", "online"),
            list_member(702, "b", "online"),
            list_member(703, "c", "online"),
        ]}]),
        groups.clone(),
    ));
    let groups4 = json!([{"id": "online", "count": 4}]);
    state.apply(&list_update(json!([{"op": "INSERT", "index": 2, "item": list_member(704, "d", "online")}]), groups4));
    assert_eq!(
        rows(&state),
        vec![(0, "g:online:3".into()), (1, "m:701".into()), (2, "m:704".into()), (3, "m:702".into()), (4, "m:703".into())]
    );
    state.apply(&list_update(
        json!([{"op": "UPDATE", "index": 0, "item": {"group": {"id": "online", "count": 4}}}]),
        json!([{"id": "online", "count": 4}]),
    ));
    state.apply(&list_update(json!([{"op": "DELETE", "index": 1}]), groups.clone()));
    assert_eq!(
        rows(&state),
        vec![(0, "g:online:4".into()), (1, "m:704".into()), (2, "m:702".into()), (3, "m:703".into())]
    );
    state.apply(&list_update(json!([{"op": "INVALIDATE", "range": [2, 3]}]), groups.clone()));
    assert_eq!(rows(&state), vec![(0, "g:online:4".into()), (1, "m:704".into())]);

    // Only invalid ops: the update is dropped (TS), counts untouched.
    let mut bad = list_update(json!([{"op": "SYNC", "range": [0, 500], "items": []}, {"op": "NOPE"}]), groups);
    bad.d["member_count"] = json!(99);
    assert!(state.apply(&bad).is_empty());
    assert_eq!(state.member_list(sf(GUILD), sf(GENERAL)).unwrap().member_count, 3);

    // Groups shrinking trims rows past the end.
    state.apply(&list_update(json!([]), json!([{"id": "online", "count": 0}])));
    assert!(rows(&state).is_empty());
}

#[test]
fn member_lists_are_dropped_with_their_guild() {
    let mut state = ready_state();
    state.apply(&list_update(
        json!([{"op": "SYNC", "range": [0, 0], "items": [list_member(OTHER, "other", "online")]}]),
        json!([]),
    ));
    assert!(state.member_list(sf(GUILD), sf(GENERAL)).is_some());
    state.apply(&dispatch("GUILD_DELETE", json!({"id": GUILD.to_string(), "unavailable": true})));
    assert!(state.member_list(sf(GUILD), sf(GENERAL)).is_none());
    // Lists of unknown guilds are ignored.
    assert!(state.apply(&list_update(json!([]), json!([]))).is_empty());
}

#[test]
fn group_dm_owner_rename_and_recipient_dispatches() {
    let mut state = ready_state();
    let gdm = sf(9001);
    let create = dispatch(
        "CHANNEL_CREATE",
        json!({
            "id": gdm.to_string(),
            "type": 3,
            "name": null,
            "owner_id": ME.to_string(),
            "recipients": [user_json(OTHER, "other")]
        }),
    );
    assert_eq!(
        state.apply(&create),
        vec![StateChange::ChannelUpserted { channel_id: gdm, guild_id: None }]
    );
    assert_eq!(state.channels[&gdm].owner_id, Some(sf(ME)));
    assert_eq!(state.channels[&gdm].recipients.len(), 1);

    let add = dispatch("CHANNEL_RECIPIENT_ADD", json!({ "channel_id": gdm.to_string(), "user": user_json(OWNER, "owner") }));
    assert_eq!(state.apply(&add), vec![StateChange::ChannelUpserted { channel_id: gdm, guild_id: None }]);
    assert_eq!(state.channels[&gdm].recipients.len(), 2);
    assert!(state.users.contains_key(&sf(OWNER)));
    // Duplicate add is a no-op.
    assert!(state.apply(&add).is_empty());

    let rename = dispatch("CHANNEL_UPDATE", json!({ "id": gdm.to_string(), "name": "Alpha" }));
    assert_eq!(state.apply(&rename), vec![StateChange::ChannelUpserted { channel_id: gdm, guild_id: None }]);
    assert_eq!(state.channels[&gdm].name.as_deref(), Some("Alpha"));

    let remove = dispatch("CHANNEL_RECIPIENT_REMOVE", json!({ "channel_id": gdm.to_string(), "user": { "id": OWNER.to_string() } }));
    assert_eq!(state.apply(&remove), vec![StateChange::ChannelUpserted { channel_id: gdm, guild_id: None }]);
    assert_eq!(state.channels[&gdm].recipients.len(), 1);
    assert!(state.apply(&remove).is_empty());
}
