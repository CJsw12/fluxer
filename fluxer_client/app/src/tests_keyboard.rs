//! Headless tests for the keyboard shortcuts, the quick switcher and the
//! composer autocomplete, through simulated keystrokes on the real layout.

use fluxer_client_core::Dispatch;
use fluxer_client_core::search::emoji::unicode_by_name;
use fluxer_client_core::state::{LoadMode, Message, Snowflake, bits};
use gpui_kit::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use serde_json::json;

use crate::backend::{Backend, UiEvent, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{composer_text, draw, focus_composer, msg_id, offline_rest, open_client, runtime};
use crate::ui::autocomplete::Trigger;
use crate::ui::composer::ComposerMode;
use crate::ui::message_actions::MessageAction;
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const GUILD: Snowflake = Snowflake(1);
const SECOND: Snowflake = Snowflake(2);
const GENERAL: Snowflake = Snowflake(11);
const ANNOUNCEMENTS: Snowflake = Snowflake(12);
const STAFF: Snowflake = Snowflake(14);
const LOBBY: Snowflake = Snowflake(21);
const DM_ALICE: Snowflake = Snowflake(31);
const DM_BOB: Snowflake = Snowflake(32);
const MODS: u64 = 50;

fn user(id: u64, name: &str) -> serde_json::Value {
    json!({ "id": id.to_string(), "username": name, "global_name": name[..1].to_uppercase() + &name[1..] })
}

fn member(id: u64, name: &str, roles: &[u64]) -> serde_json::Value {
    let roles: Vec<String> = roles.iter().map(u64::to_string).collect();
    json!({ "user": user(id, name), "roles": roles })
}

/// Two guilds and two DMs. Guild 1: general (unread, 2 mentions),
/// announcements (unread; no @everyone for Mods), staff (Mods only, read),
/// a voice channel. Me (100) and Alice (201) are Mods (mentionable, with
/// MENTION_EVERYONE); Bob (202) and Carol (203) are not.
fn ready() -> Dispatch {
    let base = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    let read = |c: Snowflake, ack: u64, mentions: u32| {
        json!({ "id": c.to_string(), "last_message_id": msg_id(ack).to_string(), "mention_count": mentions })
    };
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "session-1",
            "user": { "id": "100", "username": "me" },
            "guilds": [
                {
                    "id": GUILD.to_string(),
                    "properties": { "name": "Rust Port", "owner_id": "999" },
                    "roles": [
                        { "id": GUILD.to_string(), "name": "@everyone", "permissions": base.to_string() },
                        { "id": MODS.to_string(), "name": "Mods", "position": 2, "mentionable": true,
                          "permissions": bits::MENTION_EVERYONE.to_string() },
                        { "id": "51", "name": "Hidden", "position": 1, "permissions": "0" }
                    ],
                    "channels": [
                        { "id": "10", "type": 4, "name": "Text", "position": 0 },
                        { "id": GENERAL.to_string(), "type": 0, "name": "general", "parent_id": "10", "position": 0,
                          "last_message_id": msg_id(3).to_string() },
                        { "id": ANNOUNCEMENTS.to_string(), "type": 0, "name": "announcements", "parent_id": "10",
                          "position": 1, "last_message_id": msg_id(5).to_string(),
                          "permission_overwrites": [{ "id": MODS.to_string(), "type": 0, "allow": "0",
                                                      "deny": bits::MENTION_EVERYONE.to_string() }] },
                        { "id": "13", "type": 2, "name": "Lounge", "parent_id": "10", "position": 2 },
                        { "id": STAFF.to_string(), "type": 0, "name": "staff", "parent_id": "10", "position": 3,
                          "last_message_id": msg_id(6).to_string(),
                          "permission_overwrites": [
                              { "id": GUILD.to_string(), "type": 0, "allow": "0", "deny": bits::VIEW_CHANNEL.to_string() },
                              { "id": MODS.to_string(), "type": 0, "allow": bits::VIEW_CHANNEL.to_string(), "deny": "0" }
                          ] }
                    ],
                    "members": [
                        member(100, "me", &[MODS]),
                        member(201, "alice", &[MODS]),
                        member(202, "bob", &[]),
                        member(203, "carol", &[])
                    ],
                    "emojis": [{ "id": "700", "name": "party_parrot", "animated": false }]
                },
                {
                    "id": SECOND.to_string(),
                    "properties": { "name": "Second", "owner_id": "999" },
                    "roles": [{ "id": SECOND.to_string(), "name": "@everyone", "permissions": base.to_string() }],
                    "channels": [
                        { "id": LOBBY.to_string(), "type": 0, "name": "lobby", "position": 0,
                          "last_message_id": msg_id(7).to_string() }
                    ],
                    "members": [member(100, "me", &[])]
                }
            ],
            "private_channels": [
                { "id": DM_ALICE.to_string(), "type": 1, "recipients": [user(201, "alice")],
                  "last_message_id": msg_id(2).to_string() },
                { "id": DM_BOB.to_string(), "type": 1, "recipients": [user(202, "bob")],
                  "last_message_id": msg_id(4).to_string() }
            ],
            "read_states": [
                read(GENERAL, 1, 2),
                read(ANNOUNCEMENTS, 4, 0),
                read(STAFF, 6, 0),
                read(LOBBY, 7, 0),
                read(DM_ALICE, 2, 0),
                read(DM_BOB, 4, 0)
            ]
        }),
    }
}

