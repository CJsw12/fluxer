//! Headless GPUI tests for Milestone 9: guild member moderation (kick, ban,
//! timeout, nickname), role hierarchy gating, ban list modal, and modal shortcut isolation.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{BanMemberRequest, GuildBanEntry, GuildMemberPatch};
use fluxer_client_core::state::{Snowflake, User, bits, parse_timestamp_ms};
use gpui_kit::{Focusable as _, Modifiers, TestAppContext, point, px};
use serde_json::json;

use crate::backend::{Backend, ModerationCall, UiEvent, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{draw, focus_composer, offline_rest, open_client, runtime};
use crate::ui::moderation_modal::ModerationKind;
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const ME: Snowflake = Snowflake(100);
const OWNER: Snowflake = Snowflake(999);
const PEER_MOD: Snowflake = Snowflake(201);
const LOW_USER: Snowflake = Snowflake(202);
const ADMIN_USER: Snowflake = Snowflake(203);
const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);
const OTHER_CH: Snowflake = Snowflake(12);

const ROLE_MOD_TIE: Snowflake = Snowflake(1001); // pos 5, lower id -> higher than ROLE_MOD_HIGH in tie
const ROLE_MOD_HIGH: Snowflake = Snowflake(1002); // pos 5
const ROLE_LOW: Snowflake = Snowflake(1003); // pos 1
const ROLE_ADMIN: Snowflake = Snowflake(1004); // pos 10

fn guild_ready(owner_id: Snowflake, me_roles: Vec<Snowflake>) -> Dispatch {
    let base_view = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY | bits::CHANGE_NICKNAME;
    let me_role_strs: Vec<String> = me_roles.into_iter().map(|r| r.0.to_string()).collect();
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": ME.to_string(), "username": "me", "global_name": "Me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Mod Guild", "owner_id": owner_id.to_string() },
                "roles": [
                    { "id": GUILD.to_string(), "name": "@everyone", "position": 0, "permissions": base_view.to_string() },
                    {
                        "id": ROLE_MOD_TIE.to_string(),
                        "name": "peer-mod",
                        "position": 5,
                        "permissions": (bits::KICK_MEMBERS | bits::BAN_MEMBERS).to_string()
                    },
                    {
                        "id": ROLE_MOD_HIGH.to_string(),
                        "name": "mod-high",
                        "position": 5,
                        "permissions": (bits::KICK_MEMBERS | bits::BAN_MEMBERS | bits::MODERATE_MEMBERS | bits::MANAGE_NICKNAMES).to_string()
                    },
                    {
                        "id": ROLE_LOW.to_string(),
                        "name": "member",
                        "position": 1,
                        "permissions": "0"
                    },
                    {
                        "id": ROLE_ADMIN.to_string(),
                        "name": "admin",
                        "position": 10,
                        "permissions": bits::ADMINISTRATOR.to_string()
                    }
                ],
                "channels": [
                    { "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 },
                    { "id": OTHER_CH.to_string(), "type": 0, "name": "random", "position": 1 }
                ],
                "members": [
                    { "user": { "id": ME.to_string(), "username": "me", "global_name": "Me" }, "nick": "MeOld", "roles": me_role_strs },
                    { "user": { "id": OWNER.to_string(), "username": "owner", "global_name": "Owner" }, "roles": [] },
                    { "user": { "id": PEER_MOD.to_string(), "username": "peermod", "global_name": "Peer" }, "roles": [ROLE_MOD_TIE.to_string()] },
                    { "user": { "id": LOW_USER.to_string(), "username": "alice", "global_name": "Alice" }, "nick": "Ally", "roles": [ROLE_LOW.to_string()] },
                    { "user": { "id": ADMIN_USER.to_string(), "username": "admin", "global_name": "Admin" }, "roles": [ROLE_ADMIN.to_string()] }
                ]
            }],
            "private_channels": []
        }),
    }
}

