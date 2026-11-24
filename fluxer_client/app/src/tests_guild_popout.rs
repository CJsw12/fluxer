//! Headless GPUI tests for the 7 guild header popover actions and muted channel filtering.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{CreateGuildChannelRequest, CreateInviteRequest, GuildMemberPatch, UserSettingsPatch};
use fluxer_client_core::state::{ChannelKind, Snowflake, bits, message_notifications};
use gpui_kit::{Modifiers, TestAppContext};
use serde_json::json;

use crate::backend::{Backend, CreatedInvite, GuildChannelCall, ModerationCall, UiEvent, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{draw, offline_rest, open_client, runtime};
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const ME: Snowflake = Snowflake(100);
const OWNER: Snowflake = Snowflake(999);
const FRIEND: Snowflake = Snowflake(201);
const GUILD: Snowflake = Snowflake(1);
const CAT_MAIN: Snowflake = Snowflake(10);
const CAT_MUTED: Snowflake = Snowflake(15);
const GENERAL: Snowflake = Snowflake(11);
const MUTED_CH: Snowflake = Snowflake(12);
const MUTED_SELECTED_CH: Snowflake = Snowflake(13);
const MUTED_MENTION_CH: Snowflake = Snowflake(14);
const MUTED_IN_MUTED_CAT: Snowflake = Snowflake(16);
const DM_CH: Snowflake = Snowflake(50);

fn guild_ready(owner_id: Snowflake, role_perms: u64, with_dm: bool) -> Dispatch {
    let base = bits::VIEW_CHANNEL
        | bits::SEND_MESSAGES
        | bits::READ_MESSAGE_HISTORY
        | bits::CREATE_INSTANT_INVITE
        | role_perms;
    let dms = if with_dm {
        vec![json!({
            "id": DM_CH.to_string(),
            "type": 1,
            "recipients": [{ "id": FRIEND.to_string(), "username": "friend", "global_name": "Friend" }]
        })]
    } else {
        vec![]
    };
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": ME.to_string(), "username": "me", "global_name": "Me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Popout Guild", "owner_id": owner_id.to_string() },
                "roles": [{
                    "id": GUILD.to_string(),
                    "name": "@everyone",
                    "position": 0,
                    "permissions": base.to_string()
                }],
                "channels": [
                    { "id": CAT_MAIN.to_string(), "type": 4, "name": "Main", "position": 0 },
                    { "id": GENERAL.to_string(), "type": 0, "name": "general", "parent_id": CAT_MAIN.to_string(), "position": 0 },
                    { "id": MUTED_CH.to_string(), "type": 0, "name": "noisy", "parent_id": CAT_MAIN.to_string(), "position": 1 },
                    { "id": MUTED_SELECTED_CH.to_string(), "type": 0, "name": "pinned-muted", "parent_id": CAT_MAIN.to_string(), "position": 2 },
                    {
                        "id": MUTED_MENTION_CH.to_string(),
                        "type": 0,
                        "name": "alerts-muted",
                        "parent_id": CAT_MAIN.to_string(),
                        "position": 3,
                        "last_message_id": "900"
                    },
                    { "id": CAT_MUTED.to_string(), "type": 4, "name": "Muted Group", "position": 1 },
                    { "id": MUTED_IN_MUTED_CAT.to_string(), "type": 0, "name": "muted-child", "parent_id": CAT_MUTED.to_string(), "position": 0 }
                ],
                "members": [{
                    "user": { "id": ME.to_string(), "username": "me", "global_name": "Me" },
                    "nick": "OldNick",
                    "pronouns": "they/them",
                    "bio": "Old bio",
                    "roles": []
                }]
            }],
            "private_channels": dms,
            "user_guild_settings": [{
                "guild_id": GUILD.to_string(),
                "channel_overrides": [
                    { "channel_id": MUTED_CH.to_string(), "muted": true },
                    { "channel_id": MUTED_SELECTED_CH.to_string(), "muted": true },
                    { "channel_id": MUTED_MENTION_CH.to_string(), "muted": true },
                    { "channel_id": CAT_MUTED.to_string(), "muted": true }
                ]
            }],
            "read_states": [
                { "id": MUTED_MENTION_CH.to_string(), "last_message_id": "100", "mention_count": 1 }
            ]
        }),
    }
}