/// general: Alice wrote msg 2, Bob msg 3 (newest).
fn general_page() -> Vec<Message> {
    let mut page: Vec<Message> = [(2, 201, "alice"), (3, 202, "bob")]
        .iter()
        .map(|(n, author, name)| {
            serde_json::from_value(json!({
                "id": msg_id(*n).to_string(),
                "channel_id": GENERAL.to_string(),
                "author": user(*author, name),
                "content": format!("hello from {name}"),
                "timestamp": "2026-11-18T10:20:30.000000+00:00"
            }))
            .expect("message fixture")
        })
        .collect();
    page.reverse();
    page
}

fn backend(rt: &tokio::runtime::Runtime) -> (Backend, tokio::sync::mpsc::UnboundedReceiver<UiEvent>) {
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    {
        let mut state = backend.state();
        assert!(!state.apply(&ready()).is_empty());
        state.messages.load(GENERAL, general_page(), LoadMode::Replace);
    }
    (backend, events)
}

/// The client on guild 1 / general with the composer focused.
fn open(cx: &mut TestAppContext, backend: Backend, events: tokio::sync::mpsc::UnboundedReceiver<UiEvent>)
    -> (Entity<ClientView>, &mut VisualTestContext)
{
    let (view, cx) = open_client(cx, backend, events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    focus_composer(&view, cx);
    (view, cx)
}

fn keys(cx: &mut VisualTestContext, keys: &str) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
    draw(cx);
}

fn typed(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    cx.run_until_parked();
    draw(cx);
}

fn at(view: &Entity<ClientView>, cx: &mut VisualTestContext) -> (Place, Option<Snowflake>) {
    view.read_with(cx, |v, _| (v.place, v.selected_channel))
}

fn unread(backend: &Backend, channel: Snowflake) -> (bool, u32) {
    let u = backend.state().channel_unread(channel);
    (u.unread, u.mention_count)
}

fn popup_labels(view: &Entity<ClientView>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    view.read_with(cx, |v, _| v.autocomplete.as_ref().map(|a| a.items.iter().map(|i| i.label.clone()).collect()))
}

