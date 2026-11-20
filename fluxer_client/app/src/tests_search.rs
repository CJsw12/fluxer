//! Headless GPUI tests for Milestone 4: message search drawer (Ctrl+F,
//! header button, Esc, Enter once, seq continuity across close/reopen),
//! inline query parsing + threads + pagination + indexing + error preservation,
//! and jump-to-message (`load_around` boundaries + highlight + Jump to present).

use fluxer_client_core::rest::{
    MessageContentType, MessageSearchOutcome, MessageSearchPage, MessageSearchScope, MessageSearchSortBy,
};
use fluxer_client_core::state::{Channel, LoadMode, Message, Snowflake};
use gpui_kit::{Focusable as _, Modifiers, TestAppContext};
use serde_json::json;

use crate::backend::{UiEvent, lock_sent};
use crate::tests::{backend_with_fixture, draw, focus_composer, msg_id, open_client, runtime};
use crate::ui::search_panel::SearchScopeChoice;
use crate::view_model::Place;

const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);
const ANNOUNCEMENTS: Snowflake = Snowflake(12);
const THREAD: Snowflake = Snowflake(99);

fn make_message(id: Snowflake, channel_id: Snowflake, author_id: u64, author: &str, content: &str) -> Message {
    serde_json::from_value(json!({
        "id": id.to_string(),
        "channel_id": channel_id.to_string(),
        "guild_id": GUILD.to_string(),
        "author": { "id": author_id.to_string(), "username": author.to_lowercase(), "global_name": author },
        "content": content,
        "timestamp": "2026-11-18T10:20:30.000000+00:00"
    }))
    .expect("message fixture")
}

fn make_thread(id: Snowflake, name: &str) -> Channel {
    serde_json::from_value(json!({
        "id": id.to_string(),
        "guild_id": GUILD.to_string(),
        "name": name,
        "type": 11
    }))
    .expect("thread fixture")
}

#[gpui_kit::test]
fn ctrl_f_header_toggle_enter_once_and_seq_across_reopen(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend.clone(), events);

    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    focus_composer(&view, cx);

    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.search.is_some()));
    assert!(cx.debug_bounds("search-panel").is_some());
    let search_focused = cx.update(|window, cx| {
        view.read_with(cx, |v, cx| v.search.as_ref().is_some_and(|s| s.input.read(cx).focus_handle(cx).is_focused(window)))
    });
    assert!(search_focused, "Ctrl+F focuses search input");

    // Pressing Enter inside focused search input submits EXACTLY once
    view.update_in(cx, |v, window, cx| {
        if let Some(panel) = &v.search {
            panel.input.update(cx, |s, cx| s.set_value("first query", window, cx));
        }
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(lock_sent(&backend.sent_searches).len(), 1);
    let first_seq = view.read_with(cx, |v, _| v.search.as_ref().map(|s| s.request_seq)).unwrap();
    assert_eq!(first_seq, 1);

    // Esc closes panel and restores composer focus; reopening allocates a NEW seq
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.search.is_none()));
    assert!(cx.debug_bounds("search-panel").is_none());
    let composer_focused = cx.update(|window, cx| {
        view.read_with(cx, |v, cx| v.composer.read(cx).focus_handle(cx).is_focused(window))
    });
    assert!(composer_focused, "Escape returns focus to composer");

    let toggle_btn = cx.debug_bounds("toggle-search").expect("header search toggle drawn").center();
    cx.simulate_click(toggle_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.search.is_some()));

    view.update_in(cx, |v, window, cx| {
        if let Some(panel) = &v.search {
            panel.input.update(cx, |s, cx| s.set_value("second query", window, cx));
        }
        v.search_submit(cx);
    });
    cx.run_until_parked();
    let second_seq = view.read_with(cx, |v, _| v.search.as_ref().map(|s| s.request_seq)).unwrap();
    assert_eq!(second_seq, 2);

    // Late response from the closed panel (seq 1) must be ignored by reopened panel (seq 2)
    let stale_hit = make_message(msg_id(77), GENERAL, 201, "Alice", "stale result");
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::SearchLoaded {
                seq: first_seq,
                result: Ok(MessageSearchOutcome::Results(MessageSearchPage {
                    messages: vec![stale_hit],
                    channels: Vec::new(),
                    threads: Vec::new(),
                    total: 1,
                    hits_per_page: 25,
                    page: 1,
                    cursor: None,
                })),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.search.as_ref().is_some_and(|s| s.loading && s.hits.is_empty())));

    let toggle_btn = cx.debug_bounds("toggle-search").expect("header search toggle still drawn").center();
    cx.simulate_click(toggle_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.search.is_none()));

    drop(rt);
}