fn open_guild(
    cx: &mut TestAppContext,
    owner_id: Snowflake,
    role_perms: u64,
    with_dm: bool,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut gpui_kit::VisualTestContext) {
    let rt = runtime();
    let (mut backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.set_invite_origin_for_test(Some("https://fluxer.gg".to_owned()));
    assert!(!backend.state().apply(&guild_ready(owner_id, role_perms, with_dm)).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

fn click_selector(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    let pt = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("expected debug_bounds({selector})"))
        .center();
    cx.simulate_click(pt, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
}

#[gpui_kit::test]
fn guild_popout_create_category_opens_category_modal_and_submits(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, ME, 0, false);

    click_selector(cx, "guild-header-menu-btn");
    assert!(cx.debug_bounds("guild-popout-delete-messages").is_none(), "owner hides delete-my-messages");
    click_selector(cx, "guild-popout-create-category");

    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::CreateChannel(m) if m.kind == ChannelKind::GuildCategory && m.parent_id.is_none()
    )));
    cx.simulate_input("Secret Category");
    cx.run_until_parked();
    draw(cx);
    click_selector(cx, "create-channel-submit");

    assert_eq!(
        *lock_sent(&backend.sent_guild_channels),
        vec![GuildChannelCall::Create {
            guild_id: GUILD,
            request: CreateGuildChannelRequest {
                name: "Secret Category".to_owned(),
                kind: ChannelKind::GuildCategory,
                parent_id: None,
                topic: None,
                nsfw: None,
                rate_limit_per_user: None,
                bitrate: None,
                user_limit: None,
                voice_connection_limit: None,
            },
        }]
    );
}

#[gpui_kit::test]
fn guild_popout_hide_muted_channels_persists_and_filters_visibility(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, 0, false);
    view.update(cx, |v, cx| v.select_channel(MUTED_SELECTED_CH, cx));
    cx.run_until_parked();
    draw(cx);

    click_selector(cx, "guild-header-menu-btn");
    click_selector(cx, "guild-popout-hide-muted");

    assert!(view.read_with(cx, |v, _| v.guild_header_popout_open), "checkbox click keeps popout open");
    assert!(backend.state().guild_hides_muted_channels(GUILD));
    let visible_ids: Vec<Snowflake> = view.read_with(cx, |v, _| v.channels.iter().map(|c| c.id).collect());
    assert!(visible_ids.contains(&CAT_MAIN));
    assert!(visible_ids.contains(&GENERAL));
    assert!(visible_ids.contains(&MUTED_SELECTED_CH), "active muted channel stays visible");
    assert!(visible_ids.contains(&MUTED_MENTION_CH), "unread mentioned muted channel stays visible");
    assert!(!visible_ids.contains(&MUTED_CH), "plain muted channel hidden");
    assert!(!visible_ids.contains(&CAT_MUTED), "muted-only category hidden");
    assert!(!visible_ids.contains(&MUTED_IN_MUTED_CAT), "children under muted category hidden");

    // Toggle back off persists false and reveals muted items again.
    click_selector(cx, "guild-popout-hide-muted");
    assert!(!backend.state().guild_hides_muted_channels(GUILD));
    let restored_ids: Vec<Snowflake> = view.read_with(cx, |v, _| v.channels.iter().map(|c| c.id).collect());
    assert!(restored_ids.contains(&MUTED_CH));
    assert!(restored_ids.contains(&CAT_MUTED));

    let sent = lock_sent(&backend.sent_guild_settings);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].1.hide_muted_channels, Some(true));
    assert_eq!(sent[1].1.hide_muted_channels, Some(false));
}