/// Alt+Up/Down walk the guild's text channels (voice skipped, wrapping),
/// Alt+Shift only unread ones, Ctrl+Alt+Shift only mentioned ones;
/// Ctrl+Alt+Up/Down walk Home and the guilds and reopen the last channel;
/// in Home Alt+Down walks the DMs with a "no channel" slot. All from the
/// focused composer.
#[gpui_kit::test]
fn navigation_shortcuts(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend, events);
    let g = Place::Guild(GUILD);
    assert_eq!(at(&view, cx), (g, Some(GENERAL)));

    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (g, Some(ANNOUNCEMENTS)));
    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (g, Some(STAFF)), "the voice channel is skipped");
    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (g, Some(GENERAL)), "wraps");
    keys(cx, "alt-up");
    assert_eq!(at(&view, cx), (g, Some(STAFF)));

    keys(cx, "alt-shift-down");
    assert_eq!(at(&view, cx), (g, Some(GENERAL)), "staff is read: first unread");
    keys(cx, "alt-shift-down");
    assert_eq!(at(&view, cx), (g, Some(ANNOUNCEMENTS)));
    keys(cx, "alt-shift-up");
    assert_eq!(at(&view, cx), (g, Some(GENERAL)));
    keys(cx, "alt-down");
    keys(cx, "ctrl-alt-shift-down");
    assert_eq!(at(&view, cx), (g, Some(GENERAL)), "only general has mentions");

    keys(cx, "ctrl-alt-down");
    assert_eq!(at(&view, cx), (Place::Guild(SECOND), Some(LOBBY)));
    keys(cx, "ctrl-alt-down");
    assert_eq!(at(&view, cx), (Place::Home, Some(DM_BOB)), "past the last guild: Home, newest DM");
    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (Place::Home, Some(DM_ALICE)));
    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (Place::Home, None), "the no-channel slot");
    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (Place::Home, Some(DM_BOB)));
    keys(cx, "ctrl-alt-up");
    assert_eq!(at(&view, cx), (Place::Guild(SECOND), Some(LOBBY)));
    keys(cx, "ctrl-alt-up");
    assert_eq!(at(&view, cx), (g, Some(GENERAL)), "the guild's last channel again");
    drop(rt);
}

/// Esc first cancels the reply, then marks the channel read; Shift+Esc
/// marks the whole guild read.
#[gpui_kit::test]
fn escape_chain_and_mark_read(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);
    let target = view.read_with(cx, |v, _| v.rows.borrow()[0].actions.clone());
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Reply, &target, window, cx));
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.composer_mode.mode(), ComposerMode::Replying(_))));
    assert_eq!(unread(&backend, GENERAL), (true, 2));

    keys(cx, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.composer_mode.mode().clone()), ComposerMode::Normal);
    assert_eq!(unread(&backend, GENERAL), (true, 2), "the first Esc only cancels the reply");

    keys(cx, "escape");
    assert_eq!(unread(&backend, GENERAL), (false, 0));
    assert_eq!(backend.state().read_states.get(&GENERAL).and_then(|r| r.ack_message_id), Some(msg_id(3)));

    assert_eq!(unread(&backend, ANNOUNCEMENTS), (true, 0));
    keys(cx, "shift-escape");
    assert_eq!(unread(&backend, ANNOUNCEMENTS), (false, 0));
    assert!(!backend.state().guild_unread(GUILD).unread);
    assert!(backend.state().guild_unread(SECOND) == Default::default());
    drop(rt);
}

fn switcher_titles(view: &Entity<ClientView>, cx: &mut VisualTestContext) -> Vec<String> {
    use fluxer_client_core::search::switcher::SwitcherItem;
    view.read_with(cx, |v, _| match &v.overlay {
        Overlay::QuickSwitcher(sw) => sw
            .items
            .iter()
            .filter_map(|i| if let SwitcherItem::Result(r) = i { Some(r.title.clone()) } else { None })
            .collect(),
        _ => panic!("quick switcher closed"),
    })
}

fn switcher_open(view: &Entity<ClientView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |v, _| v.overlay.is_open())
}

