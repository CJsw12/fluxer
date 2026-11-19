//! Headless GPUI tests for Milestone 8: guild channel creation, editing,
//! two-step deletion, Escape behavior, category-scoped permissions, row
//! edit propagation isolation, and modal shortcut isolation.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{
    CreateGuildChannelRequest, GuildChannelPatch, VOICE_CHANNEL_BITRATE_DEFAULT,
    VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT,
};
use fluxer_client_core::state::{ChannelKind, Snowflake, bits};
use gpui_kit::component::input::InputContent;
use gpui_kit::{Focusable as _, Modifiers, TestAppContext};
use serde_json::json;

use crate::backend::{Backend, GuildChannelCall, UiEvent, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{draw, focus_composer, offline_rest, open_client, runtime};
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const ME: Snowflake = Snowflake(100);
const OWNER: Snowflake = Snowflake(999);
const GUILD: Snowflake = Snowflake(1);
const CAT_MAIN: Snowflake = Snowflake(10);
const CAT_MANAGED: Snowflake = Snowflake(15);
const CAT_OTHER: Snowflake = Snowflake(16);
const GENERAL: Snowflake = Snowflake(11);
const ANNOUNCE: Snowflake = Snowflake(12);
const VOICE: Snowflake = Snowflake(13);

fn guild_ready(owner_id: Snowflake, role_perms: u64, cat_managed_allow: u64) -> Dispatch {
    let base_view = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": ME.to_string(), "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Dev Guild", "owner_id": owner_id.to_string() },
                "roles": [{
                    "id": GUILD.to_string(),
                    "name": "@everyone",
                    "permissions": (base_view | role_perms).to_string()
                }],
                "channels": [
                    { "id": CAT_MAIN.to_string(), "type": 4, "name": "Main", "position": 0 },
                    {
                        "id": GENERAL.to_string(),
                        "type": 0,
                        "name": "general",
                        "topic": "Welcome",
                        "parent_id": CAT_MAIN.to_string(),
                        "position": 0
                    },
                    {
                        "id": ANNOUNCE.to_string(),
                        "type": 0,
                        "name": "announcements",
                        "parent_id": CAT_MAIN.to_string(),
                        "position": 1
                    },
                    {
                        "id": VOICE.to_string(),
                        "type": 2,
                        "name": "Lounge",
                        "parent_id": CAT_MAIN.to_string(),
                        "position": 2
                    },
                    {
                        "id": CAT_MANAGED.to_string(),
                        "type": 4,
                        "name": "Scoped",
                        "position": 1,
                        "permission_overwrites": if cat_managed_allow == 0 {
                            vec![]
                        } else {
                            vec![json!({
                                "id": GUILD.to_string(),
                                "type": 0,
                                "allow": cat_managed_allow.to_string(),
                                "deny": "0"
                            })]
                        }
                    },
                    { "id": CAT_OTHER.to_string(), "type": 4, "name": "Other Empty", "position": 2 }
                ],
                "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }]
            }],
            "private_channels": []
        }),
    }
}

fn open_guild(
    cx: &mut TestAppContext,
    owner_id: Snowflake,
    role_perms: u64,
    cat_managed_allow: u64,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut gpui_kit::VisualTestContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&guild_ready(owner_id, role_perms, cat_managed_allow)).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

