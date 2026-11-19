//! Headless GPUI tests for Milestone 5: Group DM creation modal, right-hand
//! member/rename panel, owner vs non-owner actions, gateway recipient sync,
//! and Leave group confirmation modal.

use fluxer_client_core::Dispatch;
use fluxer_client_core::state::Snowflake;
use gpui_kit::{Modifiers, TestAppContext};
use serde_json::json;

use crate::backend::{Backend, GroupDmCall, UiEvent, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{draw, focus_composer, offline_rest, open_client, runtime};
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const ME: Snowflake = Snowflake(100);
const ALICE: Snowflake = Snowflake(201);
const BOB: Snowflake = Snowflake(202);
const CAROL: Snowflake = Snowflake(203);
const GROUP_OWNER_ID: Snowflake = Snowflake(501);
const GROUP_OTHER_ID: Snowflake = Snowflake(502);

fn friend_ready(extra_channels: Vec<serde_json::Value>) -> Dispatch {
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
                    "user": { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" }
                },
                {
                    "id": BOB.to_string(),
                    "type": 1,
                    "user": { "id": BOB.to_string(), "username": "bob", "global_name": "Bob" }
                },
                {
                    "id": CAROL.to_string(),
                    "type": 1,
                    "user": { "id": CAROL.to_string(), "username": "carol", "global_name": "Carol" }
                }
            ],
            "private_channels": extra_channels,
            "guilds": []
        }),
    }
}

fn open_home(
    cx: &mut TestAppContext,
    extra_channels: Vec<serde_json::Value>,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut gpui_kit::VisualTestContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&friend_ready(extra_channels)).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);
    (rt, backend, view, cx)
}

