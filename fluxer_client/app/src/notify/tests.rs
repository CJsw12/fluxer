use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use fluxer_client_core::Dispatch;
use fluxer_client_core::state::{ClientState, NotificationKind, Snowflake};
use serde_json::{Value, json};

use super::*;
use crate::backend::apply_dispatch;

pub(crate) const GUILD: u64 = 1;
pub(crate) const GENERAL: u64 = 11;
pub(crate) const DM: u64 = 21;
pub(crate) const GROUP: u64 = 22;
pub(crate) const ME: u64 = 100;
pub(crate) const ALICE: u64 = 201;
const NOW: i64 = 1_763_000_100_000;

fn dispatch(t: &str, d: Value) -> Dispatch {
    Dispatch { t: t.to_owned(), s: 1, d }
}

fn ready() -> Dispatch {
    dispatch(
        "READY",
        json!({
            "session_id": "s",
            "user": { "id": ME.to_string(), "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Rust Port", "owner_id": "999" },
                "roles": [
                    { "id": GUILD.to_string(), "name": "@everyone", "permissions": "3072" },
                    { "id": "5", "name": "helpers", "permissions": "0" }
                ],
                "channels": [{ "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 }],
                "members": [
                    { "user": { "id": ME.to_string(), "username": "me" }, "roles": [] },
                    { "user": { "id": ALICE.to_string(), "username": "alice" }, "nick": "Ali", "roles": [] }
                ]
            }],
            "private_channels": [
                { "id": DM.to_string(), "type": 1, "recipients": [{ "id": ALICE.to_string(), "username": "alice", "global_name": "Alice" }] },
                { "id": GROUP.to_string(), "type": 3, "name": "Weekend", "recipients": [{ "id": ALICE.to_string(), "username": "alice" }] }
            ],
            "read_states": []
        }),
    )
}

fn state() -> Mutex<ClientState> {
    let mut state = ClientState::new();
    state.apply(&ready());
    Mutex::new(state)
}

pub(crate) fn message_create(id: u64, channel: u64, author: u64, content: &str) -> Dispatch {
    dispatch(
        "MESSAGE_CREATE",
        json!({
            "id": id.to_string(),
            "channel_id": channel.to_string(),
            "author": { "id": author.to_string(), "username": "alice", "global_name": "Alice" },
            "content": content,
            "timestamp": "2026-11-18T10:00:00+00:00",
            "mentions": [{ "id": ME.to_string(), "username": "me" }]
        }),
    )
}

fn focus(focused: bool, viewing: Option<u64>) -> NotifyFocus {
    let f = NotifyFocus::default();
    f.set_focused(focused);
    f.set_viewing(viewing.map(Snowflake));
    f
}

#[test]
fn guild_message_builds_title_and_resolved_body() {
    let state = state();
    let d = message_create(5000, GENERAL, ALICE, "hey <@100> see <#11> <@&5> <:blob:77> <t:1>");
    let (changed, n) = apply_dispatch(&state, &d, &focus(false, None), NOW);
    let _ = changed;
    let n = n.expect("notifies");
    assert_eq!(n.kind, NotificationKind::Message);
    assert_eq!(n.title, "Ali #general (Rust Port)");
    assert_eq!(n.body, "hey @me see #general @helpers :blob: <t:1>");
    assert_eq!((n.guild_id, n.channel_id, n.message_id), (Some(Snowflake(GUILD)), Snowflake(GENERAL), Snowflake(5000)));
}

#[test]
fn dm_and_group_titles() {
    let state = state();
    let (_, dm) = apply_dispatch(&state, &message_create(5001, DM, ALICE, "yo"), &focus(false, None), NOW);
    let dm = dm.expect("dm notifies");
    assert_eq!((dm.kind, dm.title.as_str(), dm.guild_id), (NotificationKind::DirectMessage, "Alice", None));
    let (_, group) = apply_dispatch(&state, &message_create(5002, GROUP, ALICE, "yo"), &focus(false, None), NOW);
    assert_eq!(group.expect("group notifies").title, "Alice (Weekend)");
}

#[test]
fn focused_viewed_channel_is_sound_only_and_other_events_never_notify() {
    let state = state();
    let d = message_create(5003, GENERAL, ALICE, "hi");
    let (_, n) = apply_dispatch(&state, &d, &focus(true, Some(GENERAL)), NOW);
    assert_eq!(n.map(|n| n.kind), Some(NotificationKind::SameChannelSound));
    // Unfocused while viewing it: a full notification.
    let (_, n) = apply_dispatch(&state, &message_create(5004, GENERAL, ALICE, "hi"), &focus(false, Some(GENERAL)), NOW);
    assert_eq!(n.map(|n| n.kind), Some(NotificationKind::Message));
    let typing = dispatch("TYPING_START", json!({ "channel_id": GENERAL.to_string(), "user_id": ALICE.to_string() }));
    assert_eq!(apply_dispatch(&state, &typing, &focus(false, None), NOW).1, None);
    // Own messages never notify.
    assert_eq!(apply_dispatch(&state, &message_create(5005, GENERAL, ME, "mine"), &focus(false, None), NOW).1, None);
}

