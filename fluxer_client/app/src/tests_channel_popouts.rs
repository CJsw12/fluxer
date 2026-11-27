//! Headless UI tests for channel header & composer popouts
//! (Notification Settings, Threads, Pinned Messages, Inbox, and Composer `+` menu).

use fluxer_client_core::Dispatch;
use fluxer_client_core::state::{Message, Snowflake, bits};
use gpui_kit::{Modifiers, TestAppContext};
use serde_json::json;

use crate::backend::{Backend, UiEvent, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{draw, offline_rest, open_client, runtime};
use crate::ui::channel_popouts::{ChannelPopoutKind, InboxTab, ThreadsTab};
use crate::ui::composer_text;
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);
const ANNOUNCE: Snowflake = Snowflake(12);
const THREAD_CH: Snowflake = Snowflake(31);

fn popouts_ready() -> Dispatch {
    let perms = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY | bits::ATTACH_FILES;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": "100", "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Dev Guild", "owner_id": "999", "default_message_notifications": 1 },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
                "channels": [
                    { "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0, "last_message_id": "500" },
                    { "id": ANNOUNCE.to_string(), "type": 0, "name": "announce", "position": 1, "last_message_id": "400" },
                    { "id": THREAD_CH.to_string(), "type": 11, "name": "bug-hunt", "parent_id": GENERAL.to_string(), "position": 0 }
                ],
                "members": [{ "user": { "id": "100", "username": "me" }, "roles": [] }]
            }],
            "private_channels": [],
            "read_states": [
                { "id": ANNOUNCE.to_string(), "last_message_id": "350", "mention_count": 2 }
            ],
            "user_guild_settings": [{
                "guild_id": GUILD.to_string(),
                "channel_overrides": {
                    ANNOUNCE.to_string(): { "channel_id": ANNOUNCE.to_string(), "muted": true, "collapsed": false, "message_notifications": 2 }
                }
            }]
        }),
    }
}

fn open_popouts_client(
    cx: &mut TestAppContext,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut gpui_kit::VisualTestContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&popouts_ready());
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

#[gpui_kit::test]
fn header_and_composer_popouts_switch_cleanly_and_toggle_close(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_popouts_client(cx);

    let bell = cx.debug_bounds("toggle-channel-mute").expect("bell btn").center();
    let threads = cx.debug_bounds("header-threads-btn").expect("threads btn").center();
    let pins = cx.debug_bounds("header-pins-btn").expect("pins btn").center();
    let inbox = cx.debug_bounds("header-inbox-btn").expect("inbox btn").center();
    let plus = cx.debug_bounds("composer-upload").expect("plus btn").center();

    // 1. Bell opens NotificationSettings; choosing Only mentions (1) PATCHes while keeping siblings.
    cx.simulate_click(bell, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, ChannelPopoutKind::NotificationSettings { .. })
    )));
    let level_1 = cx.debug_bounds("notif-level-1").expect("Only mentions option").center();
    cx.simulate_click(level_1, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    {
        let sent = lock_sent(&backend.sent_guild_settings);
        assert_eq!(sent.len(), 1);
        let overrides = sent[0].1.channel_overrides.clone().flatten().expect("overrides");
        assert_eq!(overrides[&GENERAL.to_string()].message_notifications, Some(1));
        assert_eq!(overrides[&GENERAL.to_string()].muted, Some(false));
        assert_eq!(overrides[&ANNOUNCE.to_string()].message_notifications, Some(2));
        assert_eq!(overrides[&ANNOUNCE.to_string()].muted, Some(true));
    }

    // 2. Direct switching Bell -> Threads -> Pins -> Inbox -> Plus without Escape, then repeat-click closes Plus.
    cx.simulate_click(bell, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_click(threads, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, ChannelPopoutKind::Threads { tab: ThreadsTab::Active, .. })
    )));
    // Switch thread tab and filter query.
    let closed_tab = cx.debug_bounds("threads-tab-Closed").expect("Closed threads tab").center();
    cx.simulate_click(closed_tab, Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, ChannelPopoutKind::Threads { tab: ThreadsTab::Closed, .. })
    )));

    cx.simulate_click(pins, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let pins_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::ChannelPopout(p) => match p.kind {
            ChannelPopoutKind::PinnedMessages { seq, .. } => seq,
            _ => panic!("expected PinnedMessages"),
        },
        _ => panic!("expected ChannelPopout"),
    });
    // Stale seq must be ignored, matching seq populates items.
    let pin_msg: Message = serde_json::from_value(json!({
        "id": "901",
        "channel_id": GENERAL.to_string(),
        "author": { "id": "201", "username": "alice" },
        "content": "remember this rule",
        "timestamp": "2026-11-18T10:20:30+00:00",
        "pinned": true
    }))
    .expect("pin message");
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::PinsLoaded { seq: pins_seq.wrapping_sub(1), channel_id: GENERAL, result: Err("stale".into()) }], cx);
        v.handle_events(vec![UiEvent::PinsLoaded { seq: pins_seq, channel_id: GENERAL, result: Ok(vec![pin_msg]) }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(&p.kind, ChannelPopoutKind::PinnedMessages { items, .. } if items.len() == 1)
    )));

    cx.simulate_click(inbox, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, ChannelPopoutKind::Inbox { tab: InboxTab::Unread })
    )));
    let mentions_tab = cx.debug_bounds("inbox-tab-Mentions").expect("Mentions inbox tab").center();
    cx.simulate_click(mentions_tab, Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, ChannelPopoutKind::Inbox { tab: InboxTab::Mentions })
    )));

    cx.simulate_click(plus, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, ChannelPopoutKind::ComposerPlusMenu)
    )));

    // Repeat-clicking Plus closes the menu cleanly.
    cx.simulate_click(plus, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Typing text then clicking Upload your message as a file queues message.txt and clears composer.
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    cx.update(|window, cx| {
        composer.update(cx, |s, cx| s.set_value("draft content", window, cx));
    });
    cx.simulate_click(plus, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let as_file = cx.debug_bounds("composer-menu-upload-text").expect("upload text item").center();
    cx.simulate_click(as_file, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        matches!(v.overlay, Overlay::None)
            && v.attachments.files(GENERAL).len() == 1
            && v.attachments.files(GENERAL)[0].file.name == "message.txt"
    }));
    assert_eq!(composer.read_with(cx, |s, _| composer_text::wire_text(s)), "");
    drop(rt);
}