#[gpui_kit::test]
fn search_submit_filters_threads_pagination_errors_and_indexing(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend.clone(), events);

    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    focus_composer(&view, cx);

    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    draw(cx);

    view.update_in(cx, |v, window, cx| {
        if let Some(panel) = &v.search {
            panel.input.update(cx, |s, cx| s.set_value("hello has:image is:pinned", window, cx));
        }
        v.search_submit(cx);
    });
    cx.run_until_parked();
    draw(cx);

    {
        let sent = lock_sent(&backend.sent_searches);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].content.as_deref(), Some("hello"));
        assert_eq!(sent[0].has, Some(vec![MessageContentType::Image]));
        assert_eq!(sent[0].pinned, Some(true));
        assert_eq!(sent[0].scope, Some(MessageSearchScope::Current));
        assert_eq!(sent[0].channel_id, Some(vec![GENERAL.to_string()]));
        assert_eq!(sent[0].context_channel_id, Some(GENERAL.to_string()));
        assert_eq!(sent[0].context_guild_id, Some(GUILD.to_string()));
        assert_eq!(sent[0].sort_by, Some(MessageSearchSortBy::Timestamp));
    }

    // Stale seq 0 must be ignored
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::SearchLoaded { seq: 0, result: Ok(MessageSearchOutcome::Indexing) }], cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.search.as_ref().is_some_and(|s| s.loading && !s.indexing)));

    // Indexing status shows and clears loading
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::SearchLoaded { seq: 1, result: Ok(MessageSearchOutcome::Indexing) }], cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.search.as_ref().is_some_and(|s| !s.loading && s.indexing)));

    // Results page 1 with a thread hit + jumpable hit enables Load more
    let hit1 = make_message(msg_id(10), GENERAL, 201, "Alice", "hello page 1");
    let thread_hit = make_message(msg_id(11), THREAD, 202, "Bob", "hello from thread");
    let hit2 = make_message(msg_id(12), ANNOUNCEMENTS, 202, "Bob", "hello page 2");
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::SearchLoaded {
                seq: 1,
                result: Ok(MessageSearchOutcome::Results(MessageSearchPage {
                    messages: vec![hit1.clone(), thread_hit.clone()],
                    channels: Vec::new(),
                    threads: vec![make_thread(THREAD, "release-notes")],
                    total: 5,
                    hits_per_page: 2,
                    page: 1,
                    cursor: Some(vec!["cur-1".to_owned()]),
                })),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("search-hit").is_some());
    assert!(cx.debug_bounds("search-load-more").is_some());
    view.read_with(cx, |v, _| {
        let panel = v.search.as_ref().unwrap();
        assert_eq!(panel.hits.len(), 2);
        assert_eq!(panel.hits[0].channel_label.as_ref(), "#general");
        assert!(panel.hits[0].jumpable);
        assert_eq!(panel.hits[1].channel_label.as_ref(), "#release-notes");
        assert!(!panel.hits[1].jumpable);
        assert!(panel.has_more);
    });

    // Load more failure preserves existing hits and keeps Load more visible for retry
    view.update(cx, |v, cx| v.search_load_more(cx));
    cx.run_until_parked();
    {
        let sent = lock_sent(&backend.sent_searches);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].page, Some(2));
        assert_eq!(sent[1].cursor, Some(vec!["cur-1".to_owned()]));
    }
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::SearchLoaded { seq: 2, result: Err("timeout".to_owned()) }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("search-error").is_some());
    assert!(cx.debug_bounds("search-hit").is_some());
    assert!(cx.debug_bounds("search-load-more").is_some());
    assert_eq!(view.read_with(cx, |v, _| v.search.as_ref().map(|s| s.hits.len())), Some(2));

    // Retry Load more succeeds and short/cursorless page disables has_more cleanly even when total > hits.len()
    view.update(cx, |v, cx| v.search_load_more(cx));
    cx.run_until_parked();
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::SearchLoaded {
                seq: 3,
                result: Ok(MessageSearchOutcome::Results(MessageSearchPage {
                    messages: vec![hit1, hit2],
                    channels: Vec::new(),
                    threads: Vec::new(),
                    total: 5,
                    hits_per_page: 25,
                    page: 2,
                    cursor: None,
                })),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.search.as_ref().map(|s| (s.hits.len(), s.has_more))), Some((3, false)));
    assert!(cx.debug_bounds("search-load-more").is_none());

    // Scope switch to CurrentGuild re-submits without channel_id filter
    view.update(cx, |v, cx| v.search_set_scope(SearchScopeChoice::CurrentGuild, cx));
    cx.run_until_parked();
    {
        let sent = lock_sent(&backend.sent_searches);
        assert_eq!(sent.len(), 4);
        assert_eq!(sent[3].scope, Some(MessageSearchScope::Current));
        assert_eq!(sent[3].channel_id, None);
    }

    drop(rt);
}