#[gpui_kit::test]
fn channel_mgmt_permissions_gate_sidebar_header_and_category_scope(cx: &mut TestAppContext) {
    // 1. Plain member cannot see create/edit controls and cannot open modals.
    {
        let mut sub_cx = cx.clone();
        let (_rt, _backend, view, vcx) = open_guild(&mut sub_cx, OWNER, 0, 0);
        assert!(vcx.debug_bounds("create-channel-btn").is_none());
        assert!(vcx.debug_bounds("edit-selected-channel-btn").is_none());
        assert!(vcx.debug_bounds("edit-channel-11").is_none());
        assert!(vcx.debug_bounds("edit-channel-15").is_none());
        vcx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.open_create_channel(None, window, cx);
                assert!(!v.overlay.is_open());
                v.open_edit_channel(GENERAL, window, cx);
                assert!(!v.overlay.is_open());
            });
        });
    }

    // 2. Category-scoped member sees empty managed category, cannot create at root or category type.
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, 0, bits::MANAGE_CHANNELS);
    assert!(cx.debug_bounds("create-channel-btn").is_none());
    assert!(cx.debug_bounds("edit-selected-channel-btn").is_none());
    assert!(cx.debug_bounds("create-in-category-15").is_some(), "empty managed category must be visible");
    assert!(cx.debug_bounds("edit-channel-15").is_some());
    assert!(cx.debug_bounds("create-in-category-16").is_none(), "unmanaged empty category stays hidden");

    let add_in_cat = cx.debug_bounds("create-in-category-15").unwrap().center();
    cx.simulate_click(add_in_cat, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    assert!(view.read_with(cx, |v, _| {
        matches!(
            &v.overlay,
            Overlay::CreateChannel(m) if !m.can_manage_root && m.parent_id == Some(CAT_MANAGED)
        )
    }));
    assert!(cx.debug_bounds("create-channel-type-category").is_none());
    assert!(cx.debug_bounds("create-channel-parent-none").is_none());

    // Trying to select category type or root parent fails silently; typing & submitting sends under CAT_MANAGED.
    view.update(cx, |v, cx| v.set_create_channel_kind(ChannelKind::GuildCategory, cx));
    cx.simulate_input("scoped-text");
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("create-channel-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        *lock_sent(&backend.sent_guild_channels),
        vec![GuildChannelCall::Create {
            guild_id: GUILD,
            request: CreateGuildChannelRequest {
                name: "scoped-text".into(),
                kind: ChannelKind::GuildText,
                parent_id: Some(CAT_MANAGED.0.to_string()),
                topic: None,
                nsfw: Some(false),
                rate_limit_per_user: None,
                bitrate: None,
                user_limit: None,
                voice_connection_limit: None,
            },
        }]
    );
}