#[gpui_kit::test]
fn escape_closes_user_area_popout_without_prior_focus(cx: &mut TestAppContext) {
    let (rt, _backend, view, cx) = open_popouts_client(cx);
    cx.update(|window, cx| window.blur(cx));
    view.update_in(cx, |v, window, cx| {
        v.toggle_user_area_popout(cx);
        assert!(v.user_area_popout_open);
        let _ = window;
    });
    cx.run_until_parked();
    draw(cx);
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.user_area_popout_open));
    drop(rt);
}

const CH_EXTRA_1: Snowflake = Snowflake(13);
const CH_EXTRA_2: Snowflake = Snowflake(14);
const CH_EXTRA_3: Snowflake = Snowflake(15);
const DM_CH: Snowflake = Snowflake(50);

fn msg_actions_ready() -> Dispatch {
    let perms = bits::VIEW_CHANNEL
        | bits::SEND_MESSAGES
        | bits::READ_MESSAGE_HISTORY
        | bits::ATTACH_FILES
        | bits::PIN_MESSAGES
        | bits::CREATE_PUBLIC_THREADS;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s-msg",
            "user": { "id": "100", "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Dev Guild", "owner_id": "999", "default_message_notifications": 0 },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
                "channels": [
                    { "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 },
                    { "id": ANNOUNCE.to_string(), "type": 0, "name": "announce", "position": 1 },
                    { "id": CH_EXTRA_1.to_string(), "type": 0, "name": "random", "position": 2 },
                    { "id": CH_EXTRA_2.to_string(), "type": 0, "name": "offtopic", "position": 3 },
                    { "id": CH_EXTRA_3.to_string(), "type": 0, "name": "media", "position": 4 },
                    { "id": THREAD_CH.to_string(), "type": 11, "name": "bug-hunt", "parent_id": GENERAL.to_string(), "position": 0 }
                ],
                "members": [{ "user": { "id": "100", "username": "me" }, "roles": [] }]
            }],
            "private_channels": [{
                "id": DM_CH.to_string(),
                "type": 1,
                "recipients": [{ "id": "201", "username": "alice", "global_name": "Alice" }]
            }],
            "read_states": []
        }),
    }
}

