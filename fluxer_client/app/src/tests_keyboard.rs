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

#[gpui_kit::test]
fn chat_keybinds_popouts_pickers_clipboard_and_focus(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);

    // Ctrl+U toggles member list.
    let initial_members = view.read_with(cx, |v, _| v.shows_member_list());
    keys(cx, "ctrl-u");
    assert_ne!(view.read_with(cx, |v, _| v.shows_member_list()), initial_members);
    keys(cx, "ctrl-u");
    assert_eq!(view.read_with(cx, |v, _| v.shows_member_list()), initial_members);

    // Ctrl+P toggles pinned messages popout.
    keys(cx, "ctrl-p");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, crate::ui::channel_popouts::ChannelPopoutKind::PinnedMessages { .. })
    )));
    keys(cx, "ctrl-p");
    assert!(!view.read_with(cx, |v, _| v.overlay.is_open()));

    // Ctrl+G opens GIF picker tab, Ctrl+S switches to Stickers, Ctrl+S again closes.
    keys(cx, "ctrl-g");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::EmojiPicker(p) if p.active_tab == crate::ui::emoji_picker::ExpressionTab::Media
    )));
    keys(cx, "ctrl-s");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::EmojiPicker(p) if p.active_tab == crate::ui::emoji_picker::ExpressionTab::Stickers
    )));
    keys(cx, "escape");
    assert!(!view.read_with(cx, |v, _| v.overlay.is_open()));

    // Ctrl+I opens Inbox only when composer is empty (allow_when_empty).
    keys(cx, "a");
    keys(cx, "ctrl-i");
    assert!(!view.read_with(cx, |v, _| v.overlay.is_open()), "ctrl-i ignored while typing");
    keys(cx, "backspace");
    keys(cx, "ctrl-i");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, crate::ui::channel_popouts::ChannelPopoutKind::Inbox { .. })
    )));
    keys(cx, "escape");

    // Ctrl+Shift+T opens Create Group DM modal.
    keys(cx, "ctrl-shift-t");
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::CreateGroupDm(_))));
    keys(cx, "escape");

    // Ctrl+Shift+L copies channel link to clipboard.
    keys(cx, "ctrl-shift-l");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    let expected_url = format!("{}/channels/{}/{}", backend.web_origin().trim_end_matches('/'), GUILD.0, GENERAL.0);
    assert_eq!(copied.as_deref(), Some(expected_url.as_str()));

    // Tab focuses composer when root has focus.
    view.update_in(cx, |v, window, cx| v.focus_root(window, cx));
    cx.run_until_parked();
    assert!(!view.update_in(cx, |v, window, cx| v.composer.read(cx).focus_handle(cx).is_focused(window)));
    keys(cx, "tab");
    assert!(view.update_in(cx, |v, window, cx| v.composer.read(cx).focus_handle(cx).is_focused(window)));

    drop(rt);
}

#[gpui_kit::test]
fn chat_keybinds_scroll_unread_and_inbox_read(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);

    // Shift+PageUp jumps to oldest unread in GENERAL (msg 1 read -> msg 2 unread).
    keys(cx, "shift-pageup");
    assert_eq!(view.read_with(cx, |v, _| v.highlighted_message), Some((GENERAL, msg_id(2))));

    // PageUp / PageDown work when composer is empty (do not panic, keep composer empty).
    keys(cx, "pageup");
    keys(cx, "pagedown");
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    assert_eq!(composer_text(&composer, cx), "");

    // Ctrl+Shift+E marks top unread inbox channel (GENERAL) as read.
    assert!(backend.state().channel_unread(GENERAL).unread);
    assert!(backend.state().channel_unread(ANNOUNCEMENTS).unread);
    keys(cx, "ctrl-shift-e");
    assert!(!backend.state().channel_unread(GENERAL).unread);
    assert!(backend.state().channel_unread(ANNOUNCEMENTS).unread);

    // Custom/dispatch chat_mark_all_inbox_read clears remaining unread channels.
    let handled = view.update_in(cx, |v, window, cx| v.dispatch_keybind_action("chat_mark_all_inbox_read", window, cx));
    assert!(handled);
    cx.run_until_parked();
    assert!(!backend.state().channel_unread(ANNOUNCEMENTS).unread);

    // All 14 chat_* actions are registered in ASSIGNABLE_ACTIONS and active by default.
    for action in [
        "chat_copy_channel_link",
        "chat_focus_textarea",
        "chat_jump_oldest_unread",
        "chat_mark_all_inbox_read",
        "chat_mark_inbox_read",
        "chat_new_dm",
        "chat_scroll_down",
        "chat_scroll_up",
        "chat_toggle_gif",
        "chat_toggle_inbox",
        "chat_toggle_member_list",
        "chat_toggle_pins",
        "chat_toggle_sticker",
        "chat_upload",
    ] {
        assert!(crate::shortcuts::action_label(action).is_some(), "missing label for {action}");
        assert!(view.read_with(cx, |v, _| v.should_run_builtin_shortcut(action)), "{action} not active");
    }

    drop(rt);
}