fn open_guild(
    cx: &mut TestAppContext,
    owner_id: Snowflake,
    me_roles: Vec<Snowflake>,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut gpui_kit::VisualTestContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    assert!(!backend.state().apply(&guild_ready(owner_id, me_roles)).is_empty());
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

fn open_popout_for(view: &gpui_kit::Entity<ClientView>, cx: &mut gpui_kit::VisualTestContext, user_id: Snowflake) {
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.open_profile(user_id, Some(GUILD), point(px(120.), px(120.)), window, cx);
        });
    });
    cx.run_until_parked();
    draw(cx);
}

fn open_guild_popout_for_bans(cx: &mut gpui_kit::VisualTestContext) {
    let menu_btn = cx.debug_bounds("guild-header-menu-btn").expect("guild menu trigger drawn").center();
    cx.simulate_click(menu_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
}

#[gpui_kit::test]
fn moderation_permissions_hierarchy_profile_buttons_and_bans_entry(cx: &mut TestAppContext) {
    // 1. Plain member: no Bans button; can change own nickname, cannot moderate others.
    {
        let mut sub_cx = cx.clone();
        let (_rt, _backend, view, vcx) = open_guild(&mut sub_cx, OWNER, vec![ROLE_LOW]);
        assert!(vcx.debug_bounds("guild-bans-btn").is_none());
        open_guild_popout_for_bans(vcx);
        assert!(vcx.debug_bounds("guild-popout-bans").is_none());
        open_guild_popout_for_bans(vcx);

        open_popout_for(&view, vcx, ME);
        assert!(vcx.debug_bounds("profile-nickname-btn").is_some());
        assert!(vcx.debug_bounds("profile-kick-btn").is_none());

        open_popout_for(&view, vcx, LOW_USER);
        assert!(vcx.debug_bounds("profile-nickname-btn").is_none());
        assert!(vcx.debug_bounds("profile-timeout-btn").is_none());
        assert!(vcx.debug_bounds("profile-kick-btn").is_none());
        assert!(vcx.debug_bounds("profile-ban-btn").is_none());

        // Direct open attempt without permission stays closed.
        vcx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.open_member_moderation(GUILD, LOW_USER, ModerationKind::Kick, window, cx);
                assert!(matches!(v.overlay, Overlay::Profile(_)));
            });
        });
    }

    // 2. Mod role: beats LOW_USER, loses tie to PEER_MOD, cannot moderate OWNER, exposes Bans in guild popout.
    {
        let mut sub_cx = cx.clone();
        let (_rt, _backend, view, vcx) = open_guild(&mut sub_cx, OWNER, vec![ROLE_MOD_HIGH]);
        assert!(vcx.debug_bounds("guild-bans-btn").is_none());
        open_guild_popout_for_bans(vcx);
        // Official popout has no Bans item; bans open from Community settings.
        assert!(vcx.debug_bounds("guild-popout-bans").is_none());
        open_guild_popout_for_bans(vcx);

        open_popout_for(&view, vcx, LOW_USER);
        assert!(vcx.debug_bounds("profile-nickname-btn").is_some());
        assert!(vcx.debug_bounds("profile-timeout-btn").is_some());
        assert!(vcx.debug_bounds("profile-kick-btn").is_some());
        assert!(vcx.debug_bounds("profile-ban-btn").is_some());

        open_popout_for(&view, vcx, PEER_MOD);
        assert!(vcx.debug_bounds("profile-kick-btn").is_none());
        assert!(vcx.debug_bounds("profile-ban-btn").is_none());
        assert!(vcx.debug_bounds("profile-timeout-btn").is_none());

        open_popout_for(&view, vcx, OWNER);
        assert!(vcx.debug_bounds("profile-kick-btn").is_none());
        assert!(vcx.debug_bounds("profile-ban-btn").is_none());
    }

    // 3. Owner: can kick/ban/nick ADMIN_USER, but CANNOT timeout administrator.
    let (_rt, _backend, view, cx) = open_guild(cx, ME, vec![]);
    open_popout_for(&view, cx, ADMIN_USER);
    assert!(cx.debug_bounds("profile-nickname-btn").is_some());
    assert!(cx.debug_bounds("profile-kick-btn").is_some());
    assert!(cx.debug_bounds("profile-ban-btn").is_some());
    assert!(cx.debug_bounds("profile-timeout-btn").is_none(), "administrators cannot be timed out");
}