#[gpui_kit::test]
fn create_channel_validation_payloads_dedup_stale_seq_and_selection(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, ME, 0, 0);
    assert!(cx.debug_bounds("create-channel-btn").is_some());
    assert!(cx.debug_bounds("edit-selected-channel-btn").is_some());

    let create_btn = cx.debug_bounds("create-channel-btn").unwrap().center();
    cx.simulate_click(create_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("create-channel-modal").is_some());

    // Blank name rejected locally
    let submit = cx.debug_bounds("create-channel-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert!(lock_sent(&backend.sent_guild_channels).is_empty());
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::CreateChannel(m) if m.error.is_some())));

    // Over-long UTF-16 name rejected locally (51 4-byte emoji = 102 UTF-16 units)
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::CreateChannel(m) = &v.overlay {
                m.name_input.update(cx, |s, cx| s.set_value(&"😀".repeat(51), window, cx));
            }
        });
    });
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("create-channel-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert!(lock_sent(&backend.sent_guild_channels).is_empty());
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::CreateChannel(m) if m.error.is_some())));

    // Clear and test Voice defaults + double-submit guard + stale seq
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::CreateChannel(m) = &v.overlay {
                m.name_input.update(cx, |s, cx| s.set_value("squad-voice", window, cx));
            }
        });
    });
    draw(cx);
    let voice_btn = cx.debug_bounds("create-channel-type-voice").unwrap().center();
    cx.simulate_click(voice_btn, Modifiers::default());
    let parent_btn = cx.debug_bounds("create-channel-parent-10").unwrap().center();
    cx.simulate_click(parent_btn, Modifiers::default());
    let nsfw_btn = cx.debug_bounds("create-channel-nsfw").unwrap().center();
    cx.simulate_click(nsfw_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    let submit = cx.debug_bounds("create-channel-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(lock_sent(&backend.sent_guild_channels).len(), 1);
    assert_eq!(
        lock_sent(&backend.sent_guild_channels)[0],
        GuildChannelCall::Create {
            guild_id: GUILD,
            request: CreateGuildChannelRequest {
                name: "squad-voice".into(),
                kind: ChannelKind::GuildVoice,
                parent_id: Some("10".into()),
                topic: None,
                nsfw: Some(true),
                rate_limit_per_user: None,
                bitrate: Some(VOICE_CHANNEL_BITRATE_DEFAULT),
                user_limit: Some(0),
                voice_connection_limit: Some(VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT),
            },
        }
    );
    let first_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::CreateChannel(m) => m.seq,
        _ => panic!("expected CreateChannel"),
    });

    // Escape closes and reopen gets a new seq -> stale first_seq event ignored
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    let create_btn = cx.debug_bounds("create-channel-btn").unwrap().center();
    cx.simulate_click(create_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::ChannelCreated { seq: first_seq, guild_id: GUILD, result: Ok(Snowflake(99)) }], cx);
    });
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::CreateChannel(_))));

    // Switching to Category clears parent_id & nsfw, and completing keeps selected GENERAL
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.set_create_channel_parent(Some(CAT_MAIN), cx);
            v.set_create_channel_nsfw(true, cx);
            v.set_create_channel_kind(ChannelKind::GuildCategory, cx);
            if let Overlay::CreateChannel(m) = &v.overlay {
                m.name_input.update(cx, |s, cx| s.set_value("New Section", window, cx));
            }
        });
    });
    draw(cx);
    let submit = cx.debug_bounds("create-channel-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    let cat_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::CreateChannel(m) => m.seq,
        _ => panic!("expected CreateChannel"),
    });
    assert_eq!(
        lock_sent(&backend.sent_guild_channels)[1],
        GuildChannelCall::Create {
            guild_id: GUILD,
            request: CreateGuildChannelRequest {
                name: "New Section".into(),
                kind: ChannelKind::GuildCategory,
                parent_id: None,
                topic: None,
                nsfw: None,
                rate_limit_per_user: None,
                bitrate: None,
                user_limit: None,
                voice_connection_limit: None,
            },
        }
    );
    let new_cat = Snowflake(40);
    backend.state().apply(&Dispatch {
        t: "CHANNEL_CREATE".to_owned(),
        s: 2,
        d: json!({ "id": new_cat.to_string(), "guild_id": GUILD.to_string(), "type": 4, "name": "New Section", "position": 9 }),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::ChannelCreated { seq: cat_seq, guild_id: GUILD, result: Ok(new_cat) }], cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None) && v.selected_channel == Some(GENERAL)));

    // Text channel creation auto-selects the created channel and restores composer focus
    let create_btn = cx.debug_bounds("create-channel-btn").unwrap().center();
    cx.simulate_click(create_btn, Modifiers::default());
    cx.run_until_parked();
    cx.simulate_input("lounge-text");
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("create-channel-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    let text_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::CreateChannel(m) => m.seq,
        _ => panic!("expected CreateChannel"),
    });
    let new_text = Snowflake(41);
    backend.state().apply(&Dispatch {
        t: "CHANNEL_CREATE".to_owned(),
        s: 3,
        d: json!({ "id": new_text.to_string(), "guild_id": GUILD.to_string(), "type": 0, "name": "lounge-text", "position": 10 }),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::ChannelCreated { seq: text_seq, guild_id: GUILD, result: Ok(new_text) }], cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None) && v.selected_channel == Some(new_text)));
    let composer_focused = cx.update(|window, cx| {
        let handle = view.read(cx).composer.read(cx).focus_handle(cx);
        handle.is_focused(window)
    });
    assert!(composer_focused);
}

