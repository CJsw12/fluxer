//! Headless GPUI tests for voice channel sidebar join, permission gating,
//! voice connection bar controls (mute/deaf/disconnect), member list display,
//! and E2EE unsupported failure rendering.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use fluxer_client_core::Dispatch;
use fluxer_client_core::state::{Snowflake, bits};
use fluxer_client_core::voice::{DecodedVideoFrame, VideoCodec};
use gpui_kit::TestAppContext;
use serde_json::json;

use crate::backend::voice::{
    TrackSubscriptionKey, VoiceConnectionState, VoiceCredentials, VoiceFailure, VoiceRoom, VoiceTrackSource,
    VoiceTransport,
};
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

struct TestVoiceRoom {
    screen_share_active: AtomicBool,
    screen_share_audio: AtomicBool,
    subscribed_tracks: Mutex<Vec<TrackSubscriptionKey>>,
    published_tracks: Mutex<Vec<TrackSubscriptionKey>>,
    watched_video_tracks: Mutex<Vec<Option<String>>>,
}

impl TestVoiceRoom {
    fn new() -> Self {
        Self {
            screen_share_active: AtomicBool::new(false),
            screen_share_audio: AtomicBool::new(false),
            subscribed_tracks: Mutex::new(Vec::new()),
            published_tracks: Mutex::new(Vec::new()),
            watched_video_tracks: Mutex::new(Vec::new()),
        }
    }
}

impl VoiceRoom for TestVoiceRoom {
    fn set_microphone_muted(&self, _muted: bool) {}
    fn set_playback_muted(&self, _muted: bool) {}
    fn set_screen_share(&self, active: bool, with_audio: bool) {
        self.screen_share_active.store(active, Ordering::SeqCst);
        self.screen_share_audio.store(active && with_audio, Ordering::SeqCst);
    }
    fn set_watched_video_track(&self, track_sid: Option<String>) {
        self.watched_video_tracks.lock().unwrap().push(track_sid);
    }
    fn subscribed_tracks(&self) -> Vec<TrackSubscriptionKey> {
        self.subscribed_tracks.lock().unwrap().clone()
    }
    fn published_tracks(&self) -> Vec<TrackSubscriptionKey> {
        let published = self.published_tracks.lock().unwrap();
        if published.is_empty() {
            self.subscribed_tracks.lock().unwrap().clone()
        } else {
            published.clone()
        }
    }
    fn close(&self) {}
}

struct ImmediateTestTransport(Arc<TestVoiceRoom>);

impl VoiceTransport for ImmediateTestTransport {
    fn connect(
        &self,
        _credentials: VoiceCredentials,
    ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
        let room = self.0.clone();
        Box::pin(async move { Ok(room as Arc<dyn VoiceRoom>) })
    }
}

fn connect_voice_in_view(
    rt: &tokio::runtime::Runtime,
    backend: &Backend,
    view: &gpui_kit::Entity<ClientView>,
    vcx: &mut gpui_kit::VisualTestContext,
) -> Arc<TestVoiceRoom> {
    let room = Arc::new(TestVoiceRoom::new());
    backend.set_voice_transport_for_test(Arc::new(ImmediateTestTransport(room.clone())));

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
            "connection_id": "conn-ui",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-ui"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            assert!(tokio::time::Instant::now() < deadline, "voice connect timeout");
            tokio::task::yield_now().await;
        }
    });
    view.update(vcx, |v, cx| v.refresh(cx));
    draw(vcx);
    room
}

#[gpui_kit::test]
fn voice_screen_share_requires_stream_permission_in_ui(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&voice_ready());
    let (view, vcx) = open_client(cx, backend.clone(), rx);
    let room = connect_voice_in_view(&rt, &backend, &view, vcx);

    let sent_before = lock(&backend.sent_voice_states).len();
    view.update(vcx, |v, cx| v.toggle_screen_share(false, cx));
    draw(vcx);

    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.state, VoiceConnectionState::Connected);
        assert!(v.error.is_some());
        assert!(!v.voice.self_stream);
        assert!(!v.voice.screen_share_audio);
    });
    assert_eq!(lock(&backend.sent_voice_states).len(), sent_before);
    assert!(!room.screen_share_active.load(Ordering::SeqCst));
}