#[gpui_kit::test]
fn kick_and_ban_validation_presets_dedup_stale_seq_error_and_success(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, vec![ROLE_MOD_HIGH]);
    open_popout_for(&view, cx, LOW_USER);

    let kick_btn = cx.debug_bounds("profile-kick-btn").unwrap().center();
    cx.simulate_click(kick_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::MemberModeration(m) if m.user_id == LOW_USER && m.kind == ModerationKind::Kick
    )));

    // Reason > 512 UTF-16 rejected locally.
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::MemberModeration(m) = &v.overlay {
                m.primary_input.update(cx, |s, cx| s.set_value(&"😀".repeat(257), window, cx));
            }
        });
    });
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("moderation-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert!(lock_sent(&backend.sent_moderation).is_empty());
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::MemberModeration(m) if m.error.is_some())));

    // Valid reason + double-submit sends once.
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::MemberModeration(m) = &v.overlay {
                let focus = m.primary_input.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
                m.primary_input.update(cx, |s, cx| s.set_value("", window, cx));
            }
        });
    });
    cx.simulate_input("  spamming  ");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::MemberModeration(m) if m.error.is_none())));
    let submit = cx.debug_bounds("moderation-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        *lock_sent(&backend.sent_moderation),
        vec![ModerationCall::Kick {
            guild_id: GUILD,
            user_id: LOW_USER,
            reason: Some("spamming".into()),
        }]
    );
    let first_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::MemberModeration(m) => m.seq,
        _ => panic!("expected MemberModeration"),
    });

    // Server error keeps modal open; Escape + reopen ignores stale first_seq.
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::MemberModerated {
                seq: first_seq,
                guild_id: GUILD,
                user_id: LOW_USER,
                error: Some("Cannot kick".into()),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::MemberModeration(m) if !m.submitting && m.error.as_deref() == Some("Cannot kick")
    )));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // Error arriving for closed modal with latest seq surfaces on view.error (P2-6).
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::MemberModerated {
                seq: first_seq,
                guild_id: GUILD,
                user_id: LOW_USER,
                error: Some("Bg kick fail".into()),
            }],
            cx,
        );
    });
    assert!(view.read_with(cx, |v, _| v.error.as_deref() == Some("Bg kick fail")));

    // Open Ban modal: default delete=86400 (P2-4), switch to 3600, duration=604800, verify stale seq ignored, then apply success.
    open_popout_for(&view, cx, LOW_USER);
    let ban_btn = cx.debug_bounds("profile-ban-btn").unwrap().center();
    cx.simulate_click(ban_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::MemberModeration(m) if m.kind == ModerationKind::Ban && m.ban_delete_seconds == 86_400
    )));
    let del_hour = cx.debug_bounds("mod-ban-del-3600").unwrap().center();
    cx.simulate_click(del_hour, Modifiers::default());
    let dur_week = cx.debug_bounds("mod-ban-dur-604800").unwrap().center();
    cx.simulate_click(dur_week, Modifiers::default());
    cx.simulate_input("ban reason");
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("moderation-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        lock_sent(&backend.sent_moderation)[1],
        ModerationCall::Ban {
            guild_id: GUILD,
            user_id: LOW_USER,
            request: BanMemberRequest {
                delete_message_seconds: Some(3600),
                reason: Some("ban reason".into()),
                ban_duration_seconds: Some(604800),
            },
        }
    );
    let ban_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::MemberModeration(m) => m.seq,
        _ => panic!("expected MemberModeration"),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::MemberModerated { seq: first_seq, guild_id: GUILD, user_id: LOW_USER, error: None }], cx);
    });
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::MemberModeration(_))));

    backend.state().apply(&Dispatch {
        t: "GUILD_MEMBER_REMOVE".to_owned(),
        s: 2,
        d: json!({ "guild_id": GUILD.to_string(), "user": { "id": LOW_USER.to_string() } }),
    });
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::MemberModerated { seq: ban_seq, guild_id: GUILD, user_id: LOW_USER, error: None }], cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert!(!backend.state().members.get(&GUILD).unwrap().contains_key(&LOW_USER));
    let composer_focused = cx.update(|window, cx| view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    assert!(composer_focused);
}

