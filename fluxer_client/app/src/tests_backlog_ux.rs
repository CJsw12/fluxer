//! Headless GPUI tests for Slice A backlog UX items:
//! grouped message hover timestamps, friends search, search scope & header updater controls,
//! personal notes context menu/purge modal, and DM voice/video call ringing.

use fluxer_client_core::Dispatch;
use fluxer_client_core::state::{LoadMode, Message, Snowflake, bits};
use gpui_kit::{Modifiers, MouseButton, TestAppContext};
use serde_json::json;

use crate::backend::{Backend, UiEvent, lock_sent};
use crate::format::set_time_24h;
use crate::tests::{draw, offline_rest, open_client, runtime};
use crate::ui::friends::FriendsTab;
use crate::ui::overlay::Overlay;
use crate::ui::search_panel::SearchScopeChoice;
use crate::ui::settings::SettingsTab;
use crate::updater::UpdateStatus;
use crate::view_model::Place;

const ME: Snowflake = Snowflake(100);
const PEER_A: Snowflake = Snowflake(201);
const PEER_B: Snowflake = Snowflake(202);
const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);
const DM_ONE: Snowflake = Snowflake(50);
const NOTES_CH: Snowflake = Snowflake(60);

fn click_selector(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    let pt = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("expected debug_bounds({selector})"))
        .center();
    cx.simulate_click(pt, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
}

fn ready_with_social() -> Dispatch {
    let perms = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": ME.to_string(), "username": "me", "global_name": "Me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "UX Guild", "owner_id": ME.to_string() },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
                "channels": [{ "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 }],
                "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }]
            }],
            "private_channels": [
                {
                    "id": DM_ONE.to_string(),
                    "type": 1,
                    "recipients": [{ "id": PEER_A.to_string(), "username": "alice", "global_name": "Alice Wonderland" }]
                },
                {
                    "id": NOTES_CH.to_string(),
                    "type": 999,
                    "recipients": []
                }
            ],
            "relationships": [
                {
                    "id": PEER_A.to_string(),
                    "type": 1,
                    "user": { "id": PEER_A.to_string(), "username": "alice", "global_name": "Alice Wonderland" }
                },
                {
                    "id": PEER_B.to_string(),
                    "type": 1,
                    "user": { "id": PEER_B.to_string(), "username": "bob_dev", "global_name": "Bob Builder" }
                }
            ],
            "presences": [
                { "user": { "id": PEER_A.to_string() }, "status": "online" },
                { "user": { "id": PEER_B.to_string() }, "status": "idle" }
            ]
        }),
    }
}

#[gpui_kit::test]
fn grouped_message_hover_timestamp_shows_only_when_hovered(cx: &mut TestAppContext) {
    set_time_24h(true);
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    {
        let mut st = backend.state();
        assert!(!st.apply(&ready_with_social()).is_empty());
        let msgs: Vec<Message> = [
            serde_json::from_value(json!({
                "id": "1001", "channel_id": GENERAL.to_string(), "content": "first in group",
                "timestamp": "2026-11-18T10:00:00+00:00",
                "author": { "id": PEER_A.to_string(), "username": "alice", "global_name": "Alice" }
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "id": "1002", "channel_id": GENERAL.to_string(), "content": "grouped child",
                "timestamp": "2026-11-18T10:01:00+00:00",
                "author": { "id": PEER_A.to_string(), "username": "alice", "global_name": "Alice" }
            }))
            .unwrap(),
        ]
        .into_iter()
        .collect();
        st.messages.load(GENERAL, msgs, LoadMode::Replace);
    }

    let (view, cx) = open_client(cx, backend, events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);

    // At rest, grouped row's hover timestamp is hidden.
    assert!(cx.debug_bounds("msg-hover-time-1002").is_none());
    view.update(cx, |v, cx| v.hover_message(Some(Snowflake(1002)), cx));
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("msg-hover-time-1002").is_some(), "hovering grouped row renders timestamp");
    set_time_24h(false);
}

#[gpui_kit::test]
fn friends_search_filters_rows_and_clears(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&ready_with_social()).is_empty());
    let (view, cx) = open_client(cx, backend, events);
    view.update(cx, |v, cx| v.set_friends_tab(FriendsTab::All, cx));
    cx.run_until_parked();
    draw(cx);

    assert!(cx.debug_bounds("friends-search-input").is_some());
    assert!(cx.debug_bounds("friends-search-clear").is_none());

    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.friends_search_input.update(cx, |s, cx| s.set_value("bob_dev", window, cx));
        });
    });
    cx.run_until_parked();
    draw(cx);

    assert!(cx.debug_bounds("friends-search-clear").is_some());
    click_selector(cx, "friends-search-clear");
    assert_eq!(view.read_with(cx, |v, cx| v.friends_search_input.read(cx).value().to_string()), "");
}