#[gpui_kit::test]
fn voice_screen_share_and_audio_toggle_render_voice_panel(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    let stream_perms = (bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM).to_string();
    backend.state().apply(&Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "sess-voice-stream-ui",
            "user": { "id": ME.to_string(), "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Voice Guild", "owner_id": OWNER.to_string() },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": stream_perms, "position": 0 }],
                "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }],
                "channels": [
                    { "id": VOICE_OPEN.to_string(), "guild_id": GUILD.to_string(), "type": 2, "name": "Lounge", "position": 0, "permission_overwrites": [] }
                ]
            }]
        }),
    });
    let (view, vcx) = open_client(cx, backend.clone(), rx);
    let room = connect_voice_in_view(&rt, &backend, &view, vcx);

    assert!(vcx.debug_bounds("voice-share-btn").is_some());
    assert!(vcx.debug_bounds("voice-share-audio-btn").is_none());

    // Start screen share (audio off by default).
    view.update(vcx, |v, cx| v.toggle_screen_share(false, cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.error.is_none());
        assert!(v.voice.self_stream);
        assert!(!v.voice.screen_share_audio);
    });
    assert!(room.screen_share_active.load(Ordering::SeqCst));
    assert!(!room.screen_share_audio.load(Ordering::SeqCst));
    assert!(vcx.debug_bounds("voice-share-btn").is_some());
    assert!(vcx.debug_bounds("voice-share-audio-btn").is_some());

    // Toggle screen share audio on, then back off.
    view.update(vcx, |v, cx| v.toggle_screen_share_audio(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.voice.self_stream);
        assert!(v.voice.screen_share_audio);
    });
    assert!(room.screen_share_audio.load(Ordering::SeqCst));

    view.update(vcx, |v, cx| v.toggle_screen_share_audio(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.voice.self_stream);
        assert!(!v.voice.screen_share_audio);
    });
    assert!(!room.screen_share_audio.load(Ordering::SeqCst));

    // Stop screen share.
    view.update(vcx, |v, cx| v.toggle_screen_share(false, cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(!v.voice.self_stream);
        assert!(!v.voice.screen_share_audio);
    });
    assert!(!room.screen_share_active.load(Ordering::SeqCst));
    assert!(vcx.debug_bounds("voice-share-audio-btn").is_none());
}

#[gpui_kit::test]
fn a11y_labels_and_roles_render_without_panic(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    let perms = (bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::MANAGE_CHANNELS).to_string();
    backend.state().apply(&Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "sess-a11y-ui",
            "user": { "id": ME.to_string(), "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Access Guild", "owner_id": OWNER.to_string() },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": perms, "position": 0 }],
                "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }],
                "channels": [
                    { "id": "11", "guild_id": GUILD.to_string(), "type": 4, "name": "Category", "position": 0, "permission_overwrites": [] },
                    { "id": "12", "guild_id": GUILD.to_string(), "type": 0, "name": "general", "parent_id": "11", "position": 1, "permission_overwrites": [] },
                    { "id": VOICE_OPEN.to_string(), "guild_id": GUILD.to_string(), "type": 2, "name": "Lounge", "parent_id": "11", "position": 2, "permission_overwrites": [] }
                ]
            }]
        }),
    });
    let (view, vcx) = open_client(cx, backend.clone(), rx);

    // Render Home place: guild rail, channel list (Friends + create-group-dm-btn), user panel.
    view.update(vcx, |v, cx| v.select_place(Place::Home, cx));
    draw(vcx);
    assert!(vcx.debug_bounds("join-guild").is_some());
    assert!(vcx.debug_bounds("home-friends").is_some());
    assert!(vcx.debug_bounds("create-group-dm-btn").is_some());

    // Render Guild place with active voice connection: unified user area, voice panel, text/voice channels, header buttons.
    let _room = connect_voice_in_view(&rt, &backend, &view, vcx);
    view.update(vcx, |v, cx| {
        v.select_place(Place::Guild(GUILD), cx);
        v.select_channel(Snowflake(12), cx);
    });
    draw(vcx);
    assert!(vcx.debug_bounds("create-channel-btn").is_some());
    assert!(vcx.debug_bounds("user-area-wrapper").is_some());
    assert!(vcx.debug_bounds("voice-panel").is_some());
    assert!(vcx.debug_bounds("voice-signal-icon").is_some());
    assert!(vcx.debug_bounds("voice-noise-suppression-btn").is_some());
    assert!(vcx.debug_bounds("voice-disconnect-btn").is_some());
    assert!(vcx.debug_bounds("voice-channel-source-link").is_some());
    assert!(vcx.debug_bounds("voice-camera-btn").is_some());
    assert!(vcx.debug_bounds("voice-share-btn").is_some());
    assert!(vcx.debug_bounds("user-panel-mute").is_some());
    assert!(vcx.debug_bounds("user-panel-deafen").is_some());
    assert!(vcx.debug_bounds("edit-selected-channel-btn").is_some());
    assert!(vcx.debug_bounds("toggle-search").is_some());
    assert!(vcx.debug_bounds("toggle-member-list").is_some());
}