#[gpui_kit::test]
fn timeout_and_nickname_set_clear_self_vs_other_routes(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, vec![ROLE_MOD_HIGH]);
    open_popout_for(&view, cx, LOW_USER);

    let timeout_btn = cx.debug_bounds("profile-timeout-btn").unwrap().center();
    cx.simulate_click(timeout_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("mod-timeout-clear").is_none());
    let dur_hour = cx.debug_bounds("mod-timeout-3600").unwrap().center();
    cx.simulate_click(dur_hour, Modifiers::default());
    cx.simulate_input("cool off");
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("moderation-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();

    let sent = lock_sent(&backend.sent_moderation).clone();
    let ModerationCall::UpdateMember { guild_id, user_id, is_self, patch, reason } = &sent[0] else {
        panic!("expected UpdateMember timeout call");
    };
    assert_eq!((*guild_id, *user_id, *is_self), (GUILD, LOW_USER, false));
    assert_eq!(reason.as_deref(), Some("cool off"));
    let Some(Some(ts)) = &patch.communication_disabled_until else {
        panic!("expected timeout timestamp");
    };
    assert!(parse_timestamp_ms(ts).is_some_and(|ms| ms > crate::accounts::now_ms() as i64));

    // Close modal, mark member as timed out, verify Remove timeout clears via Some(None).
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    backend.state().apply(&Dispatch {
        t: "GUILD_MEMBER_UPDATE".to_owned(),
        s: 2,
        d: json!({
            "guild_id": GUILD.to_string(),
            "user": { "id": LOW_USER.to_string(), "username": "alice", "global_name": "Alice" },
            "roles": [ROLE_LOW.to_string()],
            "communication_disabled_until": "2999-01-01T00:00:00Z"
        }),
    });
    open_popout_for(&view, cx, LOW_USER);
    let timeout_btn = cx.debug_bounds("profile-timeout-btn").unwrap().center();
    cx.simulate_click(timeout_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let clear_btn = cx.debug_bounds("mod-timeout-clear").expect("remove timeout shown").center();
    cx.simulate_click(clear_btn, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        lock_sent(&backend.sent_moderation)[1],
        ModerationCall::UpdateMember {
            guild_id: GUILD,
            user_id: LOW_USER,
            is_self: false,
            patch: GuildMemberPatch {
                nick: None,
                communication_disabled_until: Some(None),
                timeout_reason: None,
                ..Default::default()
            },
            reason: None,
        }
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // Nickname on self (is_self = true): unchanged noop, >32 UTF-16 rejected, clear sends Some(None).
    open_popout_for(&view, cx, ME);
    let nick_btn = cx.debug_bounds("profile-nickname-btn").unwrap().center();
    cx.simulate_click(nick_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    let submit = cx.debug_bounds("moderation-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    assert_eq!(lock_sent(&backend.sent_moderation).len(), 2, "unchanged nickname must not send");

    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::MemberModeration(m) = &v.overlay {
                m.primary_input.update(cx, |s, cx| s.set_value(&"😀".repeat(17), window, cx));
            }
        });
    });
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("moderation-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    assert_eq!(lock_sent(&backend.sent_moderation).len(), 2);
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::MemberModeration(m) if m.error.is_some())));

    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::MemberModeration(m) = &v.overlay {
                m.primary_input.update(cx, |s, cx| s.set_value("   ", window, cx));
            }
        });
    });
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("moderation-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        lock_sent(&backend.sent_moderation)[2],
        ModerationCall::UpdateMember {
            guild_id: GUILD,
            user_id: ME,
            is_self: true,
            patch: GuildMemberPatch { nick: Some(None), ..Default::default() },
            reason: None,
        }
    );

    // Editing another member's nickname sets is_self = false.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    open_popout_for(&view, cx, LOW_USER);
    let nick_btn = cx.debug_bounds("profile-nickname-btn").unwrap().center();
    cx.simulate_click(nick_btn, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            if let Overlay::MemberModeration(m) = &v.overlay {
                m.primary_input.update(cx, |s, cx| s.set_value("  Buddy  ", window, cx));
            }
        });
    });
    cx.run_until_parked();
    draw(cx);
    let submit = cx.debug_bounds("moderation-submit").unwrap().center();
    cx.simulate_click(submit, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        lock_sent(&backend.sent_moderation)[3],
        ModerationCall::UpdateMember {
            guild_id: GUILD,
            user_id: LOW_USER,
            is_self: false,
            patch: GuildMemberPatch { nick: Some(Some("Buddy".into())), ..Default::default() },
            reason: None,
        }
    );

    // Timed-out self hides own nickname button (P2-1).
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    backend.state().apply(&Dispatch {
        t: "GUILD_MEMBER_UPDATE".to_owned(),
        s: 3,
        d: json!({
            "guild_id": GUILD.to_string(),
            "user": { "id": ME.to_string(), "username": "me", "global_name": "Me" },
            "nick": "MeOld",
            "roles": [ROLE_MOD_HIGH.to_string()],
            "communication_disabled_until": "2999-01-01T00:00:00Z"
        }),
    });
    open_popout_for(&view, cx, ME);
    assert!(cx.debug_bounds("profile-nickname-btn").is_none());
}