#[gpui_kit::test]
fn nav_and_message_keybinds_and_composer_safety(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);

    // 1. All 25 new actions (12 nav + 13 messages, excluding T3 message_delete) are registered and active.
    let expected_actions = [
        "nav_current_call",
        "nav_guild_slot_1",
        "nav_guild_slot_2",
        "nav_guild_slot_3",
        "nav_guild_slot_4",
        "nav_guild_slot_5",
        "nav_guild_slot_6",
        "nav_guild_slot_7",
        "nav_guild_slot_8",
        "nav_guild_slot_9",
        "nav_guild_tab_next",
        "nav_guild_tab_prev",
        "message_reply",
        "message_reply_prev",
        "message_reply_next",
        "message_edit",
        "message_edit_prev",
        "message_edit_next",
        "message_react",
        "message_forward",
        "message_pin",
        "message_mark_unread",
        "message_copy_text",
        "message_speak",
        "message_focus_textarea",
    ];
    for action in expected_actions {
        assert!(crate::shortcuts::action_label(action).is_some(), "missing label for {action}");
        assert!(view.read_with(cx, |v, _| v.should_run_builtin_shortcut(action)), "{action} not active");
    }

    // 2. Navigation: Ctrl+1..3 (slots) and Ctrl+Tab / Ctrl+Shift+Tab.
    keys(cx, "ctrl-1");
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Home);
    keys(cx, "ctrl-2");
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Guild(GUILD));
    keys(cx, "ctrl-3");
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Guild(SECOND));
    keys(cx, "ctrl-shift-tab");
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Guild(GUILD));
    keys(cx, "ctrl-tab");
    assert_eq!(view.read_with(cx, |v, _| v.place), Place::Guild(SECOND));

    // nav_current_call: jumps to active voice channel's guild/call view.
    view.update(cx, |v, _| {
        v.voice.channel_id = Some(Snowflake(13));
        v.voice_stage_open = false;
    });
    keys(cx, "ctrl-alt-shift-v");
    assert_eq!(view.read_with(cx, |v, _| (v.place, v.voice_stage_open)), (Place::Guild(GUILD), true));
    view.update(cx, |v, _| {
        v.voice.channel_id = None;
        v.voice_stage_open = false;
    });

    // Return to GENERAL with two known messages (Alice + Me) for message cursor tests.
    view.update(cx, |v, cx| v.select_channel(GENERAL, cx));
    draw(cx);
    for (seq, id, author, content) in [
        (10u64, msg_id(10), user(201, "alice"), "hello from alice"),
        (11u64, msg_id(11), user(100, "me"), "my own message"),
    ] {
        let ev = Dispatch {
            t: "MESSAGE_CREATE".to_owned(),
            s: seq,
            d: json!({
                "id": id.to_string(),
                "channel_id": GENERAL.to_string(),
                "guild_id": GUILD.to_string(),
                "author": author,
                "content": content,
                "timestamp": "2026-01-01T00:00:00Z",
                "type": 0,
                "flags": 0
            }),
        };
        backend.state().apply(&ev);
    }
    view.update(cx, |v, cx| v.refresh(cx));
    draw(cx);

    // 3. Composer non-interference: single-char keys type into composer, not message actions.
    focus_composer(&view, cx);
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    cx.simulate_input("refps+");
    assert_eq!(composer_text(&composer, cx), "refps+");
    assert_eq!(view.read_with(cx, |v, _| v.keyboard_message), None);
    assert!(matches!(view.read_with(cx, |v, _| v.composer_mode.mode().clone()), ComposerMode::Normal));
    composer.update_in(cx, |s, window, cx| s.set_value("", window, cx));

    // 4. Message cursor navigation: ctrl-up / ctrl-down (reply) and ctrl-shift-up / ctrl-shift-down (edit).
    keys(cx, "ctrl-up");
    assert_eq!(view.read_with(cx, |v, _| v.keyboard_message), Some(msg_id(11)));
    assert!(matches!(view.read_with(cx, |v, _| v.composer_mode.mode().clone()), ComposerMode::Replying(t) if t.message_id == msg_id(11)));
    keys(cx, "ctrl-up");
    assert_eq!(view.read_with(cx, |v, _| v.keyboard_message), Some(msg_id(10)));
    keys(cx, "ctrl-down");
    assert_eq!(view.read_with(cx, |v, _| v.keyboard_message), Some(msg_id(11)));

    keys(cx, "ctrl-shift-up");
    assert_eq!(view.read_with(cx, |v, _| v.keyboard_message), Some(msg_id(11)));
    assert!(matches!(view.read_with(cx, |v, _| v.composer_mode.mode().clone()), ComposerMode::Editing(t) if t.message_id == msg_id(11)));
    keys(cx, "escape"); // cancels edit in composer
    composer.update_in(cx, |s, window, cx| s.set_value("", window, cx));

    // 5. Layout-only message actions when composer is NOT focused.
    view.update_in(cx, |v, window, cx| v.focus_root(window, cx));
    keys(cx, "+");
    assert_eq!(view.read_with(cx, |v, _| v.reaction_picker), Some(msg_id(11)));
    keys(cx, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.keyboard_message), None);
    assert!(view.update_in(cx, |v, window, cx| v.composer.read(cx).focus_handle(cx).is_focused(window)));

    // Copy text when root focused copies cursor message without interfering with composer.
    view.update_in(cx, |v, window, cx| v.focus_root(window, cx));
    keys(cx, "ctrl-c");
    assert_eq!(view.read_with(cx, |v, _| v.keyboard_message), Some(msg_id(11)));

    drop(rt);
}