#[gpui_kit::test]
fn voice_watch_video_track_toggles_and_renders_watch_button(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&voice_ready());
    let (view, vcx) = open_client(cx, backend.clone(), rx);
    let room = connect_voice_in_view(&rt, &backend, &view, vcx);

    assert!(vcx.debug_bounds("voice-watch-btn").is_none());

    let remote_screen = TrackSubscriptionKey {
        participant_identity: "peer:conn-peer".into(),
        participant_sid: "PA_peer".into(),
        track_sid: "TR_screen_peer".into(),
        source: VoiceTrackSource::ScreenShare,
    };
    room.subscribed_tracks.lock().unwrap().push(remote_screen.clone());
    view.update(vcx, |v, cx| v.refresh(cx));
    draw(vcx);

    assert!(vcx.debug_bounds("voice-watch-btn").is_some());

    view.update(vcx, |v, cx| v.toggle_watch_video_track(remote_screen.clone(), cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.watched_video_track, Some(remote_screen.clone()));
    });
    assert!(vcx.debug_bounds("voice-watch-btn").is_some());

    view.update(vcx, |v, cx| v.toggle_watch_video_track(remote_screen, cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.watched_video_track, None);
    });
    assert!(vcx.debug_bounds("voice-watch-btn").is_some());
}

#[gpui_kit::test]
fn voice_video_viewer_renders_waiting_frame_and_tears_down(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&voice_ready());
    let (view, vcx) = open_client(cx, backend.clone(), rx);
    let room = connect_voice_in_view(&rt, &backend, &view, vcx);

    let remote_screen = TrackSubscriptionKey {
        participant_identity: "peer:conn-peer".into(),
        participant_sid: "PA_peer".into(),
        track_sid: "TR_screen_peer".into(),
        source: VoiceTrackSource::ScreenShare,
    };
    room.subscribed_tracks.lock().unwrap().push(remote_screen.clone());
    view.update(vcx, |v, cx| v.refresh(cx));
    draw(vcx);

    assert!(vcx.debug_bounds("video-viewer").is_none());

    // Start watching: viewer mounts in waiting state (no frame yet).
    view.update(vcx, |v, cx| v.toggle_watch_video_track(remote_screen.clone(), cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.watched_video_track, Some(remote_screen.clone()));
        assert_eq!(v.watched_video_track_sid.as_deref(), Some("TR_screen_peer"));
        assert_eq!(v.watched_video_frame_seq, 0);
        assert!(v.watched_video_frame.is_none());
    });
    assert!(vcx.debug_bounds("video-viewer").is_some());
    assert!(vcx.debug_bounds("video-viewer-stop-btn").is_some());

    // Push a decoded 2x2 RGBA frame and render.
    let frame = DecodedVideoFrame {
        width: 2,
        height: 2,
        timestamp_us: 1000,
        codec: Some(VideoCodec::H264),
        data: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255],
    };
    assert!(backend.push_video_frame_for_test(frame));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.watched_video_frame_seq, 1);
        let (w, h, _) = v
            .watched_video_frame
            .as_ref()
            .expect("decoded frame should be converted to RenderImage");
        assert_eq!((*w, *h), (2, 2));
    });
    assert!(vcx.debug_bounds("video-viewer").is_some());

    // Stop Watching tears down the viewer and clears frame state.
    view.update(vcx, |v, cx| v.toggle_watch_video_track(remote_screen.clone(), cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.watched_video_track, None);
        assert_eq!(v.watched_video_track_sid, None);
        assert_eq!(v.watched_video_frame_seq, 0);
        assert!(v.watched_video_frame.is_none());
    });
    assert!(vcx.debug_bounds("video-viewer").is_none());

    // Watch again, push a frame, then disconnect voice: tears down cleanly.
    view.update(vcx, |v, cx| v.toggle_watch_video_track(remote_screen, cx));
    draw(vcx);
    let frame2 = DecodedVideoFrame {
        width: 2,
        height: 2,
        timestamp_us: 2000,
        codec: Some(VideoCodec::H264),
        data: [1u8, 2, 3, 255].repeat(4),
    };
    assert!(backend.push_video_frame_for_test(frame2));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.watched_video_frame.is_some());
    });

    view.update(vcx, |v, cx| v.leave_voice_channel(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.watched_video_track, None);
        assert_eq!(v.watched_video_track_sid, None);
        assert_eq!(v.watched_video_frame_seq, 0);
        assert!(v.watched_video_frame.is_none());
    });
    assert!(vcx.debug_bounds("video-viewer").is_none());
}