fn fixture_msg(channel_id: Snowflake, id: Snowflake, content: &str) -> Message {
    serde_json::from_value(json!({
        "id": id.to_string(),
        "channel_id": channel_id.to_string(),
        "author": { "id": "201", "username": "alice", "global_name": "Alice" },
        "content": content,
        "timestamp": "2026-11-18T10:20:30+00:00"
    }))
    .expect("fixture message")
}

fn open_msg_actions_client(
    cx: &mut TestAppContext,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut gpui_kit::VisualTestContext) {
    use crate::tests::msg_id;
    use fluxer_client_core::state::LoadMode;

    let rt = runtime();
    let (mut backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.set_webapp_for_test(Some("https://web.fluxer.test".to_owned()));
    {
        let mut state = backend.state();
        state.apply(&msg_actions_ready());
        let page = vec![
            fixture_msg(GENERAL, msg_id(3), "third line"),
            fixture_msg(GENERAL, msg_id(2), "second message"),
            fixture_msg(GENERAL, msg_id(1), "**hello** & world"),
        ];
        state.messages.load(GENERAL, page, LoadMode::Replace);
        state.messages.load(DM_CH, vec![fixture_msg(DM_CH, msg_id(9), "dm ping")], LoadMode::Replace);
    }
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

#[gpui_kit::test]
fn message_actions_copy_link_pin_bookmark_translate_and_speak(cx: &mut TestAppContext) {
    use crate::tests::msg_id;
    use crate::ui::message_actions::{self, ActionTarget, MessageAction};
    use gpui_kit::component::WindowExt as _;

    let (rt, backend, view, cx) = open_msg_actions_client(cx);

    let t1 = view.read_with(cx, |v, _| v.rows.borrow()[0].actions.clone());
    assert_eq!(t1.message_id, msg_id(1));
    assert!(t1.can_pin && !t1.pinned && !t1.bookmarked);

    // 1. CopyLink: guild channel and DM.
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::CopyLink, &t1, window, cx));
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
        Some(format!("https://web.fluxer.test/channels/{}/{}/{}", GUILD.0, GENERAL.0, msg_id(1).0).as_str())
    );
    let dm_target = {
        let state = backend.state();
        let dm_msg = state.messages.message(DM_CH, msg_id(9)).expect("dm message");
        ActionTarget::of(&state, dm_msg)
    };
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::CopyLink, &dm_target, window, cx));
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
        Some(format!("https://web.fluxer.test/channels/@me/{}/{}", DM_CH.0, msg_id(9).0).as_str())
    );

    // 2. Pin confirm (Escape cancels, Enter confirms) + PinNow immediate unpin + tooltip updates.
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Pin, &t1, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    assert!(!backend.state().messages.message(GENERAL, msg_id(1)).unwrap().pinned);

    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Pin, &t1, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw(cx);
    assert!(backend.state().messages.message(GENERAL, msg_id(1)).unwrap().pinned);
    let pinned_target = view.read_with(cx, |v, _| v.rows.borrow()[0].actions.clone());
    assert!(pinned_target.pinned);
    assert_eq!(message_actions::tooltip(MessageAction::Pin, &pinned_target), "Unpin message");

    // Shift+Pin (PinNow) skips the dialog.
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::PinNow, &pinned_target, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    assert!(!backend.state().messages.message(GENERAL, msg_id(1)).unwrap().pinned);
    let unpinned_target = view.read_with(cx, |v, _| v.rows.borrow()[0].actions.clone());
    assert_eq!(message_actions::tooltip(MessageAction::Pin, &unpinned_target), "Pin message");

    // 3. Bookmark toggle + Inbox Bookmarks tab + Remove button + preserved empty state.
    let t2 = view.read_with(cx, |v, _| v.rows.borrow()[1].actions.clone());
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Bookmark, &t2, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(backend.state().is_message_saved(msg_id(2)));
    let saved_t2 = view.read_with(cx, |v, _| v.rows.borrow()[1].actions.clone());
    assert!(saved_t2.bookmarked);
    assert_eq!(message_actions::tooltip(MessageAction::Bookmark, &saved_t2), "Remove bookmark");

    let inbox_btn = cx.debug_bounds("header-inbox-btn").expect("inbox btn").center();
    cx.simulate_click(inbox_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let bookmarks_tab = cx.debug_bounds("inbox-tab-Bookmarks").expect("bookmarks tab").center();
    cx.simulate_click(bookmarks_tab, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let row_sel: &'static str = Box::leak(format!("inbox-bookmark-{}", msg_id(2).0).into_boxed_str());
    assert!(cx.debug_bounds(row_sel).is_some(), "bookmarked message rendered in Inbox");
    let remove_sel: &'static str = Box::leak(format!("bookmark-remove-{}", msg_id(2).0).into_boxed_str());
    let remove_center = cx.debug_bounds(remove_sel).expect("bookmark remove btn").center();
    cx.simulate_click(remove_center, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(!backend.state().is_message_saved(msg_id(2)));
    assert!(cx.debug_bounds(row_sel).is_none(), "removed bookmark disappeared and fell back to empty view");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);

    // 4. Translate direct action and hover submenu provider.
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Translate, &t1, window, cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.opened_urls.last().cloned()),
        Some("https://translate.google.com/?sl=auto&tl=auto&text=**hello**%20%26%20world&op=translate".to_owned())
    );
    view.update_in(cx, |v, window, cx| {
        v.open_message_menu_at(t1.clone(), gpui_kit::point(gpui_kit::px(300.), gpui_kit::px(220.)), window, cx);
        v.set_message_menu_translate_submenu(true, cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("message-translate-submenu").is_some());
    let deepl = cx.debug_bounds("translate-provider-deepl").expect("deepl item").center();
    cx.simulate_click(deepl, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.opened_urls.last().cloned()),
        Some("https://www.deepl.com/translator#auto/auto/**hello**%20%26%20world".to_owned())
    );

    // 5. Speak TTS toggle.
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Speak, &t1, window, cx));
    assert!(view.read_with(cx, |v, _| v.tts.is_speaking() && v.tts.spoken() == vec!["hello & world"]));
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Speak, &t1, window, cx));
    assert!(view.read_with(cx, |v, _| !v.tts.is_speaking()));

    drop(rt);
}

