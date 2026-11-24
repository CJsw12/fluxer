//! Headless GPUI tests for Milestone 8: guild channel creation, editing,
//! two-step deletion, Escape behavior, category-scoped permissions, row
//! edit propagation isolation, and modal shortcut isolation.

use std::sync::{Arc, Mutex};

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{
    ClientIdentity, CreateGuildChannelRequest, Endpoints, GuildChannelPatch, RestClient, RestConfig, SecretToken,
    TransportPolicy, VOICE_CHANNEL_BITRATE_DEFAULT, VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT,
};
use fluxer_client_core::state::{ChannelKind, Snowflake, bits};
use gpui_kit::component::input::InputContent;
use gpui_kit::{Focusable as _, Modifiers, TestAppContext};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

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

fn open_guild_popout(cx: &mut gpui_kit::VisualTestContext) {
    let menu_btn = cx.debug_bounds("guild-header-menu-btn").expect("guild menu trigger drawn").center();
    cx.simulate_click(menu_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
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
        open_guild_popout(vcx);
        assert!(vcx.debug_bounds("guild-popout-create-channel").is_none());
        assert!(vcx.debug_bounds("guild-popout-roles").is_none());
        open_guild_popout(vcx);
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
    open_guild_popout(cx);
    assert!(cx.debug_bounds("guild-popout-create-channel").is_none());
    open_guild_popout(cx);
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
fn access_editor_uses_scoped_permissions_and_closes_on_gateway_revocation(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, bits::MANAGE_ROLES, 0);
    assert!(cx.debug_bounds("guild-roles-btn").is_none());
    open_guild_popout(cx);
    assert!(cx.debug_bounds("guild-popout-roles").is_some());
    open_guild_popout(cx);
    assert!(cx.debug_bounds("channel-permissions-11").is_some());
    assert!(cx.debug_bounds("edit-channel-11").is_none());
    let permissions = cx.debug_bounds("channel-permissions-11").unwrap().center();
    cx.simulate_click(permissions, Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::Access(_))));
    cx.simulate_keystrokes("ctrl-k alt-down");
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::Access(_)) && v.selected_channel == Some(GENERAL)));
    backend.state().apply(&Dispatch {
        t: "GUILD_ROLE_UPDATE".to_owned(),
        s: 2,
        d: json!({ "guild_id": GUILD, "role": {"id": GUILD, "name": "@everyone", "permissions": bits::DEFAULT.to_string()} }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| !v.overlay.is_open()));
    assert!(cx.debug_bounds("guild-roles-btn").is_none());
    open_guild_popout(cx);
    assert!(cx.debug_bounds("guild-popout-roles").is_none());
    open_guild_popout(cx);
    assert!(cx.debug_bounds("channel-permissions-11").is_none());
}

#[gpui_kit::test]
fn category_sync_move_retains_other_channel_settings(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, ME, 0, 0);
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.open_edit_channel(GENERAL, window, cx);
            v.set_edit_channel_parent(Some(CAT_OTHER), cx);
            let Overlay::EditChannel(modal) = &mut v.overlay else { panic!("expected channel editor") };
            assert!(modal.can_sync_parent);
            modal.sync_permissions = true;
            modal.name_input.update(cx, |input, cx| input.set_value("moved-general", window, cx));
            v.submit_edit_channel(cx);
        });
    });
    assert_eq!(
        *lock_sent(&backend.sent_guild_channels),
        vec![GuildChannelCall::SyncMove {
            channel_id: GENERAL,
            parent_id: CAT_OTHER,
            settings: GuildChannelPatch { name: Some("moved-general".into()), ..Default::default() },
        }]
    );
}

#[gpui_kit::test]
fn create_channel_validation_payloads_dedup_stale_seq_and_selection(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, ME, 0, 0);
    assert!(cx.debug_bounds("create-channel-btn").is_none());
    assert!(cx.debug_bounds("edit-selected-channel-btn").is_none());
    assert!(cx.debug_bounds("edit-channel-11").is_some());

    open_guild_popout(cx);
    assert!(cx.debug_bounds("guild-popout-create-channel").is_some());
    let create_btn = cx.debug_bounds("guild-popout-create-channel").unwrap().center();
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

    open_guild_popout(cx);
    let create_btn = cx.debug_bounds("guild-popout-create-channel").unwrap().center();
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
    open_guild_popout(cx);
    let create_btn = cx.debug_bounds("guild-popout-create-channel").unwrap().center();
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
    let edit_sel = cx.debug_bounds("edit-channel-11").unwrap().center();
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

    let edit_sel = cx.debug_bounds("edit-channel-11").unwrap().center();
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
    let edit_sel = cx.debug_bounds("edit-channel-11").unwrap().center();
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

    open_guild_popout(cx);
    let create_btn = cx.debug_bounds("guild-popout-create-channel").unwrap().center();
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

    let edit_sel = cx.debug_bounds("edit-channel-11").unwrap().center();
    cx.simulate_click(edit_sel, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    cx.simulate_keystrokes("ctrl-k alt-up alt-down ctrl-shift-n");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| {
        matches!(v.overlay, Overlay::EditChannel(_)) && v.selected_channel == Some(GENERAL)
    }));
}