#[gpui_kit::test]
fn guild_popout_invite_members_creates_copies_sends_and_regenerates(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, ME, 0, true);

    click_selector(cx, "guild-header-menu-btn");
    click_selector(cx, "guild-popout-invite");

    let first_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::InviteMembers(m) => {
            assert_eq!(m.channel_id, GENERAL);
            assert!(m.loading);
            m.seq
        }
        _ => panic!("expected InviteMembers modal"),
    });
    assert_eq!(
        *lock_sent(&backend.sent_invites),
        vec![(
            GENERAL,
            CreateInviteRequest {
                max_age: Some(604_800),
                max_uses: Some(0),
                temporary: Some(false),
                unique: None,
            }
        )]
    );

    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::InviteCreated {
                seq: first_seq,
                channel_id: GENERAL,
                result: Ok(CreatedInvite {
                    code: "abc123".to_owned(),
                    max_age: 604_800,
                    max_uses: 0,
                    temporary: false,
                }),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);

    click_selector(cx, "invite-copy-btn");
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
        Some("https://fluxer.gg/abc123")
    );
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::InviteMembers(m) if m.copied)));

    click_selector(cx, "invite-send-dm-50");
    {
        let sent = lock_sent(&backend.sent);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, DM_CH);
        assert_eq!(sent[0].1.content.as_deref(), Some("https://fluxer.gg/abc123"));
    }

    click_selector(cx, "invite-edit-link-btn");
    click_selector(cx, "invite-age-3600");
    click_selector(cx, "invite-uses-5");
    click_selector(cx, "invite-temp-switch");
    click_selector(cx, "invite-generate-btn");

    {
        let sent = lock_sent(&backend.sent_invites);
        assert_eq!(sent.len(), 2);
        assert_eq!(
            sent[1],
            (
                GENERAL,
                CreateInviteRequest {
                    max_age: Some(3600),
                    max_uses: Some(5),
                    temporary: Some(true),
                    unique: Some(true),
                }
            )
        );
    }

    // Guild disabling invites dismisses/hides Invite members item.
    backend.state().apply(&Dispatch {
        t: "GUILD_UPDATE".to_owned(),
        s: 2,
        d: json!({
            "id": GUILD.to_string(),
            "name": "Popout Guild",
            "owner_id": ME.to_string(),
            "features": ["INVITES_DISABLED"]
        }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    assert!(!view.read_with(cx, |v, _| v.overlay.is_open()));
    click_selector(cx, "guild-header-menu-btn");
    assert!(cx.debug_bounds("guild-popout-invite").is_none());
}

#[gpui_kit::test]
fn guild_popout_notifications_settings_updates_guild_and_overrides(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, ME, 0, false);

    click_selector(cx, "guild-header-menu-btn");
    click_selector(cx, "guild-popout-notifications");
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::GuildNotifications(_))));

    click_selector(cx, "guild-notif-mute-switch");
    click_selector(cx, "guild-notif-level-1");
    click_selector(cx, "guild-notif-suppress-everyone");
    click_selector(cx, "guild-notif-suppress-roles");

    {
        let state = backend.state();
        let gs = &state.guild_settings[&Some(GUILD)];
        assert!(gs.muted);
        assert_eq!(gs.message_notifications, message_notifications::ONLY_MENTIONS);
        assert!(gs.suppress_everyone);
        assert!(gs.suppress_roles);
    }

    click_selector(cx, "guild-notif-add-override-btn");
    click_selector(cx, "guild-notif-pick-11");
    click_selector(cx, "guild-notif-ov-level-11-2");
    click_selector(cx, "guild-notif-ov-mute-11");

    {
        let state = backend.state();
        let ov = &state.guild_settings[&Some(GUILD)].channel_overrides[&GENERAL];
        assert!(ov.muted);
        assert_eq!(ov.message_notifications, Some(message_notifications::NO_MESSAGES));
    }

    click_selector(cx, "guild-notif-ov-remove-11");
    assert!(!backend.state().guild_settings[&Some(GUILD)]
        .channel_overrides
        .contains_key(&GENERAL));
    assert!(!lock_sent(&backend.sent_guild_settings).is_empty());
}