#[gpui_kit::test]
fn jump_to_in_window_around_boundaries_and_jump_to_present(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend.clone(), events);

    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    lock_sent(&backend.page_loads).clear();

    // 1. In-window jump highlights message without extra REST page load
    view.update(cx, |v, cx| v.jump_to_channel_message(Some(GUILD), GENERAL, msg_id(1), cx));
    cx.run_until_parked();
    draw(cx);
    assert!(lock_sent(&backend.page_loads).is_empty(), "in-window jump should not trigger REST fetch");
    assert_eq!(view.read_with(cx, |v, _| v.highlighted_message), Some((GENERAL, msg_id(1))));
    assert!(cx.debug_bounds("highlighted-message").is_some());
    assert!(cx.debug_bounds("jump-to-present").is_none());

    // 2. Small-channel load_around (<50 msgs) does NOT falsely set has_more_after or show Jump to present
    let small_target = msg_id(40);
    view.update(cx, |v, cx| v.jump_to_channel_message(Some(GUILD), ANNOUNCEMENTS, small_target, cx));
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.page_loads), vec![(ANNOUNCEMENTS, LoadMode::Around, Some(small_target))]);
    let small_around = vec![
        make_message(msg_id(41), ANNOUNCEMENTS, 202, "Bob", "latest in small channel"),
        make_message(small_target, ANNOUNCEMENTS, 201, "Alice", "small channel target"),
    ];
    backend.state().messages.load_around(ANNOUNCEMENTS, small_around, small_target, Some(msg_id(41)));
    view.update(cx, |v, cx| {
        v.handle_events(vec![backend.history_loaded(ANNOUNCEMENTS, None)], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| (v.highlighted_message, v.has_more_after)), (Some((ANNOUNCEMENTS, small_target)), false));
    assert!(cx.debug_bounds("highlighted-message").is_some());
    assert!(cx.debug_bounds("jump-to-present").is_none());

    // 3. Full 50-message historical Around jump sets has_more_after=true and shows Jump to present
    lock_sent(&backend.page_loads).clear();
    let hist_target = msg_id(149);
    view.update(cx, |v, cx| v.jump_to_channel_message(Some(GUILD), GENERAL, hist_target, cx));
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.page_loads), vec![(GENERAL, LoadMode::Around, Some(hist_target))]);

    let around_50: Vec<Message> =
        (125..=174).rev().map(|n| make_message(msg_id(n), GENERAL, 201, "Alice", &format!("msg {n}"))).collect();
    backend.state().messages.load_around(GENERAL, around_50, hist_target, Some(msg_id(250)));
    view.update(cx, |v, cx| {
        v.handle_events(vec![backend.history_loaded(GENERAL, None)], cx);
    });
    cx.run_until_parked();
    draw(cx);

    assert_eq!(
        view.read_with(cx, |v, _| (v.pending_jump, v.highlighted_message, v.has_more_after, v.loading_history)),
        (None, Some((GENERAL, hist_target)), true, false)
    );
    assert_eq!(
        *lock_sent(&backend.page_loads),
        vec![(GENERAL, LoadMode::Around, Some(hist_target))],
        "jumping into a 50-message slice must not trigger spurious load_newer/load_older"
    );
    assert!(cx.debug_bounds("highlighted-message").is_some());
    assert!(cx.debug_bounds("jump-to-present").is_some());

    // 4. Jump to present fires Replace once and ignores duplicate click while loading_history=true
    lock_sent(&backend.page_loads).clear();
    let jtp_btn = cx.debug_bounds("jump-to-present-btn").expect("Jump to present button drawn").center();
    cx.simulate_click(jtp_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_click(jtp_btn, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.page_loads), vec![(GENERAL, LoadMode::Replace, None)]);

    let present_page = vec![make_message(msg_id(250), GENERAL, 201, "Alice", "latest present message")];
    backend.state().messages.load(GENERAL, present_page, LoadMode::Replace);
    view.update(cx, |v, cx| {
        v.handle_events(vec![backend.history_loaded(GENERAL, None)], cx);
    });
    cx.run_until_parked();
    draw(cx);

    assert_eq!(view.read_with(cx, |v, _| (v.highlighted_message, v.has_more_after)), (None, false));
    assert!(cx.debug_bounds("jump-to-present").is_none());

    drop(rt);
}