/// Ctrl+K opens the switcher; typing ranks, Enter opens the selected row
/// (channels, existing DMs, a new DM through REST); recent visits come
/// back as "Previous"; Esc and Ctrl+K close it.
#[gpui_kit::test]
fn quick_switcher_ranks_and_navigates(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);

    keys(cx, "ctrl-k");
    assert!(switcher_open(&view, cx));
    typed(cx, "lob");
    assert_eq!(switcher_titles(&view, cx).first().map(String::as_str), Some("lobby"));
    keys(cx, "enter");
    assert!(!switcher_open(&view, cx));
    assert_eq!(at(&view, cx), (Place::Guild(SECOND), Some(LOBBY)));
    let composer_focused = cx.update(|window, cx| {
        let composer = view.read(cx).composer.clone();
        composer.read(cx).focus_handle(cx).is_focused(window)
    });
    assert!(composer_focused, "focus returns to the composer");

    keys(cx, "ctrl-k");
    assert_eq!(switcher_titles(&view, cx).first().map(String::as_str), Some("general"), "previous channel first");
    keys(cx, "down");
    keys(cx, "up");
    keys(cx, "enter");
    assert_eq!(at(&view, cx), (Place::Guild(GUILD), Some(GENERAL)));

    // A member without a DM: the DM is opened through REST.
    keys(cx, "ctrl-k");
    typed(cx, "@@carol");
    assert_eq!(switcher_titles(&view, cx), vec!["Carol".to_owned()]);
    keys(cx, "enter");
    assert!(!switcher_open(&view, cx));
    assert_eq!(*lock_sent(&backend.opened_dms), vec![Snowflake(203)]);

    // An existing DM opens in Home; a row can be picked by index (click).
    keys(cx, "ctrl-k");
    typed(cx, "@bob");
    let ix = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::QuickSwitcher(sw) => sw.selected,
        _ => None,
    });
    view.update_in(cx, |v, window, cx| v.quick_switcher_accept(ix, window, cx));
    draw(cx);
    assert_eq!(at(&view, cx), (Place::Home, Some(DM_BOB)));

    keys(cx, "ctrl-k");
    keys(cx, "escape");
    assert!(!switcher_open(&view, cx), "Esc closes");
    keys(cx, "ctrl-k");
    keys(cx, "ctrl-k");
    assert!(!switcher_open(&view, cx), "Ctrl+K toggles");
    assert_eq!(at(&view, cx), (Place::Home, Some(DM_BOB)), "closing does not navigate");
    drop(rt);
}

