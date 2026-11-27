//! Headless UI tests for the avatar/name profile popout.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{GuildMemberSummary, MutualGuild, ProfileUser, UserProfileData, UserProfileFull};
use fluxer_client_core::state::{LoadMode, Message, Snowflake, bits};
use gpui_kit::{Focusable as _, Modifiers, TestAppContext, VisualTestContext, point, px};
use serde_json::json;

use crate::backend::{Backend, UiEvent, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{draw, msg_id, offline_rest, open_client, runtime};
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const GUILD: Snowflake = Snowflake(1);
const SECOND_GUILD: Snowflake = Snowflake(2);
const GENERAL: Snowflake = Snowflake(11);
const ALICE: Snowflake = Snowflake(201);
const BOB_BLOCKED: Snowflake = Snowflake(202);
const ROLE_MOD: Snowflake = Snowflake(50);

fn profile_ready() -> Dispatch {
    let perms = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": "100", "username": "me" },
            "notes": { ALICE.to_string(): "initial note" },
            "relationships": [
                { "id": BOB_BLOCKED.to_string(), "type": 2 }
            ],
            "guilds": [
                {
                    "id": GUILD.to_string(),
                    "properties": { "name": "Rust Port", "owner_id": "999" },
                    "roles": [
                        { "id": GUILD.to_string(), "name": "@everyone", "position": 0, "permissions": perms.to_string() },
                        { "id": ROLE_MOD.to_string(), "name": "Mods", "position": 3, "color": 0x3498db, "permissions": "0" }
                    ],
                    "channels": [{ "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 }],
                    "members": [
                        { "user": { "id": "100", "username": "me" }, "roles": [] },
                        { "user": { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" }, "roles": [ROLE_MOD.to_string()] }
                    ]
                },
                {
                    "id": SECOND_GUILD.to_string(),
                    "properties": { "name": "Second Guild", "owner_id": "999" },
                    "roles": [{ "id": SECOND_GUILD.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
                    "channels": [{ "id": "21", "type": 0, "name": "lobby", "position": 0 }],
                    "members": [{ "user": { "id": "100", "username": "me" }, "roles": [] }]
                }
            ],
            "private_channels": [],
            "read_states": []
        }),
    }
}

fn open_profile_fixture(
    cx: &mut TestAppContext,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut VisualTestContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    {
        let mut state = backend.state();
        state.apply(&profile_ready());
        let msg: Message = serde_json::from_value(json!({
            "id": msg_id(1).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" },
            "content": "hi",
            "timestamp": "2026-11-18T10:20:30+00:00"
        }))
        .expect("message");
        state.messages.load(GENERAL, vec![msg], LoadMode::Replace);
    }
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

#[gpui_kit::test]
fn profile_opens_loads_saves_note_and_navigates(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_profile_fixture(cx);

    view.update_in(cx, |v, window, cx| {
        v.open_profile(ALICE, Some(GUILD), point(px(200.), px(200.)), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    view.read_with(cx, |v, _| {
        let Overlay::Profile(pop) = &v.overlay else { panic!("profile did not open") };
        assert!(!pop.card.loaded);
        assert_eq!(pop.card.display_name, "Alice");
        assert_eq!(pop.card.note, "initial note");
    });

    // Simulate REST profile load
    let loaded = UserProfileFull {
        user: Some(ProfileUser {
            id: ALICE,
            username: "alice".to_owned(),
            discriminator: Some("0042".to_owned()),
            global_name: Some("Alice".to_owned()),
            avatar: None,
            avatar_color: None,
            banner: None,
            bot: false,
        }),
        user_profile: Some(UserProfileData {
            bio: Some("Global bio".to_owned()),
            pronouns: None,
            banner: None,
            banner_color: None,
            accent_color: Some(0x112233),
        }),
        guild_member_profile: Some(UserProfileData {
            bio: Some("Guild bio".to_owned()),
            pronouns: Some("they/them".to_owned()),
            banner: None,
            banner_color: None,
            accent_color: None,
        }),
        guild_member: Some(GuildMemberSummary {
            nick: Some("Ali".to_owned()),
            avatar: None,
            banner: None,
            roles: vec![ROLE_MOD],
            profile_flags: 0,
        }),
        mutual_friends: None,
        mutual_guilds: Some(vec![MutualGuild { id: SECOND_GUILD, nick: None }]),
        ..Default::default()
    };
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::ProfileLoaded { user_id: ALICE, guild_id: Some(GUILD), result: Ok(loaded) }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);

    view.read_with(cx, |v, _| {
        let Overlay::Profile(pop) = &v.overlay else { panic!("profile closed") };
        assert!(pop.card.loaded);
        assert_eq!(pop.card.display_name, "Ali");
        assert_eq!(pop.card.username_tag, "alice#0042");
        assert_eq!(pop.card.bio.as_deref(), Some("Guild bio"));
        assert_eq!(pop.card.roles.len(), 1);
        assert_eq!(pop.card.roles[0].name, "Mods");
        assert_eq!(pop.card.mutual_guilds.len(), 1);
        assert_eq!(pop.card.mutual_guilds[0].id, SECOND_GUILD);
    });

    // Edit note and close -> saves note
    let note_input = view.read_with(cx, |v, _| {
        let Overlay::Profile(pop) = &v.overlay else { panic!("profile closed") };
        pop.note_input.clone()
    });
    cx.update(|window, cx| {
        let focus = note_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    note_input.update_in(cx, |s, window, cx| s.set_value("updated note", window, cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(*lock_sent(&backend.saved_notes), vec![(ALICE, Some("updated note".to_owned()))]);

    // Mutual guild click switches place
    view.update_in(cx, |v, window, cx| {
        v.open_profile(ALICE, Some(GUILD), point(px(200.), px(200.)), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    view.update_in(cx, |v, window, cx| v.profile_open_guild(SECOND_GUILD, window, cx));
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Guild(SECOND_GUILD));
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Message button opens DM (and requires confirmation when blocked)
    view.update_in(cx, |v, window, cx| {
        v.open_profile(ALICE, None, point(px(200.), px(200.)), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |v, window, cx| v.profile_message(window, cx));
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.opened_dms), vec![ALICE]);

    view.update_in(cx, |v, window, cx| {
        v.open_profile(BOB_BLOCKED, None, point(px(200.), px(200.)), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |v, window, cx| v.profile_message(window, cx));
    cx.run_until_parked();
    assert_eq!(lock_sent(&backend.opened_dms).len(), 1, "first click only asks confirmation for blocked user");
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::Profile(p) if p.confirm_blocked_dm)));
    view.update_in(cx, |v, window, cx| v.profile_message(window, cx));
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.opened_dms), vec![ALICE, BOB_BLOCKED]);

    // Backdrop click closes profile
    view.update_in(cx, |v, window, cx| {
        v.open_profile(ALICE, None, point(px(300.), px(200.)), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    cx.simulate_click(point(px(10.), px(10.)), Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Failed profile load sets error surface; failed note save restores saved_note for retry.
    view.update_in(cx, |v, window, cx| {
        v.open_profile(ALICE, None, point(px(200.), px(200.)), window, cx);
    });
    cx.run_until_parked();
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::ProfileLoaded { user_id: ALICE, guild_id: None, result: Err("offline".to_owned()) }],
            cx,
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let Overlay::Profile(pop) = &v.overlay else { panic!("profile closed") };
        assert!(pop.card.loaded);
        assert_eq!(pop.error.as_deref(), Some("offline"));
    });

    // Simulate failed note save after optimistic state rollback: saved_note must revert so user can retry.
    backend.state().set_local_note(ALICE, Some("initial note".to_owned()));
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::NoteSaved { user_id: ALICE, error: Some("500".to_owned()) }],
            cx,
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let Overlay::Profile(pop) = &v.overlay else { panic!("profile closed") };
        assert_eq!(pop.saved_note, "initial note");
    });
    drop(rt);
}