fn ban_entry(id: u64, username: &str, reason: &str) -> GuildBanEntry {
    GuildBanEntry {
        user: User {
            id: Snowflake(id),
            username: username.into(),
            discriminator: None,
            global_name: Some(format!("Global {username}")),
            avatar: None,
            banner: None,
            avatar_color: None,
            bot: false,
        },
        reason: Some(reason.into()),
        moderator_id: Some(OWNER.0.to_string()),
        banned_at: Some("2026-11-19T00:00:00Z".into()),
        expires_at: None,
    }
}

#[gpui_kit::test]
fn guild_bans_list_filter_stale_seq_unban_and_error(cx: &mut TestAppContext) {
    let (_rt, backend, view, cx) = open_guild(cx, OWNER, vec![ROLE_MOD_HIGH]);
    cx.update(|window, app| view.update(app, |v, cx| v.open_guild_bans(window, cx)));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(*lock_sent(&backend.sent_moderation), vec![ModerationCall::ListBans { guild_id: GUILD }]);
    let first_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::GuildBans(m) => m.seq,
        _ => panic!("expected GuildBans"),
    });

    // Resolve first_seq with an error so Refresh unlocks, then verify Refresh bumps seq and ignores stale first_seq.
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::GuildBansLoaded { seq: first_seq, guild_id: GUILD, result: Err("Temporary error".into()) }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    let refresh_btn = cx.debug_bounds("guild-bans-refresh").unwrap().center();
    cx.simulate_click(refresh_btn, Modifiers::default());
    cx.run_until_parked();
    let seq2 = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::GuildBans(m) => m.seq,
        _ => panic!("expected GuildBans"),
    });
    assert_ne!(seq2, first_seq);
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::GuildBansLoaded { seq: first_seq, guild_id: GUILD, result: Ok(vec![ban_entry(901, "stale", "x")]) }],
            cx,
        );
    });
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::GuildBans(m) if m.bans.is_empty() && m.loading)));

    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::GuildBansLoaded {
                seq: seq2,
                guild_id: GUILD,
                result: Ok(vec![ban_entry(201, "alice", "spam"), ban_entry(202, "bob", "troll")]),
            }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("unban-user-201").is_some());
    assert!(cx.debug_bounds("unban-user-202").is_some());

    // Filter hides bob, keeps alice.
    cx.simulate_input("alice");
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("unban-user-201").is_some());
    assert!(cx.debug_bounds("unban-user-202").is_none());

    // Two-step unban confirmation (P2-5), double-click guard, failure retry, and Refresh/Unban race (P2-7).
    let unban_btn = cx.debug_bounds("unban-user-201").unwrap().center();
    cx.simulate_click(unban_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(lock_sent(&backend.sent_moderation).len(), 2, "first click only reveals confirm buttons");
    let cancel_btn = cx.debug_bounds("unban-cancel-201").unwrap().center();
    cx.simulate_click(cancel_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("unban-user-201").is_some());

    let unban_btn = cx.debug_bounds("unban-user-201").unwrap().center();
    cx.simulate_click(unban_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let confirm_btn = cx.debug_bounds("unban-confirm-201").unwrap().center();
    cx.simulate_click(confirm_btn, Modifiers::default());
    cx.simulate_click(confirm_btn, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        lock_sent(&backend.sent_moderation)[2],
        ModerationCall::Unban { guild_id: GUILD, user_id: Snowflake(201), reason: None }
    );
    assert_eq!(lock_sent(&backend.sent_moderation).len(), 3);
    let fail_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::GuildBans(m) => m.unban_seqs[&Snowflake(201)],
        _ => panic!("expected GuildBans"),
    });
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![UiEvent::MemberUnbanned { seq: fail_seq, guild_id: GUILD, user_id: Snowflake(201), error: Some("Unban failed".into()) }],
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::GuildBans(m) if m.bans.len() == 2 && m.error.as_deref() == Some("Unban failed")
    )));

    // Refresh started BEFORE unban completes must not resurrect user 201 when it returns later.
    let refresh_btn = cx.debug_bounds("guild-bans-refresh").unwrap().center();
    cx.simulate_click(refresh_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let stale_list_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::GuildBans(m) => m.seq,
        _ => panic!("expected GuildBans"),
    });

    let unban_btn = cx.debug_bounds("unban-user-201").unwrap().center();
    cx.simulate_click(unban_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let confirm_btn = cx.debug_bounds("unban-confirm-201").unwrap().center();
    cx.simulate_click(confirm_btn, Modifiers::default());
    cx.run_until_parked();
    let ok_seq = view.read_with(cx, |v, _| match &v.overlay {
        Overlay::GuildBans(m) => m.unban_seqs[&Snowflake(201)],
        _ => panic!("expected GuildBans"),
    });
    assert!(ok_seq > stale_list_seq);
    view.update(cx, |v, cx| {
        v.handle_events(
            vec![
                UiEvent::MemberUnbanned { seq: ok_seq, guild_id: GUILD, user_id: Snowflake(201), error: None },
                UiEvent::GuildBansLoaded {
                    seq: stale_list_seq,
                    guild_id: GUILD,
                    result: Ok(vec![ban_entry(201, "alice", "spam"), ban_entry(202, "bob", "troll")]),
                },
            ],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(
        &v.overlay,
        Overlay::GuildBans(m) if m.bans.len() == 1 && m.bans[0].user.id == Snowflake(202)
    )));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let composer_focused = cx.update(|window, cx| view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    assert!(composer_focused);

    // Switching place away from guild dismisses GuildBans modal.
    draw(cx);
    cx.update(|window, app| view.update(app, |v, cx| v.open_guild_bans(window, cx)));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::GuildBans(_))));
    view.update(cx, |v, cx| v.select_place(Place::Home, cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
}

#[gpui_kit::test]
fn moderation_and_bans_modals_isolate_global_shortcuts(cx: &mut TestAppContext) {
    let (_rt, _backend, view, cx) = open_guild(cx, ME, vec![]);
    focus_composer(&view, cx);

    open_popout_for(&view, cx, LOW_USER);
    let kick_btn = cx.debug_bounds("profile-kick-btn").unwrap().center();
    cx.simulate_click(kick_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    cx.simulate_keystrokes("ctrl-k alt-up alt-down ctrl-shift-n");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| {
        matches!(v.overlay, Overlay::MemberModeration(_)) && v.selected_channel == Some(GENERAL)
    }));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);

    cx.update(|window, app| view.update(app, |v, cx| v.open_guild_bans(window, cx)));
    cx.run_until_parked();
    draw(cx);

    cx.simulate_keystrokes("ctrl-k alt-up alt-down ctrl-shift-n");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| {
        matches!(v.overlay, Overlay::GuildBans(_)) && v.selected_channel == Some(GENERAL)
    }));
}