#[gpui_kit::test]
fn mark_unread_moves_read_state_and_suppresses_auto_ack(cx: &mut TestAppContext) {
    use crate::ack::ACK_DELAY;
    use crate::tests::msg_id;
    use crate::ui::message_actions::MessageAction;
    use fluxer_client_core::state::Snowflake;

    let (rt, backend, view, cx) = open_msg_actions_client(cx);
    let t2 = view.read_with(cx, |v, _| v.rows.borrow()[1].actions.clone());
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::MarkUnread, &t2, window, cx));
    cx.run_until_parked();
    draw(cx);

    let ack_of = || backend.state().read_states.get(&GENERAL).and_then(|r| r.ack_message_id);
    assert_eq!(ack_of(), Some(msg_id(1)));
    assert_eq!(view.read_with(cx, |v, _| (v.unread_anchor, v.manual_unread_channel)), (Some(msg_id(1)), Some(GENERAL)));

    // Auto-ack must remain suppressed past ACK_DELAY.
    view.update(cx, |v, cx| v.maybe_ack(cx));
    cx.executor().advance_clock(ACK_DELAY);
    cx.run_until_parked();
    assert_eq!(ack_of(), Some(msg_id(1)));
    assert_eq!(view.read_with(cx, |v, _| v.manual_unread_channel), Some(GENERAL));

    // Explicit MarkUnread on first message steps back 1ms.
    let t1 = view.read_with(cx, |v, _| v.rows.borrow()[0].actions.clone());
    let before_t1 = Snowflake::at_previous_millisecond(msg_id(1));
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::MarkUnread, &t1, window, cx));
    assert_eq!(ack_of(), Some(before_t1));
    assert_eq!(view.read_with(cx, |v, _| v.unread_anchor), Some(before_t1));

    // Explicit mark_channel_read clears manual unread suppression.
    view.update(cx, |v, cx| v.mark_channel_read(cx));
    assert_eq!(view.read_with(cx, |v, _| (v.unread_anchor, v.manual_unread_channel)), (None, None));
    drop(rt);
}