#[gpui_kit::test]
fn edit_channel_dirty_patch_topic_clear_and_row_propagation(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, bits::MANAGE_CHANNELS, 0);

    // Clicking edit button on non-selected row (ANNOUNCEMENTS=12) must NOT switch selected_channel
    let edit_other = cx.debug_bounds("edit-channel-12").expect("row edit button").center();
    cx.simulate_click(edit_other, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        v.selected_channel == Some(GENERAL)
            && matches!(&v.overlay, Overlay::EditChannel(m) if m.channel_id == ANNOUNCE)
    }));

    // Clicking save when nothing changed sends nothing
    let save = cx.debug_bounds("edit-channel-save").unwrap().center();
    cx.simulate_click(save, Modifiers::default());
    cx.run_until_parked();
    assert!(lock_sent(&backend.sent_guild_channels).is_empty());

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Open edit on selected GENERAL via header button; modify all fields + clear topic -> Some(None)
    let edit_sel = cx.debug_bounds("edit-selected-channel-btn").unwrap().center();
    cx.simulate_click(edit_sel, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::EditChannel(m) = &v.overlay {
                m.name_input.update(cx, |s, cx| s.set_value("  news-chat  ", window, cx));
                if let Some(topic) = &m.topic_input {
                    topic.update(cx, |s, cx| s.set_value("   ", window, cx));
                }
            }
            v.toggle_edit_channel_announcement(true, cx);
            v.set_edit_channel_slowmode(300, cx);
            v.set_edit_channel_nsfw(true, cx);
            v.set_edit_channel_parent(None, cx);
        });
    });
    draw(cx);

    let save = cx.debug_bounds("edit-channel-save").unwrap().center();
    cx.simulate_click(save, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        *lock_sent(&backend.sent_guild_channels),
        vec![GuildChannelCall::Update {
            channel_id: GENERAL,
            patch: GuildChannelPatch {
                name: Some("news-chat".into()),
                kind: Some(ChannelKind::GuildAnnouncement),
                topic: Some(None),
                parent_id: Some(None),
                nsfw: None,
                nsfw_override: Some(Some(true)),
                rate_limit_per_user: Some(300),
            },
        }]
    );
    let edit_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::EditChannel(m) => m.seq,
        _ => panic!("expected EditChannel"),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::ChannelUpdated { seq: edit_seq, channel_id: GENERAL, error: None }], cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Turning OFF NSFW when currently true sends nsfw_override: Some(Some(false)) instead of legacy inherit.
    backend.state().apply(&Dispatch {
        t: "CHANNEL_UPDATE".to_owned(),
        s: 2,
        d: json!({ "id": ANNOUNCE.to_string(), "nsfw": true }),
    });
    let edit_other = cx.debug_bounds("edit-channel-12").expect("row edit button").center();
    cx.simulate_click(edit_other, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    view.update(cx, |v, cx| v.set_edit_channel_nsfw(false, cx));
    draw(cx);
    let save = cx.debug_bounds("edit-channel-save").unwrap().center();
    cx.simulate_click(save, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        lock_sent(&backend.sent_guild_channels)[1],
        GuildChannelCall::Update {
            channel_id: ANNOUNCE,
            patch: GuildChannelPatch {
                nsfw_override: Some(Some(false)),
                ..Default::default()
            },
        }
    );
}