#[gpui_kit::test]
fn voice_published_unsubscribed_screen_share_renders_watch_button_and_triggers_watch(
    cx: &mut TestAppContext,
) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&voice_ready());
    let (view, vcx) = open_client(cx, backend.clone(), rx);
    let room = connect_voice_in_view(&rt, &backend, &view, vcx);

    let remote_screen = TrackSubscriptionKey {
        participant_identity: "peer:conn-peer".into(),
        participant_sid: "PA_peer".into(),
        track_sid: "TR_screen_unsub".into(),
        source: VoiceTrackSource::ScreenShare,
    };
    room.published_tracks.lock().unwrap().push(remote_screen.clone());
    view.update(vcx, |v, cx| v.refresh(cx));
    draw(vcx);

    view.read_with(vcx, |v, _| {
        assert!(v.voice.subscribed_tracks.is_empty());
        assert_eq!(v.voice.published_tracks, vec![remote_screen.clone()]);
    });
    assert!(vcx.debug_bounds("voice-watch-btn").is_some());

    view.update(vcx, |v, cx| v.toggle_watch_video_track(remote_screen.clone(), cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.watched_video_track, Some(remote_screen));
    });
    assert_eq!(
        *room.watched_video_tracks.lock().unwrap(),
        vec![Some("TR_screen_unsub".to_owned())]
    );
}