#[derive(Debug, Clone, PartialEq)]
struct RecordedRoleRequest {
    method: String,
    path: String,
    body: Value,
}

fn spawn_role_mock_server(
    rt: &tokio::runtime::Runtime,
) -> (String, Arc<Mutex<Vec<RecordedRoleRequest>>>) {
    let recorded: Arc<Mutex<Vec<RecordedRoleRequest>>> = Arc::default();
    let recorded_srv = recorded.clone();
    let listener = rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind mock role server");
    let addr = listener.local_addr().expect("mock server addr");

    rt.spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = Vec::with_capacity(4096);
            let mut chunk = [0u8; 1024];
            let header_end = loop {
                let n = match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(pos + 4);
                }
            };
            let Some(header_end) = header_end else { continue };
            let headers = String::from_utf8_lossy(&buf[..header_end]);
            let mut lines = headers.lines();
            let req_line = lines.next().unwrap_or("");
            let mut parts = req_line.split_whitespace();
            let method = parts.next().unwrap_or("").to_owned();
            let path = parts.next().unwrap_or("").to_owned();
            let content_len: usize = lines
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse().ok())
                        .flatten()
                })
                .unwrap_or(0);
            while buf.len() < header_end + content_len {
                let n = match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
            }
            let body_bytes = &buf[header_end..buf.len().min(header_end + content_len)];
            let body: Value = if body_bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(body_bytes).unwrap_or(Value::Null)
            };

            let (status, resp_body) = match (method.as_str(), path.as_str()) {
                ("POST", "/api/v1/guilds/1/roles") => {
                    recorded_srv.lock().unwrap().push(RecordedRoleRequest {
                        method,
                        path,
                        body: body.clone(),
                    });
                    (
                        "200 OK",
                        json!({
                            "id": "500",
                            "name": body["name"],
                            "permissions": "0",
                            "position": 1,
                            "hoist": false,
                            "mentionable": false
                        })
                        .to_string(),
                    )
                }
                ("PATCH", "/api/v1/guilds/1/roles/500") => {
                    recorded_srv.lock().unwrap().push(RecordedRoleRequest {
                        method,
                        path,
                        body: body.clone(),
                    });
                    let name = body
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Helpers");
                    let permissions = body
                        .get("permissions")
                        .and_then(Value::as_str)
                        .unwrap_or("0");
                    let hoist = body.get("hoist").and_then(Value::as_bool).unwrap_or(false);
                    (
                        "200 OK",
                        json!({
                            "id": "500",
                            "name": name,
                            "permissions": permissions,
                            "position": 1,
                            "hoist": hoist,
                            "mentionable": false
                        })
                        .to_string(),
                    )
                }
                ("DELETE", "/api/v1/guilds/1/roles/500") => {
                    recorded_srv.lock().unwrap().push(RecordedRoleRequest {
                        method,
                        path,
                        body,
                    });
                    ("204 No Content", String::new())
                }
                _ => ("404 Not Found", json!({"message": "not found"}).to_string()),
            };

            let resp = if resp_body.is_empty() {
                format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            } else {
                format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
                    resp_body.len()
                )
            };
            let _ = stream.write_all(resp.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });

    (format!("http://{addr}/api"), recorded)
}

