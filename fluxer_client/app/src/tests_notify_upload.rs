//! Headless tests for notifications (fake toaster / sound player) and the
//! attachment tray and multipart send (the backend's test log).

use bytes::Bytes;
use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::UploadSource;
use fluxer_client_core::state::{ATTACHMENT_FLAG_SPOILER, MessageState, NotificationKind, Snowflake};
use gpui_kit::{ClipboardItem, TestAppContext};
use serde_json::json;

use crate::backend::{UiEvent, UploadFailure, lock_sent};
use crate::notify::Sound;
use crate::notify::tests::{Recorder, notification};
use crate::tests::{backend_with_fixture, draw, focus_composer, open_client, open_general, runtime};
use crate::ui::attachments::{NewFile, Preview, text_file};
use crate::view_model::Place;

const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);

fn upload_failure(nonce: Snowflake, message: &str, uncertain: bool) -> UiEvent {
    UiEvent::UploadEnded { nonce, error: Some(UploadFailure { message: message.to_owned(), uncertain }) }
}

fn bytes_file(name: &str, size: u64) -> NewFile {
    NewFile {
        source: UploadSource::Bytes(Bytes::from_static(b"data")),
        name: name.to_owned(),
        mime: crate::ui::attachments::mime_for(name).map(str::to_owned),
        size,
        preview: Preview::None,
    }
}

#[gpui_kit::test]
fn notify_events_reach_the_toaster_and_sound_player(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);
    let rec = Recorder::default();
    view.update(cx, |v, _| v.notifier = rec.notifier());

    let toast = notification(NotificationKind::Message, 9001);
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::Notify(toast)], cx));
    let same = notification(NotificationKind::SameChannelSound, 9002);
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::Notify(same)], cx));
    assert_eq!(rec.toasts.borrow().len(), 1, "same-channel message: sound only");
    assert_eq!(rec.toasts.borrow()[0].title, "Ali #general (Rust Port)");
    assert_eq!(rec.toasts.borrow()[0].body, "hey @me");
    assert_eq!(*rec.sounds.borrow(), [Sound::Message, Sound::SameChannel]);

    // The header bell turns both off.
    view.update(cx, |v, cx| v.toggle_notifications(cx));
    let muted = notification(NotificationKind::DirectMessage, 9003);
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::Notify(muted)], cx));
    assert_eq!((rec.toasts.borrow().len(), rec.sounds.borrow().len()), (1, 2));
    drop(rt);
}

#[gpui_kit::test]
fn toast_click_opens_the_channel_and_focus_is_shared(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend.clone(), events);
    assert_eq!(backend.notify_focus().viewing(), None);
    let click = UiEvent::NotificationClicked { guild_id: Some(GUILD), channel_id: GENERAL };
    view.update(cx, |v, cx| v.handle_events(vec![click], cx));
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        assert_eq!((v.place, v.selected_channel), (Place::Guild(GUILD), Some(GENERAL)));
    });
    assert_eq!(backend.notify_focus().viewing(), Some(GENERAL), "the gateway task sees the viewed channel");
    drop(rt);
}

#[gpui_kit::test]
fn tray_add_remove_spoiler_and_limits(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend, events);

    view.update(cx, |v, cx| v.add_files(vec![bytes_file("a.png", 4), bytes_file("SPOILER_b.txt", 4)], cx));
    let ids = view.read_with(cx, |v, _| {
        let files = v.attachments.files(GENERAL);
        assert_eq!(files.iter().map(|f| f.file.name.as_str()).collect::<Vec<_>>(), ["a.png", "SPOILER_b.txt"]);
        assert_eq!(files.iter().map(|f| f.spoiler).collect::<Vec<_>>(), [false, true], "SPOILER_ prefix");
        files.iter().map(|f| f.id).collect::<Vec<_>>()
    });
    draw(cx);
    view.update(cx, |v, cx| v.toggle_attachment_spoiler(ids[0], cx));
    view.update(cx, |v, cx| v.remove_attachment(ids[1], cx));
    view.read_with(cx, |v, _| {
        let files = v.attachments.files(GENERAL);
        assert_eq!(files.len(), 1);
        assert!(files[0].spoiler);
    });

    // Ten in total at most, counting the queued one; nothing is added.
    let eleven: Vec<NewFile> = (0..10).map(|i| bytes_file(&format!("f{i}.bin"), 1)).collect();
    view.update(cx, |v, cx| v.add_files(eleven, cx));
    view.read_with(cx, |v, _| {
        assert_eq!(v.attachments.files(GENERAL).len(), 1);
        let error = v.error.clone().expect("error shown");
        assert!(error.contains("up to 10 files"), "{error}");
    });

    // Over 25 MiB.
    view.update(cx, |v, cx| v.add_files(vec![bytes_file("big.mp4", 26 * 1024 * 1024)], cx));
    view.read_with(cx, |v, _| {
        assert_eq!(v.attachments.files(GENERAL).len(), 1);
        let error = v.error.clone().expect("error shown");
        assert_eq!(error.as_ref(), "big.mp4 is too large (26.0 MB; the limit is 25.0 MB).");
    });
    drop(rt);
}