#[gpui_kit::test]
fn voice_stage_lifecycle_tiles_screen_share_controls_and_back_to_chat(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    let perms = (bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM).to_string();
    backend.state().apply(&Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "sess-voice-stage",
            "user": { "id": ME.to_string(), "username": "me", "global_name": "Me" },
            "users": [
                { "id": PEER.to_string(), "username": "peer", "global_name": "Peer User" }
            ],
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Stage Guild", "owner_id": OWNER.to_string() },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": perms, "position": 0 }],
                "members": [{ "user": { "id": ME.to_string(), "username": "me" }, "roles": [] }],
                "channels": [
                    { "id": "11", "guild_id": GUILD.to_string(), "type": 0, "name": "general", "position": 0 },
                    { "id": VOICE_OPEN.to_string(), "guild_id": GUILD.to_string(), "type": 2, "name": "Lounge", "position": 1 }
                ]
            }]
        }),
    });
    let (view, vcx) = open_client(cx, backend.clone(), rx);

    // Before joining voice, central stage is not shown.
    view.update(vcx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    draw(vcx);
    assert!(vcx.debug_bounds("voice-stage").is_none());

    // Joining voice channel connects and transitions center pane to VoiceCallView stage (audio-only wrapped avatar list initially).
    let room = connect_voice_in_view(&rt, &backend, &view, vcx);
    assert!(vcx.debug_bounds("voice-stage").is_some(), "voice stage rendered on connect");
    assert!(vcx.debug_bounds("voice-stage-grid").is_some());
    assert!(vcx.debug_bounds("voice-stage-avatar-list").is_some(), "audio-only call renders circular avatar list");
    assert!(vcx.debug_bounds("voice-stage-controls").is_some());
    assert!(vcx.debug_bounds("voice-stage-input-settings-btn").is_some());
    assert!(vcx.debug_bounds("voice-stage-output-settings-btn").is_some());
    assert!(vcx.debug_bounds("voice-stage-camera-btn").is_some());
    assert!(vcx.debug_bounds("voice-stage-share-settings-btn").is_some());
    assert!(vcx.debug_bounds("voice-stage-more-btn").is_some());
    assert!(vcx.debug_bounds("voice-stage-control-separator").is_some());
    assert!(vcx.debug_bounds("voice-stage-tile-100").is_some(), "self participant tile rendered");

    // Remote muted peer joins and publishes a screen share track.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_STATE_UPDATE".into(),
        s: 3,
        d: json!({
            "guild_id": GUILD.to_string(),
            "channel_id": VOICE_OPEN.to_string(),
            "user_id": PEER.to_string(),
            "session_id": "peer-sess",
            "self_mute": true,
            "self_stream": true
        }),
    });
    let remote_screen = TrackSubscriptionKey {
        participant_identity: format!("{}:conn-peer", PEER.0),
        participant_sid: "PA_peer".into(),
        track_sid: "TR_stage_screen".into(),
        source: VoiceTrackSource::ScreenShare,
    };
    room.published_tracks.lock().unwrap().push(remote_screen.clone());
    view.update(vcx, |v, cx| v.refresh(cx));
    draw(vcx);

    assert!(vcx.debug_bounds("voice-stage-tile-200").is_some(), "remote participant tile rendered");
    assert!(vcx.debug_bounds("voice-tile-muted-200").is_some(), "muted badge rendered on peer tile");
    assert!(vcx.debug_bounds("voice-stage-live-200").is_some(), "LIVE badge rendered on streaming peer");
    assert!(vcx.debug_bounds("voice-member-live-200").is_some(), "channel sidebar row renders LIVE badge");
    assert!(vcx.debug_bounds("voice-member-muted-200").is_some(), "channel sidebar row renders muted icon");

    // Caret buttons on grouped split controls open inline voice control popouts; bottom-left noise suppression button opens inline details popout.
    let in_caret = vcx.debug_bounds("voice-stage-input-settings-btn").unwrap().center();
    vcx.simulate_click(in_caret, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("voice-control-popout").is_some(), "input caret opens inline voice control popout");
    vcx.simulate_click(in_caret, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("voice-control-popout").is_none());

    let ns_btn = vcx.debug_bounds("voice-noise-suppression-btn").unwrap().center();
    vcx.simulate_click(ns_btn, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("voice-details-popout").is_some(), "bottom-left sliders icon opens inline voice details popout");
    let ns_btn_after = vcx.debug_bounds("voice-noise-suppression-btn").unwrap().center();
    vcx.simulate_click(ns_btn_after, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("voice-details-popout").is_none());
    let watch_btn = vcx
        .debug_bounds("voice-stage-watch-TR_stage_screen")
        .expect("stage Watch Stream button")
        .center();
    vcx.simulate_click(watch_btn, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("video-viewer").is_some(), "video viewer expanded inside stage");

    // Bottom grouped controls toggle mute, deafen, and screen share with official LocalVoiceStateMachine coupling.
    let mute_pos = vcx.debug_bounds("voice-stage-mute-btn").unwrap().center();
    vcx.simulate_click(mute_pos, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    let deaf_pos = vcx.debug_bounds("voice-stage-deaf-btn").unwrap().center();
    vcx.simulate_click(deaf_pos, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    let share_pos = vcx.debug_bounds("voice-stage-share-btn").unwrap().center();
    vcx.simulate_click(share_pos, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.voice.self_mute);
        assert!(v.voice.self_deaf);
        assert!(!v.voice.should_unmute_on_undeafen, "was muted before deafen, so undeafen keeps mute");
        assert!(v.voice.self_stream);
    });

    // Toggle split view (Top Call Surface + Bottom Text Chat) and back to full-height call view.
    let expand_pos = vcx.debug_bounds("voice-stage-expand-btn").unwrap().center();
    vcx.simulate_click(expand_pos, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("voice-stage-split-chat").is_some(), "split view renders call stage above text chat");
    let expand_pos2 = vcx.debug_bounds("voice-stage-expand-btn").unwrap().center();
    vcx.simulate_click(expand_pos2, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("voice-stage-split-chat").is_none(), "re-expanding returns to full-height stage");

    // Back to Chat hides the stage while keeping voice connected; clicking active voice channel reopens stage without extra opcode 4.
    let sent_before_reopen = lock(&backend.sent_voice_states).len();
    let back_pos = vcx.debug_bounds("voice-stage-back-btn").unwrap().center();
    vcx.simulate_click(back_pos, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("voice-stage").is_none(), "Back to Chat returns to text channel");
    assert!(vcx.debug_bounds("voice-panel").is_some(), "voice remains connected in sidebar bar");

    view.update(vcx, |v, cx| v.join_voice_channel(VOICE_OPEN, cx));
    draw(vcx);
    assert!(vcx.debug_bounds("voice-stage").is_some(), "clicking active voice channel reopens stage");
    assert_eq!(lock(&backend.sent_voice_states).len(), sent_before_reopen, "reopening active stage does not resend opcode 4");

    // Disconnect button leaves voice and closes the stage cleanly.
    let disc_pos = vcx.debug_bounds("voice-stage-disconnect-btn").unwrap().center();
    vcx.simulate_click(disc_pos, gpui_kit::Modifiers::default());
    vcx.run_until_parked();
    draw(vcx);
    assert!(vcx.debug_bounds("voice-stage").is_none(), "disconnect closes voice stage");
    view.read_with(vcx, |v, _| {
        assert_eq!(v.voice.state, VoiceConnectionState::Disconnected);
        assert_eq!(v.voice.channel_id, None);
    });
}