#[gpui_kit::test]
fn forward_modal_caps_at_five_and_sends_reference_plus_comment(cx: &mut TestAppContext) {
    use crate::ui::forward_modal::MAX_FORWARD_DESTINATIONS;
    use crate::ui::message_actions::MessageAction;

    let (rt, backend, view, cx) = open_msg_actions_client(cx);
    let t1 = view.read_with(cx, |v, _| v.rows.borrow()[0].actions.clone());
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Forward, &t1, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::ForwardMessage(_))));

    // Pick 4 visible writable destinations, then filter to pick the 5th and verify a 6th is capped at 5.
    for ch in [DM_CH, GENERAL, ANNOUNCE, CH_EXTRA_1] {
        let sel: &'static str = Box::leak(format!("forward-dest-{}", ch.0).into_boxed_str());
        let p = cx.debug_bounds(sel).unwrap_or_else(|| panic!("{sel} missing")).center();
        cx.simulate_click(p, Modifiers::default());
        cx.run_until_parked();
        draw(cx);
    }
    let (query_input, comment_input) = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::ForwardMessage(m) => (m.query.clone(), m.comment.clone()),
        _ => unreachable!(),
    });
    cx.update(|window, cx| query_input.update(cx, |s, cx| s.set_value("offtopic", window, cx)));
    cx.run_until_parked();
    draw(cx);
    let p_extra2 = cx.debug_bounds(Box::leak(format!("forward-dest-{}", CH_EXTRA_2.0).into_boxed_str())).expect("CH_EXTRA_2 missing").center();
    cx.simulate_click(p_extra2, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    cx.update(|window, cx| query_input.update(cx, |s, cx| s.set_value("media", window, cx)));
    cx.run_until_parked();
    draw(cx);
    let p_extra3 = cx.debug_bounds(Box::leak(format!("forward-dest-{}", CH_EXTRA_3.0).into_boxed_str())).expect("CH_EXTRA_3 missing").center();
    cx.simulate_click(p_extra3, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        let Overlay::ForwardMessage(m) = &v.overlay else { panic!("expected forward modal") };
        assert_eq!(m.selected.len(), MAX_FORWARD_DESTINATIONS);
        assert!(!m.selected.contains(&CH_EXTRA_3));
    });

    // Narrow selection to DM_CH only, add comment, and submit.
    cx.update(|window, cx| query_input.update(cx, |s, cx| s.set_value("offtopic", window, cx)));
    cx.run_until_parked();
    draw(cx);
    let p_extra2 = cx.debug_bounds(Box::leak(format!("forward-dest-{}", CH_EXTRA_2.0).into_boxed_str())).expect("CH_EXTRA_2 missing").center();
    cx.simulate_click(p_extra2, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| query_input.update(cx, |s, cx| s.set_value("", window, cx)));
    cx.run_until_parked();
    draw(cx);
    for ch in [GENERAL, ANNOUNCE, CH_EXTRA_1] {
        let sel: &'static str = Box::leak(format!("forward-dest-{}", ch.0).into_boxed_str());
        let p = cx.debug_bounds(sel).expect("dest bounds").center();
        cx.simulate_click(p, Modifiers::default());
        cx.run_until_parked();
        draw(cx);
    }
    cx.update(|window, cx| comment_input.update(cx, |s, cx| s.set_value("check this", window, cx)));
    draw(cx);
    let submit = cx.debug_bounds("forward-submit-btn").expect("submit btn").center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    {
        let sent = lock_sent(&backend.sent);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, DM_CH);
        assert_eq!(sent[0].1.content.as_deref(), Some(""));
        let r = sent[0].1.message_reference.as_ref().expect("forward reference");
        assert_eq!((r.kind, r.message_id.as_str(), r.channel_id.as_deref(), r.guild_id.as_deref()), (
            Some(1),
            t1.message_id.to_string().as_str(),
            Some(GENERAL.to_string().as_str()),
            Some(GUILD.to_string().as_str()),
        ));
        assert_eq!(sent[1].0, DM_CH);
        assert_eq!(sent[1].1.content.as_deref(), Some("check this"));
        assert!(sent[1].1.message_reference.is_none());
    }

    let seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::ForwardMessage(m) => m.seq,
        _ => unreachable!(),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::MessageForwarded { seq, target_channels: vec![DM_CH], error: None }], cx);
    });
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    drop(rt);
}

