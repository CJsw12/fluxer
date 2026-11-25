//! Headless GPUI tests for the Explore communities page (`Place::Explore`),
//! discovery search/category/language filters, pagination, and preview/join modal.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{DiscoveryCategory, DiscoveryGuild, DiscoveryGuildList, DiscoveryGuildQuery};
use fluxer_client_core::state::{Snowflake, bits};
use gpui_kit::{Modifiers, TestAppContext};
use serde_json::json;

use crate::backend::{Backend, UiEvent, lock_sent};
use crate::tests::{draw, offline_rest, open_client, runtime};
use crate::ui::explore::PAGE_SIZE;
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const ME: Snowflake = Snowflake(100);
const GUILD_ONE: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);
const DISCOVERY_GUILD_ID: Snowflake = Snowflake(777);

fn click_selector(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    let pt = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("expected debug_bounds({selector})"))
        .center();
    cx.simulate_click(pt, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
}

fn ready_dispatch() -> Dispatch {
    let perms = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": ME.to_string(), "username": "me", "global_name": "Me" },
            "guilds": [{
                "id": GUILD_ONE.to_string(),
                "properties": { "name": "Existing Guild", "owner_id": ME.to_string() },
                "roles": [{ "id": GUILD_ONE.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
                "channels": [{ "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 }],
                "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }]
            }],
            "private_channels": []
        }),
    }
}

fn sample_guild(id: &str, name: &str) -> DiscoveryGuild {
    DiscoveryGuild {
        id: id.to_owned(),
        name: name.to_owned(),
        icon: None,
        banner: None,
        description: Some("Systems & Rust hangout".to_owned()),
        category_type: 4,
        primary_language: Some("en-US".to_owned()),
        custom_tags: vec!["rust".to_owned()],
        member_count: 150,
        online_count: 25,
        features: vec!["DISCOVERABLE".to_owned()],
        badge_ids: vec![],
        verification_level: 1,
    }
}

#[gpui_kit::test]
fn compass_opens_explore_sidebar_tabs_and_empty_state(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&ready_dispatch()).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);

    click_selector(cx, "rail-discover-btn");
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Explore);
    assert!(cx.debug_bounds("explore-sidebar").is_some());
    assert!(cx.debug_bounds("explore-page").is_some());
    assert!(cx.debug_bounds("explore-nav-communities").is_some());
    assert!(cx.debug_bounds("explore-cat-all").is_some());
    assert!(cx.debug_bounds("explore-cat-0").is_some());

    {
        let sent = lock_sent(&backend.sent_discovery_searches);
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0],
            DiscoveryGuildQuery {
                query: None,
                category: None,
                language: None,
                tag: None,
                sort_by: None,
                limit: PAGE_SIZE,
                offset: 0,
            }
        );
    }

    let seq = view.read_with(cx, |v, _| v.explore.search_seq);
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![
                UiEvent::DiscoveryCategoriesLoaded {
                    result: Ok(vec![DiscoveryCategory {
                        id: 4,
                        name: "Science & Technology".to_owned(),
                    }]),
                },
                UiEvent::DiscoveryGuildsLoaded {
                    seq,
                    offset: 0,
                    result: Ok(DiscoveryGuildList {
                        guilds: vec![],
                        total: 0,
                        category_counts: vec![],
                    }),
                },
            ],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("explore-cat-4").is_some());
    assert!(view.read_with(cx, |v, _| v.explore.has_loaded && !v.explore.loading && v.explore.guilds.is_empty()));
}