/// `@`, `#` and `:` open the popup from the text before the cursor; Tab and
/// Enter pick, Down moves, Esc closes; mentions, channels and custom emoji
/// are shown by name and sent in wire form.
#[gpui_kit::test]
fn autocomplete_inserts_and_converts_on_send(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);
    let composer = view.read_with(cx, |v, _| v.composer.clone());

    // Empty query in a channel with history: the recent speakers.
    typed(cx, "@");
    assert_eq!(popup_labels(&view, cx), Some(vec![
        "Bob".into(), "Alice".into(), "@everyone".into(), "@here".into(), "@Mods".into(), "@Hidden".into()
    ]));
    keys(cx, "escape");
    assert_eq!(popup_labels(&view, cx), None, "Esc closes the popup");
    assert_eq!(unread(&backend, GENERAL), (true, 2), "and does not mark read");

    typed(cx, "al");
    assert_eq!(popup_labels(&view, cx), Some(vec!["Alice".into(), "Carol".into()]), "fuzzy: c-A-r-o-L");
    keys(cx, "tab");
    assert_eq!(composer_text(&composer, cx), "@alice ");
    assert_eq!(popup_labels(&view, cx), None);

    typed(cx, "#gen");
    assert_eq!(view.read_with(cx, |v, _| v.autocomplete.as_ref().map(|a| a.trigger)), Some(Trigger::Channel));
    keys(cx, "enter");
    assert_eq!(composer_text(&composer, cx), "@alice #general ");

    typed(cx, ":smile");
    keys(cx, "enter");
    let smile = unicode_by_name("smile").expect("smile").surrogates.clone();
    assert_eq!(composer_text(&composer, cx), format!("@alice #general {smile} "));

    typed(cx, ":parrot");
    let ix = view.read_with(cx, |v, _| {
        v.autocomplete.as_ref().and_then(|a| a.items.iter().position(|i| i.label == ":party_parrot:"))
    });
    let ix = ix.expect("custom emoji offered");
    view.update_in(cx, |v, window, cx| v.autocomplete_accept(ix, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(composer_text(&composer, cx), format!("@alice #general {smile} :party_parrot: "));

    keys(cx, "enter");
    let (channel, request) = lock_sent(&backend.sent).last().cloned().expect("sent");
    assert_eq!(channel, GENERAL);
    assert_eq!(request.content, Some(format!("<@201> <#11> {smile} <:party_parrot:700>")));
    assert_eq!(composer_text(&composer, cx), "");
    view.read_with(cx, |v, _| {
        assert!(v.frecency.data.entries.contains_key("custom:700"));
        assert!(v.frecency.data.entries.contains_key(smile.as_str()));
    });
    drop(rt);
}

/// Members who cannot view the channel are not offered; without
/// MENTION_EVERYONE only mentionable roles and no @everyone/@here.
#[gpui_kit::test]
fn mention_candidates_follow_permissions(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend, events);

    view.update(cx, |v, cx| v.select_channel(STAFF, cx));
    focus_composer(&view, cx);
    draw(cx);
    typed(cx, "@");
    assert_eq!(
        popup_labels(&view, cx),
        Some(vec!["Alice".into(), "Me".into(), "@everyone".into(), "@here".into(), "@Mods".into(), "@Hidden".into()]),
        "Bob and Carol cannot view staff"
    );
    keys(cx, "down");
    assert_eq!(view.read_with(cx, |v, _| v.autocomplete.as_ref().map(|a| a.selected)), Some(1));
    keys(cx, "escape");

    view.update(cx, |v, cx| v.select_channel(ANNOUNCEMENTS, cx));
    focus_composer(&view, cx);
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    cx.update(|window, cx| composer.update(cx, |s, cx| s.set_value("", window, cx)));
    draw(cx);
    typed(cx, "@");
    assert_eq!(
        popup_labels(&view, cx),
        Some(vec!["Alice".into(), "Bob".into(), "Carol".into(), "Me".into(), "@Mods".into()]),
        "no @everyone/@here and only mentionable roles without MENTION_EVERYONE"
    );
    drop(rt);
}

/// During IME composition (Korean `ㄴ` after `@`) the popup keys belong to
/// the IME: Up / Down do not move the selection, Tab / Enter only commit
/// the composition (no pick, focus stays), Esc only ends it (no read mark).
#[gpui_kit::test]
fn ime_composition_keeps_keys_from_the_popup(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    let compose = |text: &str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| composer.update(cx, |s, cx| s.replace_and_mark_text_in_range(None, text, None, window, cx)));
        draw(cx);
    };
    let marked = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| composer.update(cx, |s, cx| s.marked_text_range(window, cx)))
    };
    let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| composer.read(cx).focus_handle(cx).is_focused(window));
    typed(cx, "@");
    assert!(popup_labels(&view, cx).is_some());
    compose("ㄴ", cx);
    for key in ["down", "up"] {
        keys(cx, key);
        assert_eq!(view.read_with(cx, |v, _| v.autocomplete.as_ref().map(|a| a.selected)), Some(0), "{key}");
        assert_eq!(composer_text(&composer, cx), "@ㄴ", "{key}");
    }
    for key in ["tab", "enter"] {
        compose("ㄴ", cx);
        assert!(marked(cx).is_some());
        keys(cx, key);
        assert_eq!(marked(cx), None, "{key} commits the composition");
        assert!(composer_text(&composer, cx).starts_with("@ㄴ"), "{key}: no pick {:?}", composer_text(&composer, cx));
        assert!(focused(cx), "{key}: focus stays in the composer");
    }
    assert!(lock_sent(&backend.sent).is_empty(), "nothing sent");
    compose("ㄷ", cx);
    keys(cx, "escape");
    assert_eq!(marked(cx), None, "Esc ends the composition");
    assert_eq!(unread(&backend, GENERAL), (true, 2), "and does not mark read");
    drop(rt);
}