#[gpui_kit::test]
fn long_paste_becomes_message_txt(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend, events);
    let short = ClipboardItem::new_string("short".to_owned());
    assert!(!view.update(cx, |v, cx| v.paste(&short, cx)), "short text goes into the input");
    let long = "x".repeat(2001);
    assert!(view.update(cx, |v, cx| v.paste(&ClipboardItem::new_string(long.clone()), cx)));
    view.read_with(cx, |v, _| {
        let files = v.attachments.files(GENERAL);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file, text_file(&long));
        assert_eq!((files[0].file.name.as_str(), files[0].file.size), ("message.txt", 2001));
    });
    drop(rt);
}

#[gpui_kit::test]
fn enter_sends_text_and_files_as_one_multipart_message(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.add_files(vec![bytes_file("cat.png", 4), bytes_file("notes.txt", 4)], cx));
    let first = view.read_with(cx, |v, _| v.attachments.files(GENERAL)[0].id);
    view.update(cx, |v, cx| v.toggle_attachment_spoiler(first, cx));
    draw(cx);

    focus_composer(&view, cx);
    cx.simulate_input("look");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw(cx);

    assert!(lock_sent(&backend.sent).is_empty(), "no plain JSON POST");
    let upload = lock_sent(&backend.sent_uploads).last().cloned().expect("a multipart POST");
    assert_eq!(upload.channel_id, GENERAL);
    assert_eq!(upload.request.content.as_deref(), Some("look"));
    assert_eq!(upload.max_file_size, 25 * 1024 * 1024);
    let parts: Vec<(&str, Option<&str>, u32)> =
        upload.files.iter().map(|f| (f.filename.as_str(), f.content_type.as_deref(), f.flags)).collect();
    assert_eq!(
        parts,
        [("cat.png", Some("image/png"), ATTACHMENT_FLAG_SPOILER), ("notes.txt", Some("text/plain"), 0)],
        "a spoiler keeps its file name"
    );
    let nonce: Snowflake = upload.request.nonce.parse::<u64>().map(Snowflake).expect("nonce");
    view.read_with(cx, |v, _| {
        assert!(v.attachments.files(GENERAL).is_empty(), "tray emptied");
        let rows = v.rows.borrow();
        let last = rows.last().expect("pending row");
        assert_eq!(last.id, nonce);
        assert_eq!(last.state, MessageState::Sending);
        assert_eq!(last.upload_percent, Some(0));
        assert!(last.body.contains("📎 cat.png"), "{}", last.body);
    });

    // Progress events update the pending row.
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::UploadProgress { nonce, percent: 42 }], cx));
    view.read_with(cx, |v, _| assert_eq!(v.rows.borrow().last().and_then(|r| r.upload_percent), Some(42)));
    draw(cx);
    drop(rt);
}