#[test]
fn muted_guild_and_dnd_are_silent() {
    let state = state();
    state.lock().expect("lock").apply(&dispatch(
        "USER_GUILD_SETTINGS_UPDATE",
        json!({ "guild_id": GUILD.to_string(), "muted": true }),
    ));
    let (_, n) = apply_dispatch(&state, &message_create(5006, GENERAL, ALICE, "hi"), &focus(false, None), NOW);
    assert_eq!(n, None, "muted guild");
    state.lock().expect("lock").apply(&dispatch("USER_SETTINGS_UPDATE", json!({ "status": "dnd" })));
    let (_, n) = apply_dispatch(&state, &message_create(5007, DM, ALICE, "hi"), &focus(false, None), NOW);
    assert_eq!(n, None, "do not disturb");
}

/// Spoilers never reach the notification centre (`hideSpoilers`).
#[test]
fn spoilers_are_hidden_in_the_toast_body() {
    assert_eq!(hide_spoilers("a ||secret|| b ||x\ny|| c"),"a █████ b █████ c");
    assert_eq!(hide_spoilers("open || only"), "open || only");
    assert_eq!(hide_spoilers("||||"), "||||");
    let state = state();
    let d = message_create(5010, DM, ALICE, "the code is ||1234||");
    let (_, n) = apply_dispatch(&state, &d, &focus(false, None), NOW);
    assert_eq!(n.map(|n| n.body).as_deref(), Some("the code is █████"));
}

#[test]
fn file_only_body() {
    let state = state();
    let mut d = message_create(5008, DM, ALICE, "");
    d.d["attachments"] = json!([{ "id": "1", "filename": "a.png" }, { "id": "2", "filename": "b.png" }]);
    let (_, n) = apply_dispatch(&state, &d, &focus(false, None), NOW);
    assert_eq!(n.expect("notifies").body, "Sent 2 attachments");
}

/// Records what the notifier did.
#[derive(Clone, Default)]
pub(crate) struct Recorder {
    pub toasts: Rc<RefCell<Vec<Toast>>>,
    pub sounds: Rc<RefCell<Vec<Sound>>>,
}

impl Toaster for Recorder {
    fn show(&self, toast: Toast) {
        self.toasts.borrow_mut().push(toast);
    }
}

impl SoundPlayer for Recorder {
    fn play(&self, sound: Sound) {
        self.sounds.borrow_mut().push(sound);
    }
}

impl Recorder {
    pub(crate) fn notifier(&self) -> Notifier {
        Notifier::new(Box::new(self.clone()), Box::new(self.clone()))
    }
}

pub(crate) fn notification(kind: NotificationKind, message_id: u64) -> Notification {
    Notification {
        kind,
        guild_id: Some(Snowflake(GUILD)),
        channel_id: Snowflake(GENERAL),
        message_id: Snowflake(message_id),
        title: "Ali #general (Rust Port)".to_owned(),
        body: "hey @me".to_owned(),
    }
}

#[test]
fn notifier_toasts_plays_and_dedupes() {
    let rec = Recorder::default();
    let mut notifier = rec.notifier();
    assert_eq!(notifier.handle(&notification(NotificationKind::Message, 1)), Outcome::Toast);
    assert_eq!(notifier.handle(&notification(NotificationKind::Message, 1)), Outcome::Nothing, "same message");
    assert_eq!(notifier.handle(&notification(NotificationKind::SameChannelSound, 2)), Outcome::SoundOnly);
    assert_eq!(*rec.sounds.borrow(), [Sound::Message, Sound::SameChannel]);
    let toasts = rec.toasts.borrow();
    assert_eq!(toasts.len(), 1);
    assert_eq!(
        toasts[0],
        Toast {
            guild_id: Some(Snowflake(GUILD)),
            channel_id: Snowflake(GENERAL),
            title: "Ali #general (Rust Port)".to_owned(),
            body: "hey @me".to_owned()
        }
    );
    drop(toasts);
    notifier.enabled = false;
    assert_eq!(notifier.handle(&notification(NotificationKind::DirectMessage, 3)), Outcome::Nothing);
    assert_eq!(rec.sounds.borrow().len(), 2, "disabled: no sound");
}