/// Replacing the composer text (starting an edit) drops the popup, so its
/// range is never applied to the new text.
#[gpui_kit::test]
fn edit_drops_the_open_popup(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend, events);
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    typed(cx, "@a");
    assert!(popup_labels(&view, cx).is_some());
    view.update_in(cx, |v, window, cx| v.start_edit(GENERAL, msg_id(2), window, cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(popup_labels(&view, cx), None);
    keys(cx, "enter");
    assert_ne!(composer_text(&composer, cx), "@alice llo from alice", "no stale pick into the edit");
    drop(rt);
}

/// The layout's navigation shortcuts do not reach the quick switcher's
/// input (web modal suppression); Ctrl+K still closes it.
#[gpui_kit::test]
fn quick_switcher_input_gets_no_navigation_shortcuts(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);
    let before = at(&view, cx);
    keys(cx, "ctrl-k");
    assert!(switcher_open(&view, cx));
    keys(cx, "alt-down");
    keys(cx, "ctrl-alt-down");
    assert_eq!(at(&view, cx), before);
    keys(cx, "shift-escape");
    assert_ne!(unread(&backend, GENERAL), (false, 0), "Shift+Esc does not mark the guild read");
    keys(cx, "ctrl-k");
    assert!(!switcher_open(&view, cx));
    keys(cx, "alt-down");
    assert_ne!(at(&view, cx), before, "the composer still gets them");
    drop(rt);
}