#[gpui_kit::test]
fn create_thread_modal_validates_submits_and_selects_new_thread(cx: &mut TestAppContext) {
    use crate::ui::message_actions::MessageAction;

    let (rt, backend, view, cx) = open_msg_actions_client(cx);
    let t2 = view.read_with(cx, |v, _| v.rows.borrow()[1].actions.clone());
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::CreateThread, &t2, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::CreateThread(_))));

    // Empty name triggers validation error and sends nothing.
    let submit = cx.debug_bounds("create-thread-submit-btn").expect("create thread submit").center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        let Overlay::CreateThread(m) = &v.overlay else { panic!("expected create thread modal") };
        assert!(m.error.is_some());
    });
    assert!(lock_sent(&backend.sent_thread_creates).is_empty());

    // Enter thread name, select 3 Days (4320), submit, and handle ThreadCreated.
    let name_input = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::CreateThread(m) => m.name_input.clone(),
        _ => unreachable!(),
    });
    cx.update(|window, cx| name_input.update(cx, |s, cx| s.set_value("  bug triage  ", window, cx)));
    let archive = cx.debug_bounds("thread-archive-4320").expect("4320 archive btn").center();
    cx.simulate_click(archive, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("create-thread-submit-btn").expect("create thread submit").center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    {
        let sent = lock_sent(&backend.sent_thread_creates);
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0].0, sent[0].1), (GENERAL, t2.message_id));
        assert_eq!(sent[0].2.name, "bug triage");
        assert_eq!(sent[0].2.auto_archive_duration, Some(4320));
    }

    let thread_id = Snowflake(399);
    backend.state().apply(&Dispatch {
        t: "THREAD_CREATE".to_owned(),
        s: 2,
        d: json!({
            "id": thread_id.to_string(),
            "type": 11,
            "guild_id": GUILD.to_string(),
            "parent_id": GENERAL.to_string(),
            "name": "bug triage"
        }),
    });
    let seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::CreateThread(m) => m.seq,
        _ => unreachable!(),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::ThreadCreated { seq, result: Ok(thread_id) }], cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None) && v.selected_channel == Some(thread_id)));
    drop(rt);
}

#[gpui_kit::test]
fn right_click_message_row_opens_context_menu_only_for_sent_messages(cx: &mut TestAppContext) {
    use crate::tests::msg_id;
    use fluxer_client_core::state::MessageState;
    use gpui_kit::MouseButton;

    let (rt, backend, view, cx) = open_msg_actions_client(cx);

    let sent_sel: &'static str = Box::leak(format!("message-{}", msg_id(1).0).into_boxed_str());
    let sent_pt = cx.debug_bounds(sent_sel).expect("sent message bounds").center();
    cx.simulate_mouse_down(sent_pt, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(&p.kind, ChannelPopoutKind::MessageMenu { at_cursor: true, target, .. } if target.message_id == msg_id(1))
    )));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Queue an unsent message: right-clicking it must NOT open MessageMenu.
    assert!(backend.send_message(GENERAL, "pending unsent".to_owned()));
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    let pending_id = view.read_with(cx, |v, _| {
        let rows = v.rows.borrow();
        let last = rows.last().expect("pending row");
        assert_eq!(last.state, MessageState::Sending);
        last.id
    });
    let pending_sel: &'static str = Box::leak(format!("message-{}", pending_id.0).into_boxed_str());
    let pending_pt = cx.debug_bounds(pending_sel).expect("pending message bounds").center();
    cx.simulate_mouse_down(pending_pt, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    drop(rt);
}

#[gpui_kit::test]
fn hovering_mute_channel_opens_duration_submenu_without_panic(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_popouts_client(cx);
    let bell = cx.debug_bounds("toggle-channel-mute").expect("bell btn").center();
    cx.simulate_click(bell, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let trigger = cx.debug_bounds("notif-popout-mute").expect("mute trigger").center();
    cx.simulate_mouse_move(trigger, None, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("mute-channel-submenu").is_some(), "duration submenu rendered");
    let item = cx.debug_bounds("mute-duration-15m").expect("15m item").center();
    cx.simulate_mouse_move(item, None, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_click(item, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let sent = lock_sent(&backend.sent_guild_settings);
    let overrides = sent.last().expect("settings PATCH").1.channel_overrides.clone().flatten().expect("overrides");
    assert_eq!(overrides[&GENERAL.to_string()].muted, Some(true));
}