#[gpui_kit::test]
fn header_search_scope_and_update_buttons_work(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&ready_with_social()).is_empty());
    let (view, cx) = open_client(cx, backend, events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);

    assert!(cx.debug_bounds("header-update-btn").is_none(), "Idle update status hides button");
    click_selector(cx, "header-search-scope-btn");
    assert!(cx.debug_bounds("search-scope-opt-channel").is_some());
    assert!(cx.debug_bounds("search-scope-opt-guild").is_some());
    assert!(cx.debug_bounds("search-scope-opt-all").is_some());

    click_selector(cx, "search-scope-opt-guild");
    assert_eq!(
        view.read_with(cx, |v, _| v.search.as_ref().map(|s| s.scope)),
        Some(SearchScopeChoice::CurrentGuild)
    );
    assert!(cx.debug_bounds("header-search-scope-indicator").is_some());

    // Update Available with asset -> click starts download (untrusted host fails fast offline).
    view.update(cx, |v, cx| {
        v.update_status = UpdateStatus::Available {
            current: "0.1.0".into(),
            latest: "0.2.0".into(),
            release_url: "https://invalid.test/releases".into(),
            asset_name: Some("fluxer.zip".into()),
            asset_url: Some("https://invalid.test/fluxer.zip".into()),
            checksum_url: None,
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("header-update-btn").is_some());
    click_selector(cx, "header-update-btn");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.update_status,
        UpdateStatus::Error(msg) if msg.contains("Untrusted asset host")
    )));

    // Update Available without asset -> click opens Settings Advanced tab.
    view.update(cx, |v, cx| {
        v._update_task = None;
        v.update_status = UpdateStatus::Available {
            current: "0.1.0".into(),
            latest: "0.2.0".into(),
            release_url: "https://invalid.test/releases".into(),
            asset_name: None,
            asset_url: None,
            checksum_url: None,
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    click_selector(cx, "header-update-btn");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::Settings(m) if m.active_tab == SettingsTab::Advanced
    )));

    cx.update(|w, cx| view.update(cx, |v, cx| v.close_overlay(w, cx)));
    cx.run_until_parked();
    draw(cx);

    // Update Downloaded -> click opens Settings Advanced tab.
    view.update(cx, |v, cx| {
        v._update_task = None;
        v.update_status = UpdateStatus::Downloaded {
            latest: "0.2.0".into(),
            path: "fluxer.zip".into(),
            sha256: "abc".into(),
            verified: true,
        };
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    click_selector(cx, "header-update-btn");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::Settings(m) if m.active_tab == SettingsTab::Advanced
    )));
}

#[gpui_kit::test]
fn personal_notes_context_menu_and_purge_modal_work(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    {
        let mut st = backend.state();
        assert!(!st.apply(&ready_with_social()).is_empty());
        let note: Message = serde_json::from_value(json!({
            "id": "2001", "channel_id": NOTES_CH.to_string(), "content": "remember milk",
            "timestamp": "2026-11-18T10:00:00+00:00",
            "author": { "id": ME.to_string(), "username": "me" }
        }))
        .unwrap();
        st.messages.load(NOTES_CH, vec![note], LoadMode::Replace);
    }
    let (view, cx) = open_client(cx, backend.clone(), events);
    draw(cx);

    let notes_btn = cx.debug_bounds("home-personal-notes").expect("personal notes row").center();
    cx.simulate_mouse_down(notes_btn, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    click_selector(cx, "personal-notes-purge-item");
    let seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::PurgePersonalNotes(m) => {
            assert_eq!(m.channel_id, NOTES_CH);
            m.seq
        }
        _ => panic!("expected PurgePersonalNotes modal"),
    });

    click_selector(cx, "purge-personal-notes-confirm-btn");
    assert_eq!(*lock_sent(&backend.sent_personal_notes_purges), vec![NOTES_CH]);
    let sent_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::PurgePersonalNotes(m) => {
            assert!(m.seq >= seq && m.submitting);
            m.seq
        }
        _ => panic!("expected submitting PurgePersonalNotes"),
    });

    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::PersonalNotesPurged {
                seq: sent_seq,
                channel_id: NOTES_CH,
                result: Err("Server unavailable".to_owned()),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::PurgePersonalNotes(m) if !m.submitting && m.error.as_deref() == Some("Server unavailable")
    )));

    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::PersonalNotesPurged {
                seq: sent_seq,
                channel_id: NOTES_CH,
                result: Ok(1),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.overlay.is_open()));
}

#[gpui_kit::test]
fn dm_voice_and_video_call_buttons_ring_recipients(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&ready_with_social()).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.open_channel(None, DM_ONE, cx));
    cx.run_until_parked();
    draw(cx);

    click_selector(cx, "header-voice-call-btn");
    assert_eq!(*lock_sent(&backend.sent_call_rings), vec![(DM_ONE, Some(vec![PEER_A]))]);

    view.update(cx, |v, cx| v.leave_voice_channel(cx));
    click_selector(cx, "header-video-call-btn");
    assert_eq!(
        *lock_sent(&backend.sent_call_rings),
        vec![(DM_ONE, Some(vec![PEER_A])), (DM_ONE, Some(vec![PEER_A]))]
    );
}
