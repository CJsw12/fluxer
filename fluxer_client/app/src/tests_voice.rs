//! Headless GPUI tests for voice channel sidebar join, permission gating,
//! voice connection bar controls (mute/deaf/disconnect), member list display,
//! and E2EE unsupported failure rendering.

use fluxer_client_core::Dispatch;
use fluxer_client_core::state::{Snowflake, bits};
use gpui_kit::TestAppContext;
use serde_json::json;

use crate::backend::voice::{VoiceConnectionState, VoiceFailure};
use crate::backend::{Backend, UiEvent, lock};
use crate::client_view::ClientView;
use crate::tests::{draw, offline_rest, open_client, runtime};
use crate::view_model::Place;

const ME: Snowflake = Snowflake(100);
const PEER: Snowflake = Snowflake(200);
const OWNER: Snowflake = Snowflake(999);
const GUILD: Snowflake = Snowflake(1);
const VOICE_OPEN: Snowflake = Snowflake(13);
const VOICE_DENIED: Snowflake = Snowflake(14);

fn voice_ready() -> Dispatch {
    let base = (bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK).to_string();
    let deny_connect = bits::CONNECT.to_string();
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "sess-voice-ui",
            "user": { "id": ME.to_string(), "username": "me" },
            "users": [
                { "id": PEER.to_string(), "username": "peer", "global_name": "Peer User" }
            ],
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Voice Guild", "owner_id": OWNER.to_string() },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": base, "position": 0 }],
                "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }],
                "channels": [
                    { "id": VOICE_OPEN.to_string(), "guild_id": GUILD.to_string(), "type": 2, "name": "Lounge", "position": 0, "permission_overwrites": [] },
                    {
                        "id": VOICE_DENIED.to_string(),
                        "guild_id": GUILD.to_string(),
                        "type": 2,
                        "name": "Locked",
                        "position": 1,
                        "permission_overwrites": [{
                            "id": GUILD.to_string(),
                            "type": 0,
                            "allow": "0",
                            "deny": deny_connect
                        }]
                    }
                ]
            }]
        }),
    }
}

#[gpui_kit::test]
fn voice_channel_join_permission_gate_and_controls(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&voice_ready());
    let (view, vcx) = open_client(cx, backend.clone(), rx);

    view.update(vcx, |v: &mut ClientView, cx| {
        v.select_place(Place::Guild(GUILD), cx);
    });
    draw(vcx);

    // Denied voice channel sets an error and does NOT send opcode 4.
    view.update(vcx, |v, cx| v.join_voice_channel(VOICE_DENIED, cx));
    draw(vcx);
    assert!(lock(&backend.sent_voice_states).is_empty());
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.state, VoiceConnectionState::Disconnected);
        assert!(v.error.is_some());
    });

    // Allowed voice channel sends opcode 4 join and displays the voice panel.
    view.update(vcx, |v, cx| v.join_voice_channel(VOICE_OPEN, cx));
    draw(vcx);
    assert_eq!(lock(&backend.sent_voice_states).len(), 1);
    assert_eq!(lock(&backend.sent_voice_states)[0].channel_id.as_deref(), Some("13"));
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.state, VoiceConnectionState::Connecting);
        assert_eq!(v.voice.channel_id, Some(VOICE_OPEN));
        assert!(v.error.is_none());
    });

    // Remote peer joins the voice channel via VOICE_STATE_UPDATE.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_STATE_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": GUILD.to_string(),
            "channel_id": VOICE_OPEN.to_string(),
            "user_id": PEER.to_string(),
            "session_id": "peer-sess",
            "self_mute": true
        }),
    });
    view.update(vcx, |v, cx| v.refresh(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.members_in_channel(VOICE_OPEN).len(), 1);
        assert_eq!(v.voice_member_name(Some(GUILD), PEER).as_ref(), "Peer User");
    });

    // Mute, deafen, and disconnect emit updated opcode 4 frames.
    view.update(vcx, |v, cx| v.toggle_voice_mute(cx));
    view.update(vcx, |v, cx| v.toggle_voice_deaf(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.voice.self_mute);
        assert!(v.voice.self_deaf);
    });

    view.update(vcx, |v, cx| v.leave_voice_channel(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.state, VoiceConnectionState::Disconnected);
        assert_eq!(v.voice.channel_id, None);
    });
    let last = lock(&backend.sent_voice_states).last().unwrap().clone();
    assert_eq!(last.channel_id, None);
}

#[gpui_kit::test]
fn voice_e2ee_rejection_surfaces_failure_without_leaking_credentials(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&voice_ready());
    let (view, vcx) = open_client(cx, backend.clone(), rx);

    view.update(vcx, |v, cx| {
        v.select_place(Place::Guild(GUILD), cx);
        v.join_voice_channel(VOICE_OPEN, cx);
    });
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": GUILD.to_string(),
            "channel_id": VOICE_OPEN.to_string(),
            "connection_id": "conn-e2ee",
            "endpoint": "wss://rtc.example.test",
            "token": "secret-jwt-must-not-appear",
            "e2ee_key": "base64-key"
        }),
    });
    view.update(vcx, |v, cx| v.refresh(cx));
    draw(vcx);

    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.state, VoiceConnectionState::Failed(VoiceFailure::UnsupportedE2ee));
        let rendered = format!("{:?}", v.voice);
        assert!(!rendered.contains("secret-jwt-must-not-appear"));
        assert!(!rendered.contains("wss://rtc.example.test"));
    });
    let _ = UiEvent::VoiceChanged;
}