#[gpui_kit::test]
fn create_group_dm_modal_picks_friends_handles_error_and_opens_channel(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_home(cx, Vec::new());

    let btn = cx.debug_bounds("create-group-dm-btn").expect("create group DM button drawn").center();
    cx.simulate_click(btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::CreateGroupDm(_))));
    assert!(cx.debug_bounds("create-group-dm-modal").is_some());

    // Empty submit sets validation error and sends nothing
    let submit = cx.debug_bounds("create-group-dm-submit").expect("submit drawn").center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert!(lock_sent(&backend.sent_group_dm).is_empty());
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::CreateGroupDm(m) if m.error.is_some())));

    // Pick Alice & Bob and submit (seq = 1)
    let pick_alice = cx.debug_bounds("gdm-pick-201").expect("Alice row drawn").center();
    cx.simulate_click(pick_alice, Modifiers::default());
    cx.run_until_parked();
    let pick_bob = cx.debug_bounds("gdm-pick-202").expect("Bob row drawn").center();
    cx.simulate_click(pick_bob, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    let submit = cx.debug_bounds("create-group-dm-submit").expect("submit drawn").center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.sent_group_dm), vec![GroupDmCall::Create(vec![ALICE, BOB])]);
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::CreateGroupDm(m) if m.loading && m.request_seq == Some(1))));

    // Cancelling via Esc and reopening ignores stale seq = 1 completion
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let btn = cx.debug_bounds("create-group-dm-btn").expect("create group DM button drawn").center();
    cx.simulate_click(btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::GroupDmCreated { seq: 1, result: Ok(GROUP_OWNER_ID) }], cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::CreateGroupDm(_))));
    assert_eq!(view.read_with(cx, |v, _| v.selected_channel), None);

    // Re-pick Alice & Bob and submit (seq = 2); failure keeps modal open with error
    let pick_alice = cx.debug_bounds("gdm-pick-201").expect("Alice row drawn").center();
    cx.simulate_click(pick_alice, Modifiers::default());
    cx.run_until_parked();
    let pick_bob = cx.debug_bounds("gdm-pick-202").expect("Bob row drawn").center();
    cx.simulate_click(pick_bob, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("create-group-dm-submit").expect("submit drawn").center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::CreateGroupDm(m) if m.loading && m.request_seq == Some(2))));

    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::GroupDmCreated { seq: 2, result: Err("rate limited".to_owned()) }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        let Overlay::CreateGroupDm(m) = &v.overlay else { panic!("expected CreateGroupDm open") };
        assert!(!m.loading);
        assert_eq!(m.error.as_deref(), Some("rate limited"));
    });

    // Retry submit (seq = 3), apply CHANNEL_CREATE, and verify seq = 3 opens new Group DM
    let submit = cx.debug_bounds("create-group-dm-submit").expect("submit drawn").center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::CreateGroupDm(m) if m.loading && m.request_seq == Some(3))));
    assert_eq!(lock_sent(&backend.sent_group_dm).len(), 3);

    backend.state().apply(&Dispatch {
        t: "CHANNEL_CREATE".to_owned(),
        s: 2,
        d: json!({
            "id": GROUP_OWNER_ID.to_string(),
            "type": 3,
            "owner_id": ME.to_string(),
            "recipients": [
                { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" },
                { "id": BOB.to_string(), "username": "bob", "global_name": "Bob" }
            ]
        }),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::GroupDmCreated { seq: 3, result: Ok(GROUP_OWNER_ID) }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(view.read_with(cx, |v, _| (v.place, v.selected_channel)), (Place::Home, Some(GROUP_OWNER_ID)));
    assert!(cx.debug_bounds("group-dm-panel").is_some());

    drop(rt);
}

#[gpui_kit::test]
fn group_dm_panel_owner_vs_non_owner_rename_add_remove_and_gateway_sync(cx: &mut TestAppContext) {
    let owned_gdm = json!({
        "id": GROUP_OWNER_ID.to_string(),
        "type": 3,
        "name": "Raid Crew",
        "owner_id": ME.to_string(),
        "recipients": [
            { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" }
        ]
    });
    let other_gdm = json!({
        "id": GROUP_OTHER_ID.to_string(),
        "type": 3,
        "name": "Friends Chat",
        "owner_id": ALICE.to_string(),
        "recipients": [
            { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" },
            { "id": BOB.to_string(), "username": "bob", "global_name": "Bob" }
        ]
    });
    let (rt, backend, view, cx) = open_home(cx, vec![owned_gdm, other_gdm]);

    // Open owned group DM
    view.update(cx, |v, cx| v.open_channel(None, GROUP_OWNER_ID, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("group-dm-panel").is_some());
    assert!(cx.debug_bounds("gdm-remove-201").is_some(), "owner can remove recipients");

    // Toggle header button hides then restores Group DM panel without changing guild member_list_open
    let toggle = cx.debug_bounds("toggle-member-list").expect("member toggle drawn").center();
    cx.simulate_click(toggle, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("group-dm-panel").is_none());
    assert!(view.read_with(cx, |v, _| !v.group_dm_panel_open && v.member_list_open));
    let toggle = cx.debug_bounds("toggle-member-list").expect("member toggle still drawn").center();
    cx.simulate_click(toggle, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("group-dm-panel").is_some());
    assert!(view.read_with(cx, |v, _| v.group_dm_panel_open && v.member_list_open));

    // Unchanged save is a no-op; renaming sends one request and gateway CHANNEL_UPDATE syncs the input
    let save_btn = cx.debug_bounds("gdm-rename-save").expect("save button drawn").center();
    cx.simulate_click(save_btn, Modifiers::default());
    cx.run_until_parked();
    assert!(lock_sent(&backend.sent_group_dm).is_empty());

    view.update_in(cx, |v, window, cx| {
        v.group_dm_rename.update(cx, |s, cx| s.set_value("New Crew", window, cx));
    });
    cx.run_until_parked();
    draw(cx);
    let save_btn = cx.debug_bounds("gdm-rename-save").expect("save button drawn").center();
    cx.simulate_click(save_btn, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        *lock_sent(&backend.sent_group_dm),
        vec![GroupDmCall::Rename { channel_id: GROUP_OWNER_ID, name: Some("New Crew".to_owned()) }]
    );

    backend.state().apply(&Dispatch {
        t: "CHANNEL_UPDATE".to_owned(),
        s: 2,
        d: json!({
            "id": GROUP_OWNER_ID.to_string(),
            "type": 3,
            "name": "Server Renamed",
            "owner_id": ME.to_string(),
            "recipients": [
                { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" }
            ]
        }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, cx| v.group_dm_rename.read(cx).value().to_string()),
        "Server Renamed"
    );

    // Add friend inline picker shows Bob & Carol (not Alice/Me)
    let add_toggle = cx.debug_bounds("gdm-add-toggle").expect("add friends toggle drawn").center();
    cx.simulate_click(add_toggle, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("gdm-add-201").is_none(), "Alice already in group");
    let add_bob = cx.debug_bounds("gdm-add-202").expect("Bob addable").center();
    cx.simulate_click(add_bob, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        lock_sent(&backend.sent_group_dm).last(),
        Some(&GroupDmCall::AddRecipient { channel_id: GROUP_OWNER_ID, user_id: BOB })
    );

    // Gateway CHANNEL_RECIPIENT_ADD updates panel members dynamically
    backend.state().apply(&Dispatch {
        t: "CHANNEL_RECIPIENT_ADD".to_owned(),
        s: 3,
        d: json!({ "channel_id": GROUP_OWNER_ID.to_string(), "user": { "id": CAROL.to_string(), "username": "carol", "global_name": "Carol" } }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.group_dm.as_ref().map(|g| g.members.len())), Some(3));
    assert!(cx.debug_bounds("gdm-remove-203").is_some());

    // Owner clicks remove on Carol
    let rm_carol = cx.debug_bounds("gdm-remove-203").expect("Carol remove button drawn").center();
    cx.simulate_click(rm_carol, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        lock_sent(&backend.sent_group_dm).last(),
        Some(&GroupDmCall::RemoveRecipient { channel_id: GROUP_OWNER_ID, user_id: CAROL })
    );

    // Gateway CHANNEL_RECIPIENT_REMOVE shrinks panel members back down
    backend.state().apply(&Dispatch {
        t: "CHANNEL_RECIPIENT_REMOVE".to_owned(),
        s: 4,
        d: json!({ "channel_id": GROUP_OWNER_ID.to_string(), "user": { "id": CAROL.to_string() } }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.group_dm.as_ref().map(|g| g.members.len())), Some(2));

    // Switching to a group owned by Alice hides remove buttons
    view.update(cx, |v, cx| v.open_channel(None, GROUP_OTHER_ID, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("group-dm-panel").is_some());
    assert!(cx.debug_bounds("gdm-remove-201").is_none());
    assert!(cx.debug_bounds("gdm-remove-202").is_none());

    drop(rt);
}

#[gpui_kit::test]
fn leave_group_dm_modal_cancel_and_confirm(cx: &mut TestAppContext) {
    let gdm = json!({
        "id": GROUP_OTHER_ID.to_string(),
        "type": 3,
        "name": "Weekend",
        "owner_id": ALICE.to_string(),
        "recipients": [
            { "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" }
        ]
    });
    let (rt, backend, view, cx) = open_home(cx, vec![gdm]);
    view.update(cx, |v, cx| v.open_channel(None, GROUP_OTHER_ID, cx));
    cx.run_until_parked();
    draw(cx);
    focus_composer(&view, cx);

    let leave_btn = cx.debug_bounds("gdm-leave-btn").expect("leave button drawn").center();
    cx.simulate_click(leave_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::LeaveGroupDm(_))));
    assert!(cx.debug_bounds("leave-group-dm-modal").is_some());

    // Escape cancels cleanly without backend call
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert!(lock_sent(&backend.sent_group_dm).is_empty());

    // Confirm triggers Leave and UiEvent::GroupDmLeft clears channel back to Home Friends
    let leave_btn = cx.debug_bounds("gdm-leave-btn").expect("leave button drawn").center();
    cx.simulate_click(leave_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let confirm = cx.debug_bounds("leave-group-dm-confirm").expect("confirm button drawn").center();
    cx.simulate_click(confirm, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        *lock_sent(&backend.sent_group_dm),
        vec![GroupDmCall::Leave { channel_id: GROUP_OTHER_ID, silent: false }]
    );

    view.update(cx, |v, cx| {
        v.loading_history = true;
        v.handle_events(vec![UiEvent::GroupDmLeft { channel_id: GROUP_OTHER_ID, error: None }], cx);
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| (v.place, v.selected_channel, v.group_dm.is_some(), v.loading_history)),
        (Place::Home, None, false, false)
    );

    drop(rt);
}
