//! Headless UI tests for Home Friends, Relationship actions, Join Guild,
//! and Leave Guild modals.

use fluxer_client_core::Dispatch;
use fluxer_client_core::state::{RelationshipKind, Snowflake, bits};
use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, point, px};
use serde_json::json;

use crate::backend::{AcceptedInvite, Backend, InvitePreview, SocialCall, UiEvent, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{draw, focus_composer, offline_rest, open_client, runtime};
use crate::ui::friends::FriendsTab;
use crate::ui::overlay::Overlay;
use crate::view_model::Place;
use gpui_kit::Focusable as _;

const ME: Snowflake = Snowflake(100);
const ALICE: Snowflake = Snowflake(201);
const BOB: Snowflake = Snowflake(202);
const CAROL: Snowflake = Snowflake(203);
const DAVE: Snowflake = Snowflake(204);
const EVE: Snowflake = Snowflake(205);

const DM_ALICE: Snowflake = Snowflake(301);
const GUILD_MEMBER: Snowflake = Snowflake(1);
const GUILD_OWNER: Snowflake = Snowflake(2);
const GUILD_MEMBER_CH: Snowflake = Snowflake(11);
const GUILD_OWNER_CH: Snowflake = Snowflake(21);
const GUILD_JOINED: Snowflake = Snowflake(3);
const GUILD_JOINED_CH: Snowflake = Snowflake(31);

fn social_ready() -> Dispatch {
    let perms = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": ME.to_string(), "username": "me", "discriminator": "0001" },
            "relationships": [
                {
                    "id": ALICE.to_string(),
                    "type": 1,
                    "user": { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice", "discriminator": "0002" }
                },
                {
                    "id": BOB.to_string(),
                    "type": 1,
                    "user": { "id": BOB.to_string(), "username": "bob", "global_name": "Bob", "discriminator": "0003" }
                },
                {
                    "id": CAROL.to_string(),
                    "type": 3,
                    "user": { "id": CAROL.to_string(), "username": "carol", "global_name": "Carol", "discriminator": "0004" }
                },
                {
                    "id": DAVE.to_string(),
                    "type": 4,
                    "user": { "id": DAVE.to_string(), "username": "dave", "global_name": "Dave", "discriminator": "0005" }
                },
                {
                    "id": EVE.to_string(),
                    "type": 2,
                    "user": { "id": EVE.to_string(), "username": "eve", "global_name": "Eve", "discriminator": "0006" }
                }
            ],
            "presences": [
                { "user": { "id": ALICE.to_string() }, "status": "online" },
                { "user": { "id": BOB.to_string() }, "status": "offline" }
            ],
            "private_channels": [
                {
                    "id": DM_ALICE.to_string(),
                    "type": 1,
                    "recipients": [{ "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" }]
                }
            ],
            "guilds": [
                {
                    "id": GUILD_MEMBER.to_string(),
                    "properties": { "name": "Community", "owner_id": "999" },
                    "roles": [{ "id": GUILD_MEMBER.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
                    "channels": [{ "id": GUILD_MEMBER_CH.to_string(), "type": 0, "name": "general", "position": 0 }],
                    "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }]
                },
                {
                    "id": GUILD_OWNER.to_string(),
                    "properties": { "name": "Owned Place", "owner_id": ME.to_string() },
                    "roles": [{ "id": GUILD_OWNER.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
                    "channels": [{ "id": GUILD_OWNER_CH.to_string(), "type": 0, "name": "owner-chat", "position": 0 }],
                    "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }]
                }
            ],
            "read_states": []
        }),
    }
}

fn open_social_fixture(
    cx: &mut TestAppContext,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut VisualTestContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&social_ready());
    let (view, cx) = open_client(cx, backend.clone(), events);
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

#[gpui_kit::test]
fn friends_surface_tabs_badges_and_add_friend(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_social_fixture(cx);

    view.read_with(cx, |v, _| {
        assert_eq!(v.place, Place::Home);
        assert_eq!(v.selected_channel, None);
        assert_eq!(v.pending_incoming, 1);
        let ids: Vec<_> = v.friend_rows.iter().map(|r| r.user_id).collect();
        assert_eq!(ids, vec![ALICE], "Online tab excludes offline friend Bob");
    });
    assert!(cx.debug_bounds("friends-tab-online").is_some());
    assert!(cx.debug_bounds("home-friends").is_some());

    // Switch across All / Pending / Blocked / AddFriend tabs
    view.update(cx, |v, cx| v.set_friends_tab(FriendsTab::All, cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.friend_rows.iter().map(|r| r.user_id).collect::<Vec<_>>()),
        vec![ALICE, BOB]
    );

    view.update(cx, |v, cx| v.set_friends_tab(FriendsTab::Pending, cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.friend_rows.iter().map(|r| r.user_id).collect::<Vec<_>>()),
        vec![CAROL, DAVE]
    );

    view.update(cx, |v, cx| v.set_friends_tab(FriendsTab::Blocked, cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.friend_rows.iter().map(|r| r.user_id).collect::<Vec<_>>()),
        vec![EVE]
    );

    // Clicking into DM hides Friends; clicking "home-friends" brings it back
    view.update(cx, |v, cx| v.open_or_create_dm(ALICE, cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.selected_channel), Some(DM_ALICE));
    assert!(cx.debug_bounds("friends-tab-online").is_none());

    let home_friends = cx.debug_bounds("home-friends").expect("home-friends item drawn").center();
    cx.simulate_click(home_friends, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| (v.place, v.selected_channel)), (Place::Home, None));
    assert!(cx.debug_bounds("friends-tab-online").is_some());

    // Add friend tab: send tag + handle error vs success deferred input clear
    view.update(cx, |v, cx| v.set_friends_tab(FriendsTab::AddFriend, cx));
    cx.run_until_parked();
    draw(cx);
    let add_input = view.read_with(cx, |v, _| v.add_friend_input.clone());
    add_input.update_in(cx, |s, window, cx| s.set_value("charlie#0042", window, cx));
    cx.run_until_parked();
    draw(cx);
    let send_btn = cx.debug_bounds("friends-send-request").expect("send request button drawn").center();
    cx.simulate_click(send_btn, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        *lock_sent(&backend.sent_social),
        vec![SocialCall::SendFriendByTag {
            username: "charlie".to_owned(),
            discriminator: Some("0042".to_owned()),
        }]
    );

    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::FriendRequestSent {
                input: "charlie#0042".to_owned(),
                error: Some("User not found".to_owned()),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(add_input.read_with(cx, |s, _| s.value().to_string()), "charlie#0042");
    assert_eq!(
        view.read_with(cx, |v, _| v.friends_status.clone()),
        Some(Err("User not found".to_owned()))
    );

    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::FriendRequestSent {
                input: "charlie#0042".to_owned(),
                error: None,
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(add_input.read_with(cx, |s, _| s.value().to_string()), "");
    assert!(view.read_with(cx, |v, _| matches!(v.friends_status, Some(Ok(_)))));

    // Backend ActionFailed renders visibly inside Friends view (`friends-error`)
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::ActionFailed { error: "Relation blocked".to_owned() }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("friends-error").is_some());
    drop(rt);
}

#[gpui_kit::test]
fn relationship_actions_and_profile_popout_sync(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_social_fixture(cx);

    // Direct relationship calls + DM creation fallback for friend without DM
    view.update(cx, |v, cx| {
        v.open_or_create_dm(BOB, cx);
        v.backend.send_friend_request(Snowflake(777));
        v.backend.accept_friend_request(CAROL);
        v.backend.remove_relationship(DAVE);
        v.backend.block_user(ALICE);
    });
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.opened_dms), vec![BOB]);
    assert_eq!(
        *lock_sent(&backend.sent_social),
        vec![
            SocialCall::SendFriendById(Snowflake(777)),
            SocialCall::AcceptFriend(CAROL),
            SocialCall::RemoveRelationship(DAVE),
            SocialCall::BlockUser(ALICE),
        ]
    );

    // Profile popout tracks live relationship state via refresh_open_profile
    view.update_in(cx, |v, window, cx| {
        v.open_profile(CAROL, None, point(px(200.), px(200.)), window, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let Overlay::Profile(pop) = &v.overlay else { panic!("profile did not open") };
        assert_eq!(pop.card.relationship, Some(RelationshipKind::IncomingRequest));
    });

    backend.state().apply(&Dispatch {
        t: "RELATIONSHIP_ADD".to_owned(),
        s: 2,
        d: json!({ "id": CAROL.to_string(), "type": 1 }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let Overlay::Profile(pop) = &v.overlay else { panic!("profile closed") };
        assert_eq!(pop.card.relationship, Some(RelationshipKind::Friend));
    });

    backend.state().apply(&Dispatch {
        t: "RELATIONSHIP_REMOVE".to_owned(),
        s: 3,
        d: json!({ "id": CAROL.to_string() }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let Overlay::Profile(pop) = &v.overlay else { panic!("profile closed") };
        assert_eq!(pop.card.relationship, None);
    });
    drop(rt);
}

#[gpui_kit::test]
fn join_guild_shortcut_preview_accept_and_guild_create(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_social_fixture(cx);

    // Ctrl+Shift+N opens JoinGuild modal even from initial Friends screen (0px mounted composer focus)
    focus_composer(&view, cx);
    cx.simulate_keystrokes("ctrl-shift-n");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::JoinGuild(_))));

    // Escape closes JoinGuild modal and restores focus to composer
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let composer_focused = cx.update(|window, cx| {
        view.read_with(cx, |v, cx| v.composer.read(cx).focus_handle(cx).is_focused(window))
    });
    assert!(composer_focused, "Escape on JoinGuild restores composer focus");
    cx.simulate_keystrokes("ctrl-shift-n");
    cx.run_until_parked();
    draw(cx);

    let input = view.read_with(cx, |v, _| {
        let Overlay::JoinGuild(modal) = &v.overlay else { panic!("join modal missing") };
        modal.input.clone()
    });
    input.update_in(cx, |s, window, cx| s.set_value("https://fluxer.app/invite/first-code?x=1", window, cx));
    view.update(cx, |v, cx| v.preview_join_invite(cx));
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.sent_social), vec![SocialCall::FetchInvite("first-code".to_owned())]);
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::JoinGuild(m) if m.loading)));

    // Typing a new code emits `InputEvent::Change`, resets loading/preview, and ignores stale responses
    input.update_in(cx, |s, window, cx| s.set_value("", window, cx));
    cx.simulate_input("fluxer.gg/new-code");
    cx.run_until_parked();
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::InvitePreviewLoaded {
                code: "first-code".to_owned(),
                result: Ok(InvitePreview {
                    code: "first-code".to_owned(),
                    guild_name: "Stale Guild".to_owned(),
                    guild_id: Some(Snowflake(999)),
                    channel_name: None,
                    member_count: None,
                    presence_count: None,
                }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let Overlay::JoinGuild(modal) = &v.overlay else { panic!("join modal missing") };
        assert!(!modal.loading);
        assert_eq!(modal.preview, None);
    });

    // Preview new code and verify loading guard blocks duplicate accept while in flight
    view.update(cx, |v, cx| v.preview_join_invite(cx));
    cx.run_until_parked();
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::InvitePreviewLoaded {
                code: "new-code".to_owned(),
                result: Ok(InvitePreview {
                    code: "new-code".to_owned(),
                    guild_name: "New Guild".to_owned(),
                    guild_id: Some(GUILD_JOINED),
                    channel_name: Some("welcome".to_owned()),
                    member_count: Some(25),
                    presence_count: Some(4),
                }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        let Overlay::JoinGuild(modal) = &v.overlay else { panic!("join modal missing") };
        assert_eq!(modal.preview.as_ref().map(|p| p.guild_name.as_str()), Some("New Guild"));
    });

    let submit_btn = cx.debug_bounds("join-guild-submit").expect("join submit drawn").center();
    cx.simulate_click(submit_btn, Modifiers::default());
    cx.run_until_parked();
    view.update(cx, |v, cx| v.confirm_join_invite(cx));
    cx.run_until_parked();
    assert_eq!(
        *lock_sent(&backend.sent_social),
        vec![
            SocialCall::FetchInvite("first-code".to_owned()),
            SocialCall::FetchInvite("new-code".to_owned()),
            SocialCall::AcceptInvite("new-code".to_owned()),
        ]
    );

    // Stale InviteAccepted for superseded code must be ignored
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::InviteAccepted {
                code: "first-code".to_owned(),
                result: Ok(AcceptedInvite {
                    guild_id: Some(Snowflake(999)),
                    channel_id: None,
                }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::JoinGuild(_))));
    assert_eq!(view.read_with(cx, |v, _| v.pending_joined_guild), None);

    // Matching accept closes modal and awaits GUILD_CREATE -> auto-switches into joined guild
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::InviteAccepted {
                code: "new-code".to_owned(),
                result: Ok(AcceptedInvite {
                    guild_id: Some(GUILD_JOINED),
                    channel_id: None,
                }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(view.read_with(cx, |v, _| v.pending_joined_guild), Some(GUILD_JOINED));

    let perms = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    backend.state().apply(&Dispatch {
        t: "GUILD_CREATE".to_owned(),
        s: 4,
        d: json!({
            "id": GUILD_JOINED.to_string(),
            "properties": { "name": "New Guild", "owner_id": "999" },
            "roles": [{ "id": GUILD_JOINED.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
            "channels": [{ "id": GUILD_JOINED_CH.to_string(), "type": 0, "name": "welcome", "position": 0 }],
            "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }]
        }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        assert_eq!(v.pending_joined_guild, None);
        assert_eq!(v.place, Place::Guild(GUILD_JOINED));
        assert_eq!(v.selected_channel, Some(GUILD_JOINED_CH));
    });

    // Rail "+" button opens JoinGuild and group DM invite opens channel on CHANNEL_CREATE
    view.update(cx, |v, cx| v.select_place(Place::Home, cx));
    cx.run_until_parked();
    draw(cx);
    let plus_btn = cx.debug_bounds("join-guild").expect("join-guild rail button drawn").center();
    cx.simulate_click(plus_btn, Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::JoinGuild(_))));
    cx.simulate_input("gdm-code");
    cx.run_until_parked();
    view.update(cx, |v, cx| v.confirm_join_invite(cx));
    cx.run_until_parked();
    let group_dm = Snowflake(888);
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::InviteAccepted {
                code: "gdm-code".to_owned(),
                result: Ok(AcceptedInvite { guild_id: None, channel_id: Some(group_dm) }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.pending_joined_channel), Some(group_dm));
    backend.state().apply(&Dispatch {
        t: "CHANNEL_CREATE".to_owned(),
        s: 5,
        d: json!({ "id": group_dm.to_string(), "type": 3, "name": "Weekend Crew" }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| (v.place, v.selected_channel, v.pending_joined_channel)), (Place::Home, Some(group_dm), None));
    drop(rt);
}

#[gpui_kit::test]
fn leave_guild_non_owner_vs_owner(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_social_fixture(cx);

    // Non-owner guild exposes leave button; Escape closes modal and restores composer focus
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD_MEMBER), cx));
    cx.run_until_parked();
    draw(cx);
    focus_composer(&view, cx);
    assert!(!view.read_with(cx, |v, _| v.current_guild_is_owner()));
    let leave_btn = cx.debug_bounds("leave-guild-btn").expect("leave button shown for non-owner").center();
    cx.simulate_click(leave_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::LeaveGuild(_))));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let composer_focused = cx.update(|window, cx| {
        view.read_with(cx, |v, cx| v.composer.read(cx).focus_handle(cx).is_focused(window))
    });
    assert!(composer_focused, "Escape on LeaveGuild closes modal instead of composer");

    // Reopen and confirm leave
    let leave_btn = cx.debug_bounds("leave-guild-btn").expect("leave button shown").center();
    cx.simulate_click(leave_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let confirm_btn = cx.debug_bounds("leave-guild-confirm").expect("leave confirm drawn").center();
    cx.simulate_click(confirm_btn, Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(*lock_sent(&backend.sent_social), vec![SocialCall::LeaveGuild(GUILD_MEMBER)]);

    // Failure event populates error without panicking
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::GuildLeft {
                guild_id: GUILD_MEMBER,
                error: Some("Could not leave community: offline".to_owned()),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.error.is_some()));

    // GUILD_DELETE + GuildLeft returns cleanly to Home Friends
    backend.state().apply(&Dispatch {
        t: "GUILD_DELETE".to_owned(),
        s: 5,
        d: json!({ "id": GUILD_MEMBER.to_string() }),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::GuildLeft { guild_id: GUILD_MEMBER, error: None }, UiEvent::StateChanged], cx);
    });
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| (v.place, v.selected_channel, v.last_guild)), (Place::Home, None, None));

    // Owner guild hides leave button and ignores open_leave_guild
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD_OWNER), cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.current_guild_is_owner()));
    assert!(cx.debug_bounds("leave-guild-btn").is_none());
    view.update_in(cx, |v, window, cx| v.open_leave_guild(GUILD_OWNER, window, cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    drop(rt);
}