#[gpui_kit::test]
fn explore_category_search_and_language_filters_dispatch_queries(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&ready_dispatch()).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);

    click_selector(cx, "rail-discover-btn");
    click_selector(cx, "explore-cat-0");

    view.update_in(cx, |v, window, cx| {
        v.explore.search_input.update(cx, |s, cx| s.set_value("rust", window, cx));
        v.submit_explore_search(cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("explore-search-back").is_some());
    assert!(cx.debug_bounds("explore-search-clear").is_some());

    click_selector(cx, "explore-language-btn");
    assert!(cx.debug_bounds("explore-lang-all").is_some());
    click_selector(cx, "explore-lang-de");

    click_selector(cx, "explore-search-clear");
    assert!(cx.debug_bounds("explore-cat-0").is_some());

    let sent = lock_sent(&backend.sent_discovery_searches).clone();
    assert_eq!(sent.len(), 5);
    assert_eq!(sent[1].category, Some(0));
    assert_eq!(sent[1].query, None);
    assert_eq!(sent[2].category, Some(0));
    assert_eq!(sent[2].query.as_deref(), Some("rust"));
    assert_eq!(sent[3].language.as_deref(), Some("de"));
    assert_eq!(sent[4].query, None);
    assert_eq!(sent[4].language.as_deref(), Some("de"));
}

#[gpui_kit::test]
fn explore_renders_cards_paginates_and_retries_on_error(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&ready_dispatch()).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);

    click_selector(cx, "rail-discover-btn");
    let seq1 = view.read_with(cx, |v, _| v.explore.search_seq);
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::DiscoveryGuildsLoaded {
                seq: seq1,
                offset: 0,
                result: Err("network down".to_owned()),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("explore-retry-btn").is_some());
    click_selector(cx, "explore-retry-btn");

    let seq2 = view.read_with(cx, |v, _| v.explore.search_seq);
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::DiscoveryGuildsLoaded {
                seq: seq2,
                offset: 0,
                result: Ok(DiscoveryGuildList {
                    guilds: vec![sample_guild(&DISCOVERY_GUILD_ID.to_string(), "Rust Lab")],
                    total: 2,
                    category_counts: vec![],
                }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);

    assert!(cx.debug_bounds("explore-card-777").is_some());
    assert!(cx.debug_bounds("explore-load-more").is_some());
    click_selector(cx, "explore-load-more");

    let seq3 = view.read_with(cx, |v, _| v.explore.search_seq);
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::DiscoveryGuildsLoaded {
                seq: seq3,
                offset: 1,
                result: Ok(DiscoveryGuildList {
                    guilds: vec![sample_guild("778", "Systems Club")],
                    total: 2,
                    category_counts: vec![],
                }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("explore-card-778").is_some());
    assert!(cx.debug_bounds("explore-load-more").is_none());

    let sent = lock_sent(&backend.sent_discovery_searches);
    assert_eq!(sent.last().map(|q| q.offset), Some(1));
}

#[gpui_kit::test]
fn explore_preview_modal_joins_and_switches_on_guild_create(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&ready_dispatch()).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);

    click_selector(cx, "rail-discover-btn");
    let seq = view.read_with(cx, |v, _| v.explore.search_seq);
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::DiscoveryGuildsLoaded {
                seq,
                offset: 0,
                result: Ok(DiscoveryGuildList {
                    guilds: vec![
                        sample_guild(&DISCOVERY_GUILD_ID.to_string(), "Rust Lab"),
                        sample_guild(&GUILD_ONE.to_string(), "Existing Guild"),
                    ],
                    total: 2,
                    category_counts: vec![],
                }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);

    click_selector(cx, "explore-card-777");
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::DiscoveryPreview(_))));

    click_selector(cx, "explore-preview-join-btn");
    assert_eq!(*lock_sent(&backend.sent_discovery_joins), vec![DISCOVERY_GUILD_ID]);
    let join_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::DiscoveryPreview(m) => m.join_seq,
        _ => None,
    })
    .unwrap();

    // Error keeps modal open and surfaces message.
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::DiscoveryGuildJoined {
                seq: join_seq,
                guild_id: DISCOVERY_GUILD_ID,
                error: Some("invites disabled".to_owned()),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::DiscoveryPreview(m) if !m.joining && m.error.as_deref() == Some("invites disabled")
    )));

    // Retry succeeds -> modal closes -> GUILD_CREATE navigates to joined guild.
    click_selector(cx, "explore-preview-join-btn");
    let join_seq2 = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::DiscoveryPreview(m) => m.join_seq,
        _ => None,
    })
    .unwrap();
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::DiscoveryGuildJoined {
                seq: join_seq2,
                guild_id: DISCOVERY_GUILD_ID,
                error: None,
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None) && v.pending_joined_guild == Some(DISCOVERY_GUILD_ID)));

    view.update(cx, |v, cx| {
        assert!(!v.backend.state().apply(&Dispatch {
            t: "GUILD_CREATE".to_owned(),
            s: 2,
            d: json!({
                "id": DISCOVERY_GUILD_ID.to_string(),
                "properties": { "name": "Rust Lab", "owner_id": "999" },
                "roles": [{ "id": DISCOVERY_GUILD_ID.to_string(), "name": "@everyone", "permissions": bits::VIEW_CHANNEL.to_string() }],
                "channels": [{ "id": "7771", "type": 0, "name": "welcome", "position": 0 }]
            }),
        }).is_empty());
        v.handle_events(vec![UiEvent::StateChanged], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Guild(DISCOVERY_GUILD_ID));
    assert_eq!(view.read_with(cx, |v, _| v.selected_channel), Some(Snowflake(7771)));

    // Clicking already-joined guild opens community directly without REST join.
    click_selector(cx, "rail-discover-btn");
    click_selector(cx, "explore-card-1");
    click_selector(cx, "explore-preview-join-btn");
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Guild(GUILD_ONE));
    assert_eq!(lock_sent(&backend.sent_discovery_joins).len(), 2);
}
