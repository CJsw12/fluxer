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