#[gpui_kit::test]
fn unread_bar_jumps_inside_the_loaded_page_and_marks_read(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    lock_sent(&backend.page_loads).clear();

    let jump = cx.debug_bounds("jump-to-unread-btn").unwrap().center();
    cx.simulate_click(jump, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.highlighted_message), Some((GENERAL, msg_id(2))));
    assert!(lock_sent(&backend.page_loads).is_empty());
    cx.executor().advance_clock(crate::ack::ACK_DELAY);
    cx.run_until_parked();
    assert!(backend.state().channel_unread(GENERAL).unread);

    let mark_read = cx.debug_bounds("mark-unread-bar-read-btn").unwrap().center();
    cx.simulate_click(mark_read, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(!backend.state().channel_unread(GENERAL).unread);
    assert_eq!(backend.state().channel_unread(GENERAL).mention_count, 0);
    assert!(cx.debug_bounds("unread-bar").is_none());
    assert!(view.read_with(cx, |v, _| v.rows.borrow().iter().all(|row| !row.new_divider)));
}

#[gpui_kit::test]
fn unread_jump_fetches_around_a_deleted_cursor_without_acknowledging_the_tail(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let read = msg_id(75);
    let latest = msg_id(150);
    {
        let mut state = backend.state();
        state.channels.get_mut(&GENERAL).unwrap().last_message_id = Some(latest);
        state.read_states.get_mut(&GENERAL).unwrap().ack_message_id = Some(read);
        let page = (101..=150).rev().map(|id| make_message(msg_id(id), GENERAL, 201, "Alice", "latest page")).collect();
        state.messages.load(GENERAL, page, LoadMode::Replace);
    }
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    lock_sent(&backend.page_loads).clear();

    let jump = cx.debug_bounds("jump-to-unread-btn").unwrap().center();
    cx.simulate_click(jump, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_click(jump, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(*lock_sent(&backend.page_loads), vec![(GENERAL, LoadMode::Around, Some(read))]);
    view.update(cx, |v, cx| {
        v.handle_events(vec![backend.history_loaded(ANNOUNCEMENTS, None)], cx);
    });
    assert!(view.read_with(cx, |v, _| v.loading_history));

    let around = (51..=100).rev().filter(|&id| id != 75)
        .map(|id| make_message(msg_id(id), GENERAL, 201, "Alice", "around read cursor")).collect();
    backend.state().messages.load_around(GENERAL, around, read, Some(latest));
    view.update(cx, |v, cx| {
        v.handle_events(vec![backend.history_loaded(GENERAL, None)], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.highlighted_message), Some((GENERAL, msg_id(76))));
    assert!(backend.state().messages.window(GENERAL).unwrap().has_more_before);
    assert!(view.read_with(cx, |v, _| v.has_more_after));
    cx.executor().advance_clock(crate::ack::ACK_DELAY);
    cx.run_until_parked();
    assert_eq!(backend.state().read_states[&GENERAL].ack_message_id, Some(read));

    let mark_read = cx.debug_bounds("mark-unread-bar-read-btn").unwrap().center();
    cx.simulate_click(mark_read, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(backend.state().read_states[&GENERAL].ack_message_id, Some(latest));
    assert_eq!(lock_sent(&backend.page_loads).last(), Some(&(GENERAL, LoadMode::Replace, None)));
    assert!(cx.debug_bounds("unread-bar").is_none());
}