#[gpui_kit::test]
fn per_channel_drafts_keep_tokens_ignore_edits_clear_on_send_and_survive_rapid_switches(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    let wire = |cx: &VisualTestContext| composer.read_with(cx, |s, _| crate::ui::composer_text::wire_text(s));

    // Compose text + mention token in GENERAL.
    typed(cx, "hello @a");
    keys(cx, "enter");
    assert_eq!(wire(cx), "hello <@201> ");

    // Move to ANNOUNCEMENTS and write a separate draft.
    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (Place::Guild(GUILD), Some(ANNOUNCEMENTS)));
    assert_eq!(composer_text(&composer, cx), "");
    typed(cx, "ann draft");

    // Rapid synchronous switches before cx.defer fires must not drop or clobber drafts.
    view.update(cx, |v, cx| {
        v.select_channel(STAFF, cx);
        v.select_channel(GENERAL, cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert_eq!(at(&view, cx), (Place::Guild(GUILD), Some(GENERAL)));
    assert_eq!(wire(cx), "hello <@201> ");

    // Enter edit mode on a past message, then switch channels: the stashed pre-edit
    // draft (not the edited message text) is saved back for GENERAL.
    view.update_in(cx, |v, window, cx| v.start_edit(GENERAL, msg_id(2), window, cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(composer_text(&composer, cx), "hello from alice");
    keys(cx, "alt-down");
    assert_eq!(composer_text(&composer, cx), "ann draft");
    keys(cx, "alt-up");
    assert_eq!(wire(cx), "hello <@201> ");

    // Sending clears the draft so switching away and returning stays empty.
    keys(cx, "enter");
    assert_eq!(lock_sent(&backend.sent).last().map(|(c, r)| (*c, r.content.as_deref())), Some((GENERAL, Some("hello <@201>"))));
    assert_eq!(composer_text(&composer, cx), "");
    keys(cx, "alt-down");
    assert_eq!(composer_text(&composer, cx), "ann draft");
    keys(cx, "alt-up");
    assert_eq!(composer_text(&composer, cx), "");
    drop(rt);
}

#[gpui_kit::test]
fn history_back_forward_and_toggle_last_guild(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend, events);
    let g1 = Place::Guild(GUILD);
    let g2 = Place::Guild(SECOND);
    assert_eq!(at(&view, cx), (g1, Some(GENERAL)));

    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (g1, Some(ANNOUNCEMENTS)));
    keys(cx, "ctrl-alt-down");
    assert_eq!(at(&view, cx), (g2, Some(LOBBY)));

    // Step backward then forward.
    keys(cx, "alt-left");
    assert_eq!(at(&view, cx), (g1, Some(ANNOUNCEMENTS)));
    keys(cx, "alt-left");
    assert_eq!(at(&view, cx), (g1, Some(GENERAL)));
    keys(cx, "alt-right");
    assert_eq!(at(&view, cx), (g1, Some(ANNOUNCEMENTS)));

    // Visiting a new channel from mid-history truncates the old forward branch.
    keys(cx, "alt-down");
    assert_eq!(at(&view, cx), (g1, Some(STAFF)));
    keys(cx, "alt-right");
    assert_eq!(at(&view, cx), (g1, Some(STAFF)), "forward history truncated");

    // Ctrl+Alt+Right toggles Home <-> last guild.
    keys(cx, "ctrl-alt-right");
    assert_eq!(at(&view, cx), (Place::Home, Some(DM_BOB)));
    keys(cx, "ctrl-alt-right");
    assert_eq!(at(&view, cx), (g1, Some(STAFF)));
    drop(rt);
}

#[gpui_kit::test]
fn channel_mute_toggles_guild_and_dm_and_preserves_siblings(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    {
        let mut state = backend.state();
        state.apply(&Dispatch {
            t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
            s: 2,
            d: json!({
                "guild_id": GUILD.to_string(),
                "channel_overrides": {
                    ANNOUNCEMENTS.to_string(): { "channel_id": ANNOUNCEMENTS.to_string(), "muted": true }
                }
            }),
        });
    }
    let (view, cx) = open(cx, backend.clone(), events);
    assert!(!view.read_with(cx, |v, _| v.selected_channel_muted()));

    view.update(cx, |v, cx| v.toggle_channel_mute(cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.selected_channel_muted()));
    {
        let state = backend.state();
        assert!(state.is_channel_muted(Some(GUILD), GENERAL, 0));
        assert!(state.is_channel_muted(Some(GUILD), ANNOUNCEMENTS, 0), "sibling override kept");
    }
    assert!(cx.debug_bounds("toggle-channel-mute").is_some());

    // Toggle off on guild channel.
    view.update(cx, |v, cx| v.toggle_channel_mute(cx));
    cx.run_until_parked();
    assert!(!backend.state().is_channel_muted(Some(GUILD), GENERAL, 0));

    // Toggle mute on a DM channel (@me settings path).
    keys(cx, "ctrl-alt-right");
    assert_eq!(at(&view, cx), (Place::Home, Some(DM_BOB)));
    view.update(cx, |v, cx| v.toggle_channel_mute(cx));
    cx.run_until_parked();
    assert!(backend.state().is_channel_muted(None, DM_BOB, 0));

    let sent = lock_sent(&backend.sent_guild_settings);
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0].0, Some(GUILD));
    let first_overrides = sent[0]
        .1
        .channel_overrides
        .as_ref()
        .and_then(|inner| inner.as_ref())
        .expect("patch carries channel_overrides");
    assert_eq!(first_overrides.get(&GENERAL.to_string()).and_then(|o| o.muted), Some(true));
    assert_eq!(first_overrides.get(&ANNOUNCEMENTS.to_string()).and_then(|o| o.muted), Some(true));
    assert_eq!(sent[2].0, None);
    drop(rt);
}