#[gpui_kit::test]
fn delete_channel_two_step_escape_failure_fallback_and_category_detach(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, ME, 0, 0);

    // Stash a draft in GENERAL to verify cleanup on delete
    view.update(cx, |v, _| {
        v.drafts.insert(GENERAL, InputContent::new("unsent draft"));
    });

    let edit_sel = cx.debug_bounds("edit-selected-channel-btn").unwrap().center();
    cx.simulate_click(edit_sel, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    // First delete click enters confirmation only
    let del_btn = cx.debug_bounds("edit-channel-delete").unwrap().center();
    cx.simulate_click(del_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(lock_sent(&backend.sent_guild_channels).is_empty());
    assert!(cx.debug_bounds("edit-channel-delete-confirm").is_some());
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::EditChannel(m) if m.confirming_delete)));

    // First Escape cancels delete confirmation only (modal stays open)
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::EditChannel(m) if !m.confirming_delete)));

    // Second Escape closes modal and restores composer focus
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Reopen, confirm delete -> handle failure, then confirm delete -> handle success + fallback to ANNOUNCE
    let edit_sel = cx.debug_bounds("edit-selected-channel-btn").unwrap().center();
    cx.simulate_click(edit_sel, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    let del_btn = cx.debug_bounds("edit-channel-delete").unwrap().center();
    cx.simulate_click(del_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let confirm = cx.debug_bounds("edit-channel-delete-confirm").unwrap().center();
    cx.simulate_click(confirm, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(*lock_sent(&backend.sent_guild_channels), vec![GuildChannelCall::Delete { channel_id: GENERAL }]);
    let fail_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::EditChannel(m) => m.seq,
        _ => panic!("expected EditChannel"),
    });
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::ChannelDeleted {
                seq: fail_seq,
                channel_id: GENERAL,
                error: Some("Missing Permissions".to_owned()),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::EditChannel(m) if !m.submitting && !m.confirming_delete && m.error.as_deref() == Some("Missing Permissions")
    )));

    let del_btn = cx.debug_bounds("edit-channel-delete").unwrap().center();
    cx.simulate_click(del_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let confirm = cx.debug_bounds("edit-channel-delete-confirm").unwrap().center();
    cx.simulate_click(confirm, Modifiers::default());
    cx.run_until_parked();
    let ok_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::EditChannel(m) => m.seq,
        _ => panic!("expected EditChannel"),
    });
    backend.state().apply(&Dispatch {
        t: "CHANNEL_DELETE".to_owned(),
        s: 2,
        d: json!({ "id": GENERAL.to_string() }),
    });
    // Cross-batch race: gateway StateChanged arrives first (before ChannelDeleted), must STILL fall back to ANNOUNCE & prune draft.
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::StateChanged], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        matches!(v.overlay, Overlay::None)
            && !v.drafts.contains_key(&GENERAL)
            && v.selected_channel == Some(ANNOUNCE)
    }));
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::ChannelDeleted { seq: ok_seq, channel_id: GENERAL, error: None }], cx);
    });
    cx.run_until_parked();
    draw(cx);

    assert!(view.read_with(cx, |v, _| {
        matches!(v.overlay, Overlay::None)
            && !v.drafts.contains_key(&GENERAL)
            && v.selected_channel == Some(ANNOUNCE)
    }));

    // Delete category CAT_MAIN -> ANNOUNCE stays selected & detached to root
    let edit_cat = cx.debug_bounds("edit-channel-10").unwrap().center();
    cx.simulate_click(edit_cat, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let del_btn = cx.debug_bounds("edit-channel-delete").unwrap().center();
    cx.simulate_click(del_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let confirm = cx.debug_bounds("edit-channel-delete-confirm").unwrap().center();
    cx.simulate_click(confirm, Modifiers::default());
    cx.run_until_parked();
    let cat_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::EditChannel(m) => m.seq,
        _ => panic!("expected EditChannel"),
    });
    backend.state().apply(&Dispatch {
        t: "CHANNEL_DELETE".to_owned(),
        s: 3,
        d: json!({ "id": CAT_MAIN.to_string() }),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::ChannelDeleted { seq: cat_seq, channel_id: CAT_MAIN, error: None }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None) && v.selected_channel == Some(ANNOUNCE)));
    assert_eq!(backend.state().channels.get(&ANNOUNCE).and_then(|c| c.parent_id), None);
}

#[gpui_kit::test]
fn channel_modals_isolate_global_shortcuts(cx: &mut TestAppContext) {
    let (_rt, _backend, view, cx) = open_guild(cx, ME, 0, 0);
    focus_composer(&view, cx);

    let create_btn = cx.debug_bounds("create-channel-btn").unwrap().center();
    cx.simulate_click(create_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    cx.simulate_keystrokes("ctrl-k alt-up alt-down ctrl-shift-n");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| {
        matches!(v.overlay, Overlay::CreateChannel(_)) && v.selected_channel == Some(GENERAL)
    }));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);

    let edit_sel = cx.debug_bounds("edit-selected-channel-btn").unwrap().center();
    cx.simulate_click(edit_sel, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    cx.simulate_keystrokes("ctrl-k alt-up alt-down ctrl-shift-n");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| {
        matches!(v.overlay, Overlay::EditChannel(_)) && v.selected_channel == Some(GENERAL)
    }));
}