#[test]
fn role_create_edit_and_delete_backend_rest_round_trip() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("multi-thread test runtime");
    let (api_base, recorded) = spawn_role_mock_server(&rt);
    let endpoints = Endpoints {
        api_code_version: 1,
        api: api_base,
        gateway: "ws://127.0.0.1:1/gateway".to_owned(),
        media: None,
        static_cdn: None,
        upload_relay: None,
        invite: None,
        webapp: None,
        discovery_url: String::new(),
        document: Value::Null,
    };
    let mut config = RestConfig::new(&endpoints, ClientIdentity::default());
    config.policy = TransportPolicy { allow_insecure_loopback: true };
    let rest = Arc::new(RestClient::new(config).expect("loopback rest client"));
    rt.block_on(rest.set_token(Some(SecretToken::new("test-token"))));

    let (backend, mut events) = Backend::new(rt.handle().clone(), rest, None);
    let mod_role = Snowflake(200);
    let mod_perms = bits::VIEW_CHANNEL
        | bits::SEND_MESSAGES
        | bits::READ_MESSAGE_HISTORY
        | bits::MANAGE_ROLES
        | bits::MANAGE_MESSAGES;
    backend.state().apply(&Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": ME.to_string(), "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Dev Guild", "owner_id": OWNER.to_string() },
                "roles": [
                    { "id": GUILD.to_string(), "name": "@everyone", "permissions": "0", "position": 0 },
                    { "id": mod_role.to_string(), "name": "Moderator", "permissions": mod_perms.to_string(), "position": 5 }
                ],
                "channels": [
                    { "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 }
                ],
                "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [mod_role.to_string()] }]
            }],
            "private_channels": []
        }),
    });

    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        async fn wait_seq(
            events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
            deadline: tokio::time::Instant,
            target_seq: u64,
        ) {
            loop {
                match tokio::time::timeout_at(deadline, events.recv()).await {
                    Ok(Some(UiEvent::AccessChanged { seq, error })) if seq == target_seq => {
                        assert_eq!(error, None, "access change seq {seq} failed");
                        return;
                    }
                    Ok(Some(_)) => continue,
                    other => panic!("unexpected event waiting for AccessChanged({target_seq}): {other:?}"),
                }
            }
        }

        // 1. Create role "Helpers"
        backend.change_access(
            1,
            crate::backend::access::AccessChange::CreateRole {
                guild: GUILD,
                name: "Helpers".to_owned(),
            },
        );
        wait_seq(&mut events, deadline, 1).await;
        let created_role = Snowflake(500);
        let helpers_role = backend
            .state()
            .guilds
            .get(&GUILD)
            .and_then(|g| g.roles.get(&created_role))
            .cloned()
            .expect("created Helpers role");
        assert_eq!(helpers_role.name, "Helpers");

        // 2. Edit role "Helpers" -> "Supporters"
        backend.change_access(
            2,
            crate::backend::access::AccessChange::EditRole {
                guild: GUILD,
                before: helpers_role,
                patch: fluxer_client_core::rest::RolePatch {
                    name: Some("Supporters".to_owned()),
                    permissions: Some(bits::SEND_MESSAGES),
                    hoist: Some(true),
                    mentionable: None,
                },
            },
        );
        wait_seq(&mut events, deadline, 2).await;
        let supporters_role = backend
            .state()
            .guilds
            .get(&GUILD)
            .and_then(|g| g.roles.get(&created_role))
            .cloned()
            .expect("updated Supporters role");
        assert_eq!(supporters_role.name, "Supporters");

        // 3. Delete role "Supporters"
        backend.change_access(
            3,
            crate::backend::access::AccessChange::DeleteRole {
                guild: GUILD,
                before: supporters_role,
            },
        );
        wait_seq(&mut events, deadline, 3).await;
        assert!(!backend.state().guilds[&GUILD].roles.contains_key(&created_role));
    });

    let reqs = recorded.lock().unwrap();
    assert_eq!(reqs.len(), 3);
    assert_eq!(reqs[0].method, "POST");
    assert_eq!(reqs[0].path, "/api/v1/guilds/1/roles");
    assert_eq!(reqs[0].body, json!({ "name": "Helpers" }));
    assert_eq!(reqs[1].method, "PATCH");
    assert_eq!(reqs[1].path, "/api/v1/guilds/1/roles/500");
    assert_eq!(reqs[1].body["name"], json!("Supporters"));
    assert_eq!(reqs[2].method, "DELETE");
    assert_eq!(reqs[2].path, "/api/v1/guilds/1/roles/500");
}

#[gpui_kit::test]
fn role_editor_ui_create_edit_and_delete_submissions(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, ME, bits::MANAGE_ROLES, 0);
    open_guild_popout(cx);
    let roles_btn = cx.debug_bounds("guild-popout-roles").unwrap().center();
    cx.simulate_click(roles_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::Access(_))));
    assert!(cx.debug_bounds("access-modal").is_some());

    // Create role draft submits pending access request and settles on AccessChanged.
    view.update_in(cx, |v, window, cx| {
        v.test_access_create_role("Helpers", window, cx);
    });
    assert!(view.read_with(cx, |v, _| v.access_pending.is_some()));
    let seq1 = view.read_with(cx, |v, _| v.access_pending.unwrap());
    backend.state().apply(&Dispatch {
        t: "GUILD_ROLE_CREATE".to_owned(),
        s: 2,
        d: json!({
            "guild_id": GUILD.to_string(),
            "role": { "id": "500", "name": "Helpers", "permissions": "0", "position": 1, "hoist": false, "mentionable": false }
        }),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::StateChanged, UiEvent::AccessChanged { seq: seq1, error: None }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.access_pending.is_none()));

    // Edit role draft submits pending request and settles.
    let role = backend.state().guilds[&GUILD].roles[&Snowflake(500)].clone();
    view.update_in(cx, |v, window, cx| {
        v.test_access_edit_role(role, "Supporters", bits::SEND_MESSAGES, true, window, cx);
    });
    let seq2 = view.read_with(cx, |v, _| v.access_pending.expect("edit pending"));
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::AccessChanged { seq: seq2, error: None }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.access_pending.is_none()));

    // Two-step delete role draft submits pending request and settles.
    let role = backend.state().guilds[&GUILD].roles[&Snowflake(500)].clone();
    view.update_in(cx, |v, window, cx| {
        v.test_access_delete_role(role, window, cx);
    });
    let seq3 = view.read_with(cx, |v, _| v.access_pending.expect("delete pending"));
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::AccessChanged { seq: seq3, error: None }], cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.access_pending.is_none()));
}