#[gpui_kit::test]
fn guild_popout_privacy_settings_toggles_restrictions_and_respects_guild_disable(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, 0, false);

    click_selector(cx, "guild-header-menu-btn");
    click_selector(cx, "guild-popout-privacy");
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::GuildPrivacy(_))));

    click_selector(cx, "guild-privacy-dms-switch");
    click_selector(cx, "guild-privacy-bot-dms-switch");
    assert_eq!(backend.state().user_settings.restricted_guilds(), vec![GUILD]);
    assert_eq!(backend.state().user_settings.bot_restricted_guilds(), vec![GUILD]);

    click_selector(cx, "guild-privacy-dms-switch");
    assert!(backend.state().user_settings.restricted_guilds().is_empty());

    // When community disables DMs, switch becomes disabled and ignores clicks.
    backend.state().apply(&Dispatch {
        t: "GUILD_UPDATE".to_owned(),
        s: 2,
        d: json!({
            "id": GUILD.to_string(),
            "name": "Popout Guild",
            "owner_id": OWNER.to_string(),
            "features": ["MEMBER_DMS_DISABLED"]
        }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    let sent_before = lock_sent(&backend.sent_settings).len();
    click_selector(cx, "guild-privacy-dms-switch");
    assert_eq!(lock_sent(&backend.sent_settings).len(), sent_before);
    assert_eq!(
        *lock_sent(&backend.sent_settings).first().unwrap(),
        UserSettingsPatch {
            restricted_guilds: Some(vec![GUILD.to_string()]),
            ..Default::default()
        }
    );
}

#[gpui_kit::test]
fn guild_popout_edit_profile_validates_and_submits(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, bits::CHANGE_NICKNAME, false);

    click_selector(cx, "guild-header-menu-btn");
    click_selector(cx, "guild-popout-edit-profile");

    view.read_with(cx, |v, cx| match &v.overlay {
        Overlay::GuildProfile(m) => {
            assert!(m.can_edit_nick);
            assert_eq!(m.nick_input.read(cx).value(), "OldNick");
            assert_eq!(m.pronouns_input.read(cx).value(), "they/them");
            assert_eq!(m.bio_input.read(cx).value(), "Old bio");
        }
        _ => panic!("expected GuildProfile modal"),
    });

    // Validation catches >40 UTF-16 pronouns locally without issuing REST call.
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::GuildProfile(m) = &v.overlay {
                m.pronouns_input.update(cx, |s, cx| s.set_value(&"p".repeat(41), window, cx));
            }
        });
    });
    cx.run_until_parked();
    draw(cx);
    click_selector(cx, "guild-profile-save-btn");
    assert!(lock_sent(&backend.sent_moderation).is_empty());
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::GuildProfile(m) if m.error.is_some())));

    // Valid pronouns + cleared bio sends PATCH /members/@me (nick unchanged -> None).
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::GuildProfile(m) = &v.overlay {
                m.pronouns_input.update(cx, |s, cx| s.set_value("she/her", window, cx));
                m.bio_input.update(cx, |s, cx| s.set_value("", window, cx));
            }
        });
    });
    cx.run_until_parked();
    draw(cx);
    click_selector(cx, "guild-profile-save-btn");

    assert_eq!(
        *lock_sent(&backend.sent_moderation),
        vec![ModerationCall::UpdateMember {
            guild_id: GUILD,
            user_id: ME,
            is_self: true,
            patch: GuildMemberPatch {
                nick: None,
                pronouns: Some(Some("she/her".to_owned())),
                bio: Some(None),
                ..Default::default()
            },
            reason: None,
        }]
    );

    let seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::GuildProfile(m) => m.seq,
        _ => panic!("expected GuildProfile"),
    });
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::MemberModerated {
                seq,
                guild_id: GUILD,
                user_id: ME,
                error: None,
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.overlay.is_open()));
}

#[gpui_kit::test]
fn guild_popout_delete_my_messages_handles_password_error_and_success(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, 0, false);

    click_selector(cx, "guild-header-menu-btn");
    click_selector(cx, "guild-popout-delete-messages");

    cx.simulate_input("s3cr3t");
    cx.run_until_parked();
    draw(cx);
    click_selector(cx, "guild-delete-messages-confirm-btn");

    assert_eq!(*lock_sent(&backend.sent_guild_bulk_deletes), vec![(GUILD, true)]);
    let seq1 = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::DeleteMyGuildMessages(m) => m.seq,
        _ => panic!("expected DeleteMyGuildMessages"),
    });

    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::GuildMessagesBulkDeleted {
                seq: seq1,
                guild_id: GUILD,
                error: Some("Invalid password".to_owned()),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::DeleteMyGuildMessages(m) if !m.submitting && m.error.as_deref() == Some("Invalid password")
    )));

    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::DeleteMyGuildMessages(m) = &v.overlay {
                m.password_input.update(cx, |s, cx| s.set_value("", window, cx));
            }
        });
    });
    cx.run_until_parked();
    draw(cx);
    click_selector(cx, "guild-delete-messages-confirm-btn");
    assert_eq!(
        *lock_sent(&backend.sent_guild_bulk_deletes),
        vec![(GUILD, true), (GUILD, false)]
    );

    let seq2 = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::DeleteMyGuildMessages(m) => m.seq,
        _ => panic!("expected DeleteMyGuildMessages"),
    });
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::GuildMessagesBulkDeleted {
                seq: seq2,
                guild_id: GUILD,
                error: None,
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.overlay.is_open()));
}