#[gpui_kit::test]
fn local_voice_state_machine_prevents_speaker_only_deafen_and_restores_mute(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, rx) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&voice_ready());
    let (view, vcx) = open_client(cx, backend.clone(), rx);
    let _room = connect_voice_in_view(&rt, &backend, &view, vcx);

    // 1. Starting unmuted: toggling deafen ON forces self_mute=true, self_deaf=true, should_unmute_on_undeafen=true.
    view.update(vcx, |v, cx| v.toggle_voice_deaf(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(v.voice.self_deaf);
        assert!(v.voice.self_mute, "deafening must also mute microphone");
        assert!(v.voice.should_unmute_on_undeafen);
        assert!(v.voice.effective_muted());
        assert!(v.voice.effective_deafened());
    });

    // 2. Toggling deafen OFF restores self_mute=false because user was unmuted before deafening.
    view.update(vcx, |v, cx| v.toggle_voice_deaf(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(!v.voice.self_deaf);
        assert!(!v.voice.self_mute, "undeafening restores prior unmuted state");
        assert!(!v.voice.should_unmute_on_undeafen);
    });

    // 3. Starting manually muted: toggling deafen ON -> OFF keeps self_mute=true.
    view.update(vcx, |v, cx| v.toggle_voice_mute(cx));
    view.update(vcx, |v, cx| v.toggle_voice_deaf(cx));
    view.read_with(vcx, |v, _| {
        assert!(v.voice.self_deaf);
        assert!(v.voice.self_mute);
        assert!(!v.voice.should_unmute_on_undeafen);
    });
    view.update(vcx, |v, cx| v.toggle_voice_deaf(cx));
    view.read_with(vcx, |v, _| {
        assert!(!v.voice.self_deaf);
        assert!(v.voice.self_mute, "undeafening preserves prior manual mute");
    });

    // 4. While deafened, clicking the mic mute button to unmute automatically undeafens too.
    view.update(vcx, |v, cx| v.toggle_voice_deaf(cx));
    view.read_with(vcx, |v, _| {
        assert!(v.voice.self_deaf && v.voice.self_mute);
    });
    view.update(vcx, |v, cx| v.toggle_voice_mute(cx));
    draw(vcx);
    view.read_with(vcx, |v, _| {
        assert!(!v.voice.self_mute, "unmuting while deafened unmutes mic");
        assert!(!v.voice.self_deaf, "unmuting while deafened also clears deafen");
        assert!(!v.voice.should_unmute_on_undeafen);
    });
}