/// Files only (no text) still send; Enter in an empty composer otherwise
/// does nothing.
#[gpui_kit::test]
fn files_without_text_send(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    focus_composer(&view, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(lock_sent(&backend.sent_uploads).is_empty());
    view.update(cx, |v, cx| v.add_files(vec![bytes_file("a.zip", 4)], cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let upload = lock_sent(&backend.sent_uploads).last().cloned().expect("multipart POST");
    assert_eq!((upload.request.content, upload.files.len()), (None, 1));
    drop(rt);
}

/// A failed upload puts its files back. A gateway-confirmed upload cannot
/// restore files on a late HTTP error.
#[gpui_kit::test]
fn failed_upload_restores_the_files(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.add_files(vec![bytes_file("cat.png", 4)], cx));
    focus_composer(&view, cx);
    cx.simulate_input("look");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let upload = lock_sent(&backend.sent_uploads).last().cloned().expect("a multipart POST");
    let nonce: Snowflake = upload.request.nonce.parse::<u64>().map(Snowflake).expect("nonce");
    view.read_with(cx, |v, _| assert!(v.attachments.files(GENERAL).is_empty(), "tray emptied while sending"));

    backend.state().messages.mark_failed(nonce);
    view.update(cx, |v, cx| {
        v.handle_events(vec![upload_failure(nonce, "upload failed", false)], cx);
    });
    view.read_with(cx, |v, _| {
        let names: Vec<&str> = v.attachments.files(GENERAL).iter().map(|f| f.file.name.as_str()).collect();
        assert_eq!(names, ["cat.png"], "back in the tray to send again");
        assert!(v.sent_files.is_empty());
    });

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let upload = lock_sent(&backend.sent_uploads).last().cloned().expect("retried upload");
    let retry = Snowflake::parse(&upload.request.nonce).unwrap();
    assert_ne!(retry, nonce);
    backend.state().apply(&Dispatch {
        t: "MESSAGE_CREATE".to_owned(),
        s: 3,
        d: json!({
            "id": crate::tests::msg_id(10), "channel_id": GENERAL, "nonce": retry,
            "author": { "id": "100", "username": "me" }, "content": "sent",
        }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![upload_failure(retry, "late transport failure", true)], cx));
    assert!(view.read_with(cx, |v, _| v.attachments.files(GENERAL).is_empty() && v.sent_files.is_empty() && v.error.is_none()));
    drop(rt);
}

#[gpui_kit::test]
fn failed_upload_restores_files_without_a_message_window(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.add_files(vec![bytes_file("retry.txt", 4)], cx));
    focus_composer(&view, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let upload = lock_sent(&backend.sent_uploads).last().cloned().expect("a multipart POST");
    let nonce = Snowflake::parse(&upload.request.nonce).unwrap();

    {
        let mut state = backend.state();
        state.apply(&Dispatch {
            t: "GUILD_DELETE".to_owned(),
            s: 2,
            d: json!({ "id": GUILD, "unavailable": true }),
        });
        state.messages.select(None);
        for id in 0..fluxer_client_core::state::MAX_CACHED_CHANNELS {
            state.messages.load(Snowflake(1000 + id as u64), Vec::new(), fluxer_client_core::state::LoadMode::Replace);
        }
    }
    assert!(backend.state().messages.window(GENERAL).is_none());
    view.update(cx, |v, cx| {
        v.handle_events(vec![upload_failure(nonce, "upload interrupted", true)], cx);
    });
    view.read_with(cx, |v, _| {
        assert_eq!(v.attachments.files(GENERAL)[0].file.name, "retry.txt");
        assert!(v.sent_files.is_empty());
        assert!(v.error.as_ref().is_some_and(|error| error.contains("Check the channel before retrying")));
    });
}

#[gpui_kit::test]
fn failed_upload_keeps_newly_queued_files_and_checks_the_next_send_limit(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    view.update(cx, |v, cx| {
        v.attachments.max_count = 1;
        v.add_files(vec![bytes_file("first.txt", 4)], cx);
    });
    focus_composer(&view, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let upload = lock_sent(&backend.sent_uploads).last().cloned().unwrap();
    let nonce = Snowflake::parse(&upload.request.nonce).unwrap();
    view.update(cx, |v, cx| {
        v.add_files(vec![bytes_file("second.txt", 4)], cx);
        v.handle_events(vec![upload_failure(nonce, "upload failed", false)], cx);
    });
    cx.simulate_input("still my draft");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(lock_sent(&backend.sent_uploads).len(), 1);
    assert!(view.read_with(cx, |v, cx| {
        let names: Vec<_> = v.attachments.files(GENERAL).iter().map(|file| file.file.name.as_str()).collect();
        names == ["first.txt", "second.txt"]
            && v.composer.read(cx).value().as_ref() == "still my draft"
            && v.error.as_ref().is_some_and(|error| error.contains("up to 1"))
    }));
}

/// Revoking ATTACH_FILES blocks both adding files and sending existing ones.
#[gpui_kit::test]
fn attach_files_permission_gates_the_tray(cx: &mut TestAppContext) {
    use fluxer_client_core::state::bits;
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.add_files(vec![bytes_file("already-queued.png", 4)], cx));
    let composer = focus_composer(&view, cx);
    cx.simulate_input("keep this text");
    let send_only = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    backend.state().apply(&Dispatch {
        t: "GUILD_ROLE_UPDATE".to_owned(),
        s: 2,
        d: serde_json::json!({ "guild_id": GUILD.to_string(),
            "role": { "id": GUILD.to_string(), "name": "@everyone", "permissions": send_only.to_string() } }),
    });
    view.update(cx, |v, cx| v.add_files(vec![bytes_file("cat.png", 4)], cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(lock_sent(&backend.sent_uploads).is_empty());
    assert_eq!(crate::tests::composer_text(&composer, cx), "keep this text");
    view.read_with(cx, |v, _| {
        assert!(v.can_send, "sending text is still allowed");
        let names: Vec<_> = v.attachments.files(GENERAL).iter().map(|file| file.file.name.as_str()).collect();
        assert_eq!(names, ["already-queued.png"]);
        let error = v.error.clone().expect("error shown");
        assert!(error.contains("permission to attach files"), "{error}");
    });
    drop(rt);
}