#[gpui_kit::test]
fn m9_1_keybind_parity_batch(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend(&rt);
    let (view, cx) = open(cx, backend.clone(), events);

    // 1. All 33 non-T3 keybind actions have labels and are active by default.
    let all_33 = [
        "message_bookmark",
        "message_copy_id",
        "message_copy_link",
        "message_toggle_embeds",
        "misc_help",
        "system_open_theme_studio_popout",
        "system_toggle_custom_css",
        "system_toggle_downloads",
        "system_toggle_shortcuts_overlay",
        "voice_disconnect",
        "voice_priority_vad",
        "voice_push_to_mute",
        "voice_push_to_talk",
        "voice_push_to_talk_priority",
        "voice_start_dm_call",
        "voice_switch_channel",
        "voice_toggle_camera",
        "voice_toggle_compact_call_view",
        "voice_toggle_vad",
        "chat_mark_all_inbox_read",
        "chat_send_voice_message",
        "chat_toggle_saved_media",
        "dnd_cancel",
        "dnd_drop",
        "dnd_move_down",
        "dnd_move_up",
        "dnd_start",
        "misc_open_context_menu",
        "voice_answer_call",
        "voice_decline_call",
        "voice_toggle_deafen",
        "voice_toggle_mute",
        "voice_toggle_soundboard",
    ];
    for action in all_33 {
        assert!(crate::shortcuts::action_label(action).is_some(), "missing label for {action}");
        assert!(view.read_with(cx, |v, _| v.should_run_builtin_shortcut(action)), "{action} not active");
    }

    // 2. dnd_drop cheatsheet combo matches official ' '.
    let dnd_rows = crate::shortcuts::default_shortcut_rows(crate::shortcuts::KeybindSection::DragAndDrop);
    let drop_spec = dnd_rows
        .iter()
        .flat_map(|r| r.specs())
        .find(|s| s.action == "dnd_drop")
        .expect("dnd_drop spec");
    assert_eq!(drop_spec.combo, " ");

    // 3. Voice shortcuts: mute, deafen, compact call view, VAD toggle.
    let initial_mute = view.read_with(cx, |v, _| v.voice.self_mute);
    keys(cx, "ctrl-shift-m");
    assert_ne!(view.read_with(cx, |v, _| v.voice.self_mute), initial_mute);

    let initial_deaf = view.read_with(cx, |v, _| v.voice.self_deaf);
    keys(cx, "ctrl-shift-d");
    assert_ne!(view.read_with(cx, |v, _| v.voice.self_deaf), initial_deaf);

    view.update(cx, |v, _| {
        v.voice.channel_id = Some(Snowflake(13));
        v.voice_stage_open = false;
    });
    keys(cx, "ctrl-shift-v");
    assert!(view.read_with(cx, |v, _| v.voice_stage_open));
    keys(cx, "ctrl-shift-v");
    assert!(!view.read_with(cx, |v, _| v.voice_stage_open));
    view.update(cx, |v, _| {
        v.voice.channel_id = None;
        v.voice_stage_open = false;
    });

    let initial_ptt = view.read_with(cx, |v, _| v.prefs.data.voice_push_to_talk);
    assert!(view.update_in(cx, |v, window, cx| v.dispatch_keybind_action("voice_toggle_vad", window, cx)));
    assert_ne!(view.read_with(cx, |v, _| v.prefs.data.voice_push_to_talk), initial_ptt);

    // 4. Shortcuts overlay (ctrl-/) opens Settings -> Keybinds and toggles closed.
    keys(cx, "ctrl-/");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::Settings(m) if m.active_tab == crate::ui::settings::SettingsTab::Keybinds
    )));
    keys(cx, "ctrl-/");
    assert!(!view.read_with(cx, |v, _| v.overlay.is_open()));

    // 5. Message cursor actions: bookmark, copy_id, copy_link, and shift-f10 context menu.
    view.update(cx, |v, _| v.keyboard_message = Some(Snowflake(2)));
    let _ = view.update_in(cx, |v, window, cx| {
        assert!(v.dispatch_keybind_action("message_copy_id", window, cx));
        assert!(v.dispatch_keybind_action("message_copy_link", window, cx));
        assert!(v.dispatch_keybind_action("message_bookmark", window, cx));
    });
    view.update_in(cx, |v, window, cx| v.focus_root(window, cx));
    keys(cx, "shift-f10");
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::ChannelPopout(p) if matches!(p.kind, crate::ui::channel_popouts::ChannelPopoutKind::MessageMenu { .. })
    )));
    keys(cx, "escape");

    drop(rt);
}
