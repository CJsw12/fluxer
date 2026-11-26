use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{ClientIdentity, Endpoints, RestClient, RestConfig};
use serde_json::json;
use tokio::sync::Notify;

use super::*;

struct FakeRoom {
    mic_muted: AtomicBool,
    playback_muted: AtomicBool,
    screen_share_active: AtomicBool,
    screen_share_audio: AtomicBool,
    audio_settings: Mutex<Vec<VoiceAudioSettings>>,
    watched_video_tracks: Mutex<Vec<Option<String>>>,
    closed: AtomicBool,
    close_count: AtomicUsize,
}

impl FakeRoom {
    fn new() -> Self {
        Self {
            mic_muted: AtomicBool::new(false),
            playback_muted: AtomicBool::new(false),
            screen_share_active: AtomicBool::new(false),
            screen_share_audio: AtomicBool::new(false),
            audio_settings: Mutex::new(Vec::new()),
            watched_video_tracks: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
            close_count: AtomicUsize::new(0),
        }
    }
}

impl VoiceRoom for FakeRoom {
    fn set_microphone_muted(&self, muted: bool) {
        self.mic_muted.store(muted, Ordering::SeqCst);
    }

    fn set_playback_muted(&self, muted: bool) {
        self.playback_muted.store(muted, Ordering::SeqCst);
    }

    fn set_screen_share(&self, active: bool, with_audio: bool) {
        self.screen_share_active.store(active, Ordering::SeqCst);
        self.screen_share_audio.store(active && with_audio, Ordering::SeqCst);
    }

    fn set_audio_settings(&self, settings: &VoiceAudioSettings) {
        self.audio_settings.lock().unwrap().push(settings.clone());
    }

    fn set_watched_video_track(&self, track_sid: Option<String>) {
        self.watched_video_tracks.lock().unwrap().push(track_sid);
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.close_count.fetch_add(1, Ordering::SeqCst);
    }
}

struct FakeTransport {
    rooms: Mutex<Vec<Arc<FakeRoom>>>,
    gate: Option<Arc<Notify>>,
}

impl FakeTransport {
    fn immediate() -> (Arc<Self>, Arc<FakeRoom>) {
        let room = Arc::new(FakeRoom::new());
        let transport = Arc::new(Self { rooms: Mutex::new(vec![room.clone()]), gate: None });
        (transport, room)
    }

    fn gated() -> (Arc<Self>, Arc<FakeRoom>, Arc<Notify>) {
        let room = Arc::new(FakeRoom::new());
        let gate = Arc::new(Notify::new());
        let transport = Arc::new(Self {
            rooms: Mutex::new(vec![room.clone()]),
            gate: Some(gate.clone()),
        });
        (transport, room, gate)
    }
}

impl VoiceTransport for FakeTransport {
    fn connect(
        &self,
        _credentials: VoiceCredentials,
    ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
        let room = self.rooms.lock().unwrap().remove(0);
        let gate = self.gate.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            Ok(room as Arc<dyn VoiceRoom>)
        })
    }
}

fn test_backend(rt: &tokio::runtime::Runtime) -> (Backend, mpsc::UnboundedReceiver<UiEvent>) {
    let endpoints = Endpoints {
        api_code_version: 1,
        api: "http://127.0.0.1:9/api".into(),
        gateway: "ws://127.0.0.1:9/gateway".into(),
        media: None,
        static_cdn: None,
        upload_relay: None,
        invite: None,
        webapp: None,
        discovery_url: "http://127.0.0.1:9/.well-known/fluxer".into(),
        document: json!({}),
    };
    let mut config = RestConfig::new(&endpoints, ClientIdentity::default());
    config.policy = fluxer_client_core::rest::TransportPolicy { allow_insecure_loopback: true };
    let rest = Arc::new(RestClient::new(config).unwrap());
    Backend::new(rt.handle().clone(), rest, None)
}

#[test]
fn voice_credentials_debug_redacts_token_and_endpoint() {
    let creds = VoiceCredentials {
        attempt_id: 1,
        guild_id: Some(Snowflake(42)),
        channel_id: Snowflake(100),
        connection_id: "conn-1".into(),
        endpoint: "wss://secret-rtc.example.test".into(),
        token: SecretToken::new("super-secret-livekit-jwt"),
        initial_mic_muted: true,
        initial_playback_muted: false,
    };
    let rendered = format!("{creds:?}");
    assert!(!rendered.contains("super-secret-livekit-jwt"));
    assert!(!rendered.contains("secret-rtc.example.test"));
    assert!(rendered.contains("<redacted>"));
}

#[test]
fn endpoint_validation_rejects_unsafe_urls_and_empty_ice_servers() {
    for bad_endpoint in [
        "ws://rtc.example.test",
        "http://rtc.example.test",
        "wss://user:secret@rtc.example.test",
        "wss://rtc.example.test?token=secret",
        "wss://rtc.example.test#frag",
    ] {
        let err = parse_voice_server_update(&json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-1",
            "endpoint": bad_endpoint,
            "token": "jwt-secret"
        }))
        .err()
        .expect("bad endpoint must be rejected");
        assert!(!err.contains("secret"));
    }

    // Loopback ws/http and wss/https are accepted and normalized to ws/wss.
    for (input, expected) in [
        ("wss://rtc.example.test/rtc", "wss://rtc.example.test/rtc"),
        ("https://rtc.example.test/rtc", "wss://rtc.example.test/rtc"),
        ("ws://127.0.0.1:7880", "ws://127.0.0.1:7880/"),
        ("http://127.0.0.1:7880", "ws://127.0.0.1:7880/"),
        ("ws://[::1]:7880", "ws://[::1]:7880/"),
    ] {
        let parsed = parse_voice_server_update(&json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-1",
            "endpoint": input,
            "token": "jwt-1"
        }))
        .expect("valid endpoint must be accepted");
        let ParsedVoiceServerUpdate::Sfu { endpoint, .. } = parsed else {
            panic!("expected SFU update");
        };
        assert_eq!(endpoint, expected);
    }

    // Empty ice_servers array `[]` is truthy in JS and must be flagged as P2P mesh.
    let parsed = parse_voice_server_update(&json!({
        "guild_id": null,
        "channel_id": "200",
        "connection_id": "conn-p2p-empty",
        "endpoint": "wss://rtc.example.test",
        "token": "jwt-1",
        "ice_servers": []
    }))
    .unwrap();
    assert!(matches!(parsed, ParsedVoiceServerUpdate::UnsupportedP2pMesh { .. }));
}

#[test]
fn join_server_update_connect_mute_deaf_and_leave_lifecycle() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let (transport, room) = FakeTransport::immediate();
    backend.set_voice_transport_for_test(transport);

    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connecting);
    assert_eq!(lock(&backend.sent_voice_states).len(), 1);
    assert_eq!(lock(&backend.sent_voice_states)[0].channel_id.as_deref(), Some("100"));

    // Mismatched guild is ignored.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "999",
            "channel_id": "100",
            "connection_id": "conn-wrong",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-wrong"
        }),
    });
    assert_eq!(backend.voice_snapshot().connection_id, None);

    // Matching server update connects the transport.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-1",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-secret-1"
        }),
    });

    rt.block_on(async {
        for _ in 0..10 {
            if backend.voice_snapshot().state == VoiceConnectionState::Connected {
                break;
            }
            let _ = rx.recv().await;
        }
    });
    let snap = backend.voice_snapshot();
    assert_eq!(snap.state, VoiceConnectionState::Connected);
    assert_eq!(snap.connection_id.as_deref(), Some("conn-1"));

    // Attach owned mic + playback tasks keyed by participant/track/source.
    let (mic_dropped_tx, mic_dropped_rx) = tokio::sync::oneshot::channel::<()>();
    let mic_task = rt.spawn(async move {
        struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }
        let _guard = DropSignal(Some(mic_dropped_tx));
        std::future::pending::<()>().await;
    });
    assert!(lock(&backend.voice).attach_mic_task(snap.attempt_id, mic_task));

    let key = TrackSubscriptionKey {
        participant_identity: "user-2:conn-2".into(),
        participant_sid: "PA_2".into(),
        track_sid: "TR_audio_2".into(),
        source: VoiceTrackSource::Microphone,
    };
    let (playback_dropped_tx, playback_dropped_rx) = tokio::sync::oneshot::channel::<()>();
    let playback_task = rt.spawn(async move {
        struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }
        let _guard = DropSignal(Some(playback_dropped_tx));
        std::future::pending::<()>().await;
    });
    assert!(lock(&backend.voice).attach_playback_task(snap.attempt_id, key.clone(), playback_task));
    assert_eq!(backend.voice_snapshot().subscribed_tracks, vec![key.clone()]);

    // Verify explicit per-track detach works too, then re-attach for leave verification.
    let extra_key = TrackSubscriptionKey {
        participant_identity: "user-3:conn-3".into(),
        participant_sid: "PA_3".into(),
        track_sid: "TR_screen_3".into(),
        source: VoiceTrackSource::ScreenShareAudio,
    };
    let extra_task = rt.spawn(std::future::pending::<()>());
    assert!(lock(&backend.voice).attach_playback_task(snap.attempt_id, extra_key.clone(), extra_task));
    assert!(lock(&backend.voice).detach_playback_task(&extra_key));
    assert_eq!(backend.voice_snapshot().subscribed_tracks, vec![key]);

    backend.set_voice_mute(true);
    assert!(room.mic_muted.load(Ordering::SeqCst));
    assert!(!room.playback_muted.load(Ordering::SeqCst));

    backend.set_voice_deaf(true);
    assert!(room.playback_muted.load(Ordering::SeqCst));

    // Leave stops mic + playback tasks, closes room, and sends channel_id: null.
    backend.leave_voice();
    rt.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(2), mic_dropped_rx)
            .await
            .expect("mic task dropped")
            .expect("mic drop signal");
        tokio::time::timeout(std::time::Duration::from_secs(2), playback_dropped_rx)
            .await
            .expect("playback task dropped")
            .expect("playback drop signal");
    });
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    assert!(backend.voice_snapshot().subscribed_tracks.is_empty());
    assert!(room.closed.load(Ordering::SeqCst));
    let last_sent = lock(&backend.sent_voice_states).last().unwrap().clone();
    assert_eq!(last_sent.channel_id, None);
    assert_eq!(last_sent.guild_id.as_deref(), Some("42"));
    assert_eq!(last_sent.connection_id.as_deref(), Some("conn-1"));
}

#[test]
fn e2ee_and_p2p_mesh_updates_are_explicitly_rejected() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, _rx) = test_backend(&rt);

    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-e2ee",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-e2ee",
            "e2ee_key": "base64-key"
        }),
    });
    assert_eq!(
        backend.voice_snapshot().state,
        VoiceConnectionState::Failed(VoiceFailure::UnsupportedE2ee)
    );

    backend.join_voice(None, Snowflake(200));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": null,
            "channel_id": "200",
            "connection_id": "conn-p2p",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-p2p",
            "ice_servers": [{"urls": "stun:stun.example.test:3478"}]
        }),
    });
    assert_eq!(
        backend.voice_snapshot().state,
        VoiceConnectionState::Failed(VoiceFailure::UnsupportedP2pMesh)
    );
}

#[test]
fn late_room_completion_after_leave_is_closed_immediately() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, _rx) = test_backend(&rt);
    let (transport, room, gate) = FakeTransport::gated();
    backend.set_voice_transport_for_test(transport);

    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-late",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-late"
        }),
    });

    // User leaves while transport.connect is still in flight; then connect resolves.
    let prev_attempt = backend.voice_snapshot().attempt_id;
    let stale_ok = lock(&backend.voice).finish_connect(
        prev_attempt.wrapping_sub(1),
        Some(Snowflake(42)),
        Snowflake(100),
        "conn-late",
        Ok(room.clone()),
    );
    assert!(!stale_ok);
    assert!(room.closed.load(Ordering::SeqCst));

    backend.leave_voice();
    gate.notify_one();
    rt.block_on(tokio::task::yield_now());
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
}

#[test]
fn voice_state_dispatch_tracks_members_and_server_disconnect() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let (transport, room) = FakeTransport::immediate();
    backend.set_voice_transport_for_test(transport);

    // Seed current user id = 10 and gateway session id = "sess-1".
    backend.state().apply(&Dispatch {
        t: "READY".into(),
        s: 1,
        d: json!({
            "session_id": "sess-1",
            "user": { "id": "10", "username": "me" },
            "guilds": []
        }),
    });
    on_gateway_ready("sess-1", &backend.voice, None, &backend.events);

    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-1",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-1"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv())
                .await
                .expect("connected event timeout")
                .expect("event channel open");
        }
    });
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connected);

    // Another member joins the same channel.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_STATE_UPDATE".into(),
        s: 3,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "user_id": "20",
            "session_id": "sess-2",
            "self_mute": false,
            "self_deaf": false
        }),
    });
    assert_eq!(backend.voice_snapshot().members_in_channel(Snowflake(100)).len(), 1);

    // Server disconnects our own user from the voice channel.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_STATE_UPDATE".into(),
        s: 4,
        d: json!({
            "guild_id": "42",
            "channel_id": null,
            "user_id": "10",
            "session_id": "sess-1",
            "connection_id": "conn-1"
        }),
    });
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    assert!(room.closed.load(Ordering::SeqCst));
}

#[test]
fn same_channel_join_is_serialized_and_duplicate_server_update_is_ignored() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let room1 = Arc::new(FakeRoom::new());
    let room2 = Arc::new(FakeRoom::new());
    let connect_calls = Arc::new(AtomicUsize::new(0));
    struct CountingTransport {
        rooms: Mutex<Vec<Arc<FakeRoom>>>,
        calls: Arc<AtomicUsize>,
    }
    impl VoiceTransport for CountingTransport {
        fn connect(
            &self,
            _credentials: VoiceCredentials,
        ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let room = self.rooms.lock().unwrap().remove(0);
            Box::pin(async move { Ok(room as Arc<dyn VoiceRoom>) })
        }
    }
    backend.set_voice_transport_for_test(Arc::new(CountingTransport {
        rooms: Mutex::new(vec![room1.clone(), room2.clone()]),
        calls: connect_calls.clone(),
    }));

    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    let first_attempt = backend.voice_snapshot().attempt_id;
    // Repeated join to the same channel while connecting is serialized (no-op).
    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    assert_eq!(backend.voice_snapshot().attempt_id, first_attempt);
    assert_eq!(lock(&backend.sent_voice_states).len(), 1);

    let server_update = Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-dup",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-1"
        }),
    };
    backend.feed_voice_dispatch_for_test(&server_update);
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);

    // Duplicate VOICE_SERVER_UPDATE with identical connection_id and endpoint is ignored.
    backend.feed_voice_dispatch_for_test(&server_update);
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert!(!room1.closed.load(Ordering::SeqCst));

    // Repeated join while already Connected is also a no-op.
    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    assert_eq!(backend.voice_snapshot().attempt_id, first_attempt);
    assert_eq!(lock(&backend.sent_voice_states).len(), 1);
}

#[test]
fn out_of_order_server_update_after_channel_move_is_ignored_and_old_room_closed() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let room_a = Arc::new(FakeRoom::new());
    let room_b = Arc::new(FakeRoom::new());
    let transport = Arc::new(FakeTransport {
        rooms: Mutex::new(vec![room_a.clone(), room_b.clone()]),
        gate: None,
    });
    backend.set_voice_transport_for_test(transport);

    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-a",
            "endpoint": "wss://rtc-a.example.test",
            "token": "jwt-a"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });

    // Move to channel 200 -> closes room_a immediately.
    backend.join_voice(Some(Snowflake(42)), Snowflake(200));
    assert!(room_a.closed.load(Ordering::SeqCst));
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connecting);

    // Delayed / out-of-order VOICE_SERVER_UPDATE for channel 100 is ignored while connecting to 200.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-a-delayed",
            "endpoint": "wss://rtc-a.example.test",
            "token": "jwt-a-delayed"
        }),
    });
    assert_eq!(backend.voice_snapshot().channel_id, Some(Snowflake(200)));
    assert_eq!(backend.voice_snapshot().connection_id, None);

    // Now channel 200 server update arrives and connects room_b.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 3,
        d: json!({
            "guild_id": "42",
            "channel_id": "200",
            "connection_id": "conn-b",
            "endpoint": "wss://rtc-b.example.test",
            "token": "jwt-b"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });
    assert_eq!(backend.voice_snapshot().connection_id.as_deref(), Some("conn-b"));
    assert!(!room_b.closed.load(Ordering::SeqCst));
}

#[test]
fn ready_recovers_voice_and_prunes_ghosts_while_invalid_session_resets() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let (transport, room) = FakeTransport::immediate();
    backend.set_voice_transport_for_test(transport);

    on_gateway_ready("sess-1", &backend.voice, None, &backend.events);
    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-1",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-1"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });

    // Ghost participant present before reconnect.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_STATE_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "user_id": "99",
            "session_id": "ghost-sess"
        }),
    });
    assert_eq!(backend.voice_snapshot().members_in_channel(Snowflake(100)).len(), 1);

    // Fresh READY with a new session prunes ghosts, closes old room, and enters Reconnecting.
    on_gateway_ready("sess-2", &backend.voice, None, &backend.events);
    assert!(room.closed.load(Ordering::SeqCst));
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Reconnecting);
    assert!(backend.voice_snapshot().members_in_channel(Snowflake(100)).is_empty());

    // Hydrating READY guild voice_states populates only live members.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "READY".into(),
        s: 1,
        d: json!({
            "session_id": "sess-2",
            "guilds": [{
                "id": "42",
                "voice_states": [{
                    "channel_id": "100",
                    "user_id": "55",
                    "session_id": "live-sess"
                }]
            }]
        }),
    });
    let members = backend.voice_snapshot().members_in_channel(Snowflake(100));
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].user_id, Snowflake(55));

    // Non-resumable INVALID_SESSION clears voice state and members.
    on_gateway_reset(&backend.voice, &backend.events);
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    assert!(backend.voice_snapshot().channel_members.is_empty());
}

#[test]
fn permission_revocation_and_channel_delete_close_room_and_stop_tasks() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let (transport, room) = FakeTransport::immediate();
    backend.set_voice_transport_for_test(transport);

    // Seed guild 42, non-owner user 10, @everyone role 42 with VIEW_CHANNEL | CONNECT, voice channel 100.
    let allow_view_connect = (bits::VIEW_CHANNEL | bits::CONNECT).to_string();
    let ready = Dispatch {
        t: "READY".into(),
        s: 1,
        d: json!({
            "session_id": "sess-1",
            "user": { "id": "10", "username": "me" },
            "guilds": [{
                "id": "42",
                "properties": { "name": "Test", "owner_id": "1" },
                "roles": [{ "id": "42", "name": "@everyone", "permissions": allow_view_connect, "position": 0 }],
                "members": [{ "user": { "id": "10", "username": "me" }, "roles": [] }],
                "channels": [{ "id": "100", "guild_id": "42", "type": 2, "name": "Voice", "permission_overwrites": [] }]
            }]
        }),
    };
    backend.state().apply(&ready);
    on_gateway_ready("sess-1", &backend.voice, None, &backend.events);

    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-perm",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-perm"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connected);

    // Revoke CONNECT permission via CHANNEL_UPDATE overwrite on @everyone.
    let deny_connect = bits::CONNECT.to_string();
    let channel_update = Dispatch {
        t: "CHANNEL_UPDATE".into(),
        s: 3,
        d: json!({
            "id": "100",
            "guild_id": "42",
            "type": 2,
            "name": "Voice",
            "permission_overwrites": [{
                "id": "42",
                "type": 0,
                "allow": "0",
                "deny": deny_connect
            }]
        }),
    };
    backend.state().apply(&channel_update);
    backend.feed_voice_dispatch_for_test(&channel_update);

    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    assert!(room.closed.load(Ordering::SeqCst));
}

#[test]
fn leave_voice_and_wait_awaits_task_shutdown() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let (transport, room) = FakeTransport::immediate();
    backend.set_voice_transport_for_test(transport);

    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-wait",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-wait"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });

    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let attempt_id = backend.voice_snapshot().attempt_id;
    let task = rt.spawn(async move {
        struct Guard(Arc<AtomicBool>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let _g = Guard(flag);
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    rt.block_on(started_rx).unwrap();
    assert!(lock(&backend.voice).attach_mic_task(attempt_id, task));

    rt.block_on(backend.leave_voice_and_wait());
    assert!(dropped.load(Ordering::SeqCst));
    assert!(room.closed.load(Ordering::SeqCst));
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
}

fn seed_guild_voice_channel_and_connect(
    rt: &tokio::runtime::Runtime,
    backend: &Backend,
    rx: &mut mpsc::UnboundedReceiver<UiEvent>,
    permissions: u64,
) -> Arc<FakeRoom> {
    let (transport, room) = FakeTransport::immediate();
    backend.set_voice_transport_for_test(transport);

    let allow = permissions.to_string();
    let ready = Dispatch {
        t: "READY".into(),
        s: 1,
        d: json!({
            "session_id": "sess-1",
            "user": { "id": "10", "username": "me" },
            "guilds": [{
                "id": "42",
                "properties": { "name": "Test", "owner_id": "1" },
                "roles": [{ "id": "42", "name": "@everyone", "permissions": allow, "position": 0 }],
                "members": [{ "user": { "id": "10", "username": "me" }, "roles": [] }],
                "channels": [{ "id": "100", "guild_id": "42", "type": 2, "name": "Voice", "permission_overwrites": [] }]
            }]
        }),
    };
    backend.state().apply(&ready);
    on_gateway_ready("sess-1", &backend.voice, None, &backend.events);

    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-perm",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-perm"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });
    room
}

fn spawn_drop_flag_task(
    rt: &tokio::runtime::Runtime,
) -> (JoinHandle<()>, Arc<AtomicBool>, tokio::sync::oneshot::Receiver<()>) {
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel::<()>();
    let task = rt.spawn(async move {
        struct Guard(Arc<AtomicBool>, Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
                if let Some(tx) = self.1.take() {
                    let _ = tx.send(());
                }
            }
        }
        let _g = Guard(flag, Some(dropped_tx));
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    rt.block_on(started_rx).unwrap();
    (task, dropped, dropped_rx)
}

fn await_task_drop(rt: &tokio::runtime::Runtime, rx: tokio::sync::oneshot::Receiver<()>) {
    rt.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .expect("task drop timeout")
            .expect("task drop signal");
    });
}

#[test]
fn start_screen_share_requires_stream_permission_and_sends_no_opcode4() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
    );

    let sent_before = lock(&backend.sent_voice_states).len();
    assert!(backend.start_screen_share(true).is_err());

    let snap = backend.voice_snapshot();
    assert!(!snap.self_stream);
    assert!(!snap.screen_share_audio);
    assert!(!room.screen_share_active.load(Ordering::SeqCst));
    assert!(!room.screen_share_audio.load(Ordering::SeqCst));
    assert_eq!(lock(&backend.sent_voice_states).len(), sent_before);
}

#[test]
fn screen_share_separates_video_and_audio_and_prevents_stale_audio() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM,
    );
    let attempt_id = backend.voice_snapshot().attempt_id;

    // Stale attempt_id must abort tasks immediately and return false.
    let (stale_v, stale_v_dropped, stale_v_rx) = spawn_drop_flag_task(&rt);
    let (stale_a, stale_a_dropped, stale_a_rx) = spawn_drop_flag_task(&rt);
    assert!(!lock(&backend.voice).attach_screen_share_tasks(
        attempt_id.wrapping_sub(1),
        Some(stale_v),
        Some(stale_a),
    ));
    await_task_drop(&rt, stale_v_rx);
    await_task_drop(&rt, stale_a_rx);
    assert!(stale_v_dropped.load(Ordering::SeqCst));
    assert!(stale_a_dropped.load(Ordering::SeqCst));

    // Start with audio enabled.
    assert!(backend.start_screen_share(true).is_ok());
    let snap = backend.voice_snapshot();
    assert!(snap.self_stream);
    assert!(snap.screen_share_audio);
    assert!(room.screen_share_active.load(Ordering::SeqCst));
    assert!(room.screen_share_audio.load(Ordering::SeqCst));
    assert!(lock(&backend.sent_voice_states).last().unwrap().self_stream);

    let (video_task, video_dropped, video_rx) = spawn_drop_flag_task(&rt);
    let (audio_task, audio_dropped, audio_rx) = spawn_drop_flag_task(&rt);
    assert!(lock(&backend.voice).attach_screen_share_tasks(
        attempt_id,
        Some(video_task),
        Some(audio_task),
    ));

    // Switching to include_audio == false aborts the audio task while keeping video alive.
    assert!(backend.start_screen_share(false).is_ok());
    await_task_drop(&rt, audio_rx);
    assert!(audio_dropped.load(Ordering::SeqCst));
    assert!(!video_dropped.load(Ordering::SeqCst));
    let snap = backend.voice_snapshot();
    assert!(snap.self_stream);
    assert!(!snap.screen_share_audio);
    assert!(room.screen_share_active.load(Ordering::SeqCst));
    assert!(!room.screen_share_audio.load(Ordering::SeqCst));

    // Stopping screen share aborts video too and sends self_stream: false.
    backend.stop_screen_share();
    await_task_drop(&rt, video_rx);
    assert!(video_dropped.load(Ordering::SeqCst));
    let snap = backend.voice_snapshot();
    assert!(!snap.self_stream);
    assert!(!snap.screen_share_audio);
    assert!(!room.screen_share_active.load(Ordering::SeqCst));
    assert!(!lock(&backend.sent_voice_states).last().unwrap().self_stream);
}

#[test]
fn revoking_stream_permission_stops_screen_share_only_and_keeps_voice() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM,
    );
    let attempt_id = backend.voice_snapshot().attempt_id;

    let (mic_task, mic_dropped, _mic_rx) = spawn_drop_flag_task(&rt);
    assert!(lock(&backend.voice).attach_mic_task(attempt_id, mic_task));

    assert!(backend.start_screen_share(true).is_ok());
    let (video_task, video_dropped, video_rx) = spawn_drop_flag_task(&rt);
    let (audio_task, audio_dropped, audio_rx) = spawn_drop_flag_task(&rt);
    assert!(lock(&backend.voice).attach_screen_share_tasks(
        attempt_id,
        Some(video_task),
        Some(audio_task),
    ));

    let deny_stream = bits::STREAM.to_string();
    let channel_update = Dispatch {
        t: "CHANNEL_UPDATE".into(),
        s: 3,
        d: json!({
            "id": "100",
            "guild_id": "42",
            "type": 2,
            "name": "Voice",
            "permission_overwrites": [{
                "id": "42",
                "type": 0,
                "allow": "0",
                "deny": deny_stream
            }]
        }),
    };
    backend.state().apply(&channel_update);
    backend.feed_voice_dispatch_for_test(&channel_update);

    await_task_drop(&rt, video_rx);
    await_task_drop(&rt, audio_rx);

    assert!(video_dropped.load(Ordering::SeqCst));
    assert!(audio_dropped.load(Ordering::SeqCst));
    assert!(!mic_dropped.load(Ordering::SeqCst));

    let snap = backend.voice_snapshot();
    assert_eq!(snap.state, VoiceConnectionState::Connected);
    assert!(!snap.self_stream);
    assert!(!snap.screen_share_audio);
    assert!(!room.closed.load(Ordering::SeqCst));
    assert!(!room.screen_share_active.load(Ordering::SeqCst));
    assert!(!room.screen_share_audio.load(Ordering::SeqCst));

    let last_sent = lock(&backend.sent_voice_states).last().unwrap().clone();
    assert_eq!(last_sent.channel_id.as_deref(), Some("100"));
    assert!(!last_sent.self_stream);
}

#[test]
fn revoking_and_restoring_speak_permission_mutes_mic_without_leaving() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
    );

    assert!(!backend.voice_snapshot().speak_denied);
    assert!(!room.mic_muted.load(Ordering::SeqCst));

    let deny_speak = bits::SPEAK.to_string();
    let revoke_update = Dispatch {
        t: "CHANNEL_UPDATE".into(),
        s: 3,
        d: json!({
            "id": "100",
            "guild_id": "42",
            "type": 2,
            "name": "Voice",
            "permission_overwrites": [{
                "id": "42",
                "type": 0,
                "allow": "0",
                "deny": deny_speak
            }]
        }),
    };
    backend.state().apply(&revoke_update);
    backend.feed_voice_dispatch_for_test(&revoke_update);

    let snap = backend.voice_snapshot();
    assert_eq!(snap.state, VoiceConnectionState::Connected);
    assert!(snap.speak_denied);
    assert!(room.mic_muted.load(Ordering::SeqCst));
    assert!(!room.closed.load(Ordering::SeqCst));

    let restore_update = Dispatch {
        t: "CHANNEL_UPDATE".into(),
        s: 4,
        d: json!({
            "id": "100",
            "guild_id": "42",
            "type": 2,
            "name": "Voice",
            "permission_overwrites": []
        }),
    };
    backend.state().apply(&restore_update);
    backend.feed_voice_dispatch_for_test(&restore_update);

    let snap = backend.voice_snapshot();
    assert_eq!(snap.state, VoiceConnectionState::Connected);
    assert!(!snap.speak_denied);
    assert!(!room.mic_muted.load(Ordering::SeqCst));
}

#[test]
fn leave_voice_and_wait_awaits_mic_screen_share_and_playback_tasks() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM,
    );
    let attempt_id = backend.voice_snapshot().attempt_id;

    let (mic_task, mic_dropped, _mic_rx) = spawn_drop_flag_task(&rt);
    let (video_task, video_dropped, _video_rx) = spawn_drop_flag_task(&rt);
    let (audio_task, audio_dropped, _audio_rx) = spawn_drop_flag_task(&rt);
    let (playback_task, playback_dropped, _playback_rx) = spawn_drop_flag_task(&rt);

    let key = TrackSubscriptionKey {
        participant_identity: "user-2:conn-2".into(),
        participant_sid: "PA_2".into(),
        track_sid: "TR_audio_2".into(),
        source: VoiceTrackSource::Microphone,
    };
    assert!(lock(&backend.voice).attach_mic_task(attempt_id, mic_task));
    assert!(lock(&backend.voice).attach_screen_share_tasks(
        attempt_id,
        Some(video_task),
        Some(audio_task),
    ));
    assert!(lock(&backend.voice).attach_playback_task(attempt_id, key, playback_task));

    rt.block_on(backend.leave_voice_and_wait());

    assert!(mic_dropped.load(Ordering::SeqCst));
    assert!(video_dropped.load(Ordering::SeqCst));
    assert!(audio_dropped.load(Ordering::SeqCst));
    assert!(playback_dropped.load(Ordering::SeqCst));
    assert!(room.closed.load(Ordering::SeqCst));
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
}

#[test]
fn room_disconnected_event_fails_connected_session_unless_already_left() {
    struct EventRoom {
        event_rx: Mutex<Option<mpsc::UnboundedReceiver<VoiceRoomEvent>>>,
        closed: AtomicBool,
    }
    impl VoiceRoom for EventRoom {
        fn set_microphone_muted(&self, _muted: bool) {}
        fn set_playback_muted(&self, _muted: bool) {}
        fn set_screen_share(&self, _active: bool, _with_audio: bool) {}
        fn take_event_rx(&self) -> Option<mpsc::UnboundedReceiver<VoiceRoomEvent>> {
            self.event_rx.lock().unwrap().take()
        }
        fn close(&self) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }
    struct EventTransport {
        rooms: Mutex<Vec<Arc<EventRoom>>>,
    }
    impl VoiceTransport for EventTransport {
        fn connect(
            &self,
            _credentials: VoiceCredentials,
        ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
            let room = self.rooms.lock().unwrap().remove(0);
            Box::pin(async move { Ok(room as Arc<dyn VoiceRoom>) })
        }
    }

    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);

    let (ev_tx1, ev_rx1) = mpsc::unbounded_channel();
    let room1 = Arc::new(EventRoom {
        event_rx: Mutex::new(Some(ev_rx1)),
        closed: AtomicBool::new(false),
    });
    let (ev_tx2, ev_rx2) = mpsc::unbounded_channel();
    let room2 = Arc::new(EventRoom {
        event_rx: Mutex::new(Some(ev_rx2)),
        closed: AtomicBool::new(false),
    });
    backend.set_voice_transport_for_test(Arc::new(EventTransport {
        rooms: Mutex::new(vec![room1.clone(), room2.clone()]),
    }));

    // 1. Disconnected event while Connected transitions to Failed(TransportFailed) and emits VoiceChanged.
    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-ev-1",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-ev-1"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });

    assert!(backend.start_screen_share(true).is_ok());
    while rx.try_recv().is_ok() {}

    ev_tx1
        .send(VoiceRoomEvent::Disconnected("worker crashed".to_owned()))
        .unwrap();
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
            if matches!(
                backend.voice_snapshot().state,
                VoiceConnectionState::Failed(VoiceFailure::TransportFailed(_))
            ) {
                break;
            }
        }
    });
    let snap = backend.voice_snapshot();
    assert_eq!(
        snap.state,
        VoiceConnectionState::Failed(VoiceFailure::TransportFailed("worker crashed".to_owned()))
    );
    assert!(!snap.self_stream);
    assert!(!snap.screen_share_audio);
    assert!(room1.closed.load(Ordering::SeqCst));

    // 2. Late Disconnected event after leave_voice() is ignored.
    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(101)));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "101",
            "connection_id": "conn-ev-2",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-ev-2"
        }),
    });
    rt.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while backend.voice_snapshot().state != VoiceConnectionState::Connected {
            tokio::time::timeout_at(deadline, rx.recv()).await.unwrap().unwrap();
        }
    });
    backend.leave_voice();
    let _ = ev_tx2.send(VoiceRoomEvent::Disconnected("late disconnect".to_owned()));
    rt.block_on(tokio::task::yield_now());
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
}

#[test]
fn screen_share_codec_tracker_tracks_stages_and_resets_on_stop_and_leave() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let _room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK | bits::STREAM,
    );

    assert!(backend.start_screen_share(true).is_ok());

    backend.set_screen_share_requested_codec(VideoCodec::H265);
    assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::H264), Ok(true));
    assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::H264), Ok(false));
    backend.record_rendered_screen_share_codec(VideoCodec::H264);

    let snap = backend.voice_snapshot();
    assert_eq!(snap.screen_share_codec.requested, Some(VideoCodec::H265));
    assert_eq!(snap.screen_share_codec.negotiated, Some(VideoCodec::H264));
    assert_eq!(snap.screen_share_codec.rendered, Some(VideoCodec::H264));
    assert_eq!(snap.screen_share_codec.republish_count, 0);

    // Exhaust republish budget (MAX_CODEC_REPUBLISH_ATTEMPTS = 3).
    assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::Vp9), Ok(true));
    assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::Vp8), Ok(true));
    assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::Av1), Ok(true));
    assert!(backend.apply_negotiated_screen_share_codec(VideoCodec::H264).is_err());

    // Stopping screen share resets codec tracker.
    backend.stop_screen_share();
    assert_eq!(
        backend.voice_snapshot().screen_share_codec,
        ScreenShareCodecTracker::default()
    );

    // Leaving voice also resets codec tracker.
    assert!(backend.start_screen_share(false).is_ok());
    backend.set_screen_share_requested_codec(VideoCodec::Av1);
    assert_eq!(backend.apply_negotiated_screen_share_codec(VideoCodec::Vp8), Ok(true));
    backend.record_rendered_screen_share_codec(VideoCodec::Vp8);
    backend.leave_voice();
    assert_eq!(
        backend.voice_snapshot().screen_share_codec,
        ScreenShareCodecTracker::default()
    );
}

#[test]
fn set_voice_audio_settings_applies_on_connect_and_updates_active_room() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);

    // Pre-connect settings are normalized and retained until room connects.
    backend.set_voice_audio_settings(VoiceAudioSettings {
        input_device: Some("   ".to_owned()),
        output_device: Some("Headset".to_owned()),
        input_volume: 250,
        output_volume: 300,
    });

    let room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
    );
    let expected_initial = VoiceAudioSettings {
        input_device: None,
        output_device: Some("Headset".to_owned()),
        input_volume: 200,
        output_volume: 200,
    };
    assert_eq!(*room.audio_settings.lock().unwrap(), vec![expected_initial.clone()]);

    // Live update while connected immediately propagates to the active room.
    backend.set_voice_audio_settings(VoiceAudioSettings {
        input_device: Some("USB Mic".to_owned()),
        output_device: Some(String::new()),
        input_volume: 75,
        output_volume: 150,
    });
    let expected_updated = VoiceAudioSettings {
        input_device: Some("USB Mic".to_owned()),
        output_device: None,
        input_volume: 75,
        output_volume: 150,
    };
    assert_eq!(
        *room.audio_settings.lock().unwrap(),
        vec![expected_initial, expected_updated]
    );
}

#[test]
fn watch_video_track_validates_source_replaces_queue_and_clears_on_stop_or_leave() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);

    let screen_key = TrackSubscriptionKey {
        participant_identity: "user-2:conn-2".into(),
        participant_sid: "PA_2".into(),
        track_sid: "TR_screen_2".into(),
        source: VoiceTrackSource::ScreenShare,
    };
    assert!(backend.watch_video_track(screen_key.clone()).is_err());

    let room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
    );

    let mic_key = TrackSubscriptionKey {
        participant_identity: "user-2:conn-2".into(),
        participant_sid: "PA_2".into(),
        track_sid: "TR_mic_2".into(),
        source: VoiceTrackSource::Microphone,
    };
    let screen_audio_key = TrackSubscriptionKey {
        participant_identity: "user-2:conn-2".into(),
        participant_sid: "PA_2".into(),
        track_sid: "TR_sa_2".into(),
        source: VoiceTrackSource::ScreenShareAudio,
    };
    assert!(backend.watch_video_track(mic_key).is_err());
    assert!(backend.watch_video_track(screen_audio_key).is_err());
    assert_eq!(backend.voice_snapshot().watched_video_track, None);

    let queue = backend
        .watch_video_track(screen_key.clone())
        .expect("screen share track should be watchable");
    assert_eq!(backend.voice_snapshot().watched_video_track, Some(screen_key));

    let make_frame = |ts: i64, byte: u8| DecodedVideoFrame {
        width: 1280,
        height: 720,
        timestamp_us: ts,
        codec: Some(VideoCodec::H264),
        data: vec![byte],
    };
    assert!(queue.push(make_frame(100, 1)));
    assert!(queue.push(make_frame(200, 2)));
    assert_eq!(queue.dropped_frames(), 1);
    let (gen_id, latest) = backend
        .latest_video_frame(0)
        .expect("latest frame should be available");
    assert_eq!(gen_id, 2);
    assert_eq!(latest.timestamp_us, 200);
    assert_eq!(latest.data, vec![2]);
    assert!(backend.latest_video_frame(gen_id).is_none());

    backend.stop_watching_video();
    assert_eq!(backend.voice_snapshot().watched_video_track, None);
    assert!(!queue.push(make_frame(300, 3)));
    assert!(backend.latest_video_frame(0).is_none());

    let camera_key = TrackSubscriptionKey {
        participant_identity: "user-3:conn-3".into(),
        participant_sid: "PA_3".into(),
        track_sid: "TR_cam_3".into(),
        source: VoiceTrackSource::Camera,
    };
    let cam_queue = backend
        .watch_video_track(camera_key.clone())
        .expect("camera track should be watchable");
    assert_eq!(backend.voice_snapshot().watched_video_track, Some(camera_key));
    assert!(cam_queue.push(make_frame(400, 4)));

    backend.leave_voice();
    assert_eq!(backend.voice_snapshot().watched_video_track, None);
    assert!(!cam_queue.push(make_frame(500, 5)));
    assert!(backend.latest_video_frame(0).is_none());
    assert_eq!(
        *room.watched_video_tracks.lock().unwrap(),
        vec![
            Some("TR_screen_2".to_owned()),
            None,
            Some("TR_cam_3".to_owned()),
            None,
        ]
    );
}

#[test]
fn handle_room_event_pushes_watched_video_frame_and_ignores_unwatched() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let _room = seed_guild_voice_channel_and_connect(
        &rt,
        &backend,
        &mut rx,
        bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK,
    );
    let watched_key = TrackSubscriptionKey {
        participant_identity: "user-2:conn-2".into(),
        participant_sid: "PA_2".into(),
        track_sid: "TR_cam_watched".into(),
        source: VoiceTrackSource::Camera,
    };
    let _queue = backend
        .watch_video_track(watched_key)
        .expect("camera track should be watchable");

    {
        let mut mgr = lock(&backend.voice);
        assert!(!mgr.handle_room_event(VoiceRoomEvent::VideoFrame {
            track_sid: "TR_cam_other".to_owned(),
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 255, 4, 5, 6, 255],
        }));
    }
    assert!(backend.latest_video_frame(0).is_none());

    let expected_rgba = vec![9, 8, 7, 255, 6, 5, 4, 255];
    {
        let mut mgr = lock(&backend.voice);
        assert!(mgr.handle_room_event(VoiceRoomEvent::VideoFrame {
            track_sid: "TR_cam_watched".to_owned(),
            width: 2,
            height: 1,
            rgba: expected_rgba.clone(),
        }));
    }
    let (_gen_id, frame) = backend
        .latest_video_frame(0)
        .expect("watched video frame should update latest_video_frame");
    assert_eq!(frame.width, 2);
    assert_eq!(frame.height, 1);
    assert_eq!(frame.data, expected_rgba);
}

#[test]
fn published_unsubscribed_screen_share_can_be_watched_and_clears_on_unpublish() {
    struct PublishedOnlyRoom {
        published: Mutex<Vec<TrackSubscriptionKey>>,
        watched: Mutex<Vec<Option<String>>>,
        event_rx: Mutex<Option<mpsc::UnboundedReceiver<VoiceRoomEvent>>>,
    }

    impl VoiceRoom for PublishedOnlyRoom {
        fn set_microphone_muted(&self, _muted: bool) {}
        fn set_playback_muted(&self, _muted: bool) {}
        fn set_screen_share(&self, _active: bool, _with_audio: bool) {}
        fn set_watched_video_track(&self, track_sid: Option<String>) {
            self.watched.lock().unwrap().push(track_sid);
        }
        fn published_tracks(&self) -> Vec<TrackSubscriptionKey> {
            self.published.lock().unwrap().clone()
        }
        fn take_event_rx(&self) -> Option<mpsc::UnboundedReceiver<VoiceRoomEvent>> {
            self.event_rx.lock().unwrap().take()
        }
        fn close(&self) {}
    }

    struct PublishedOnlyTransport(Arc<PublishedOnlyRoom>);
    impl VoiceTransport for PublishedOnlyTransport {
        fn connect(
            &self,
            _credentials: VoiceCredentials,
        ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
            let room = self.0.clone();
            Box::pin(async move { Ok(room as Arc<dyn VoiceRoom>) })
        }
    }

    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let (ev_tx, ev_rx) = mpsc::unbounded_channel();
    let screen_key = TrackSubscriptionKey {
        participant_identity: "user-2:conn-2".into(),
        participant_sid: "PA_2".into(),
        track_sid: "TR_screen_unsub".into(),
        source: VoiceTrackSource::ScreenShare,
    };
    let room = Arc::new(PublishedOnlyRoom {
        published: Mutex::new(vec![screen_key.clone()]),
        watched: Mutex::new(Vec::new()),
        event_rx: Mutex::new(Some(ev_rx)),
    });
    backend.set_voice_transport_for_test(Arc::new(PublishedOnlyTransport(room.clone())));

    let mut state = backend.state();
    state.apply(&Dispatch {
        s: 1,
        t: "READY".into(),
        d: json!({
            "user": { "id": "1", "username": "me" },
            "guilds": [{
                "id": "10",
                "name": "Guild",
                "owner_id": "1",
                "channels": [{ "id": "100", "type": 2, "name": "Voice" }],
                "roles": [{ "id": "10", "name": "@everyone", "permissions": (bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK).to_string() }]
            }]
        }),
    });
    assert!(backend.join_voice(Some(Snowflake(10)), Snowflake(100)));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        s: 2,
        t: "VOICE_SERVER_UPDATE".into(),
        d: json!({
            "guild_id": "10",
            "channel_id": "100",
            "endpoint": "wss://rtc.example.test",
            "token": "tok",
            "connection_id": "conn-1"
        }),
    });
    rt.block_on(async {
        for _ in 0..20 {
            if backend.voice_snapshot().state == VoiceConnectionState::Connected {
                break;
            }
            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        }
    });
    let snap = backend.voice_snapshot();
    assert_eq!(snap.state, VoiceConnectionState::Connected);
    assert_eq!(snap.published_tracks, vec![screen_key.clone()]);
    assert!(snap.subscribed_tracks.is_empty());

    let queue = backend
        .watch_video_track(screen_key.clone())
        .expect("published-but-unsubscribed screen share must be watchable");
    assert_eq!(backend.voice_snapshot().watched_video_track, Some(screen_key));
    assert_eq!(
        *room.watched.lock().unwrap(),
        vec![Some("TR_screen_unsub".to_owned())]
    );

    // Unpublish the watched track and emit TracksChanged.
    room.published.lock().unwrap().clear();
    ev_tx.send(VoiceRoomEvent::TracksChanged).unwrap();
    rt.block_on(async {
        for _ in 0..20 {
            if backend.voice_snapshot().watched_video_track.is_none() {
                break;
            }
            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        }
    });
    assert_eq!(backend.voice_snapshot().watched_video_track, None);
    assert!(backend.voice_snapshot().published_tracks.is_empty());
    assert!(!queue.push(DecodedVideoFrame {
        width: 2,
        height: 2,
        timestamp_us: 1,
        codec: None,
        data: vec![0; 16],
    }));
}

#[test]
fn endpoint_change_with_same_connection_id_bumps_attempt_id_and_rejects_stale_connect() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, _rx) = test_backend(&rt);
    let (_transport, _room, _gate) = FakeTransport::gated();
    backend.set_voice_transport_for_test(_transport);

    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-same",
            "endpoint": "wss://rtc-1.example.test",
            "token": "jwt-1"
        }),
    });
    let attempt_1 = backend.voice_snapshot().attempt_id;

    // Replace endpoint while keeping the exact same connection_id.
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-same",
            "endpoint": "wss://rtc-2.example.test",
            "token": "jwt-2"
        }),
    });
    let attempt_2 = backend.voice_snapshot().attempt_id;
    assert!(attempt_2 > attempt_1, "endpoint replacement must bump attempt_id");

    let stale_room = Arc::new(FakeRoom::new());
    let accepted = lock(&backend.voice).finish_connect(
        attempt_1,
        Some(Snowflake(42)),
        Snowflake(100),
        "conn-same",
        Ok(stale_room.clone()),
    );
    assert!(!accepted);
    assert!(stale_room.closed.load(Ordering::SeqCst));
    assert_eq!(backend.voice_snapshot().attempt_id, attempt_2);
}

#[test]
fn failed_same_channel_reconnect_closes_previous_active_room() {
    struct FailingTransport;
    impl VoiceTransport for FailingTransport {
        fn connect(
            &self,
            _credentials: VoiceCredentials,
        ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
            Box::pin(async { Err("reconnect failed".to_owned()) })
        }
    }

    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, mut rx) = test_backend(&rt);
    let (transport, old_room) = FakeTransport::immediate();
    backend.set_voice_transport_for_test(transport);

    backend.join_voice(Some(Snowflake(42)), Snowflake(100));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-1",
            "endpoint": "wss://rtc-1.example.test",
            "token": "jwt-1"
        }),
    });
    rt.block_on(async {
        for _ in 0..20 {
            if backend.voice_snapshot().state == VoiceConnectionState::Connected {
                break;
            }
            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        }
    });
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Connected);

    let attempt_id = backend.voice_snapshot().attempt_id;
    let mic_task = rt.spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    });
    assert!(lock(&backend.voice).attach_mic_task(attempt_id, mic_task));
    let watch_key = TrackSubscriptionKey {
        participant_identity: "user-2".into(),
        participant_sid: "PA_2".into(),
        track_sid: "TR_cam_1".into(),
        source: VoiceTrackSource::Camera,
    };
    let queue = backend.watch_video_track(watch_key).unwrap();

    // Now switch to a failing transport and trigger a same-channel reconnect.
    backend.set_voice_transport_for_test(Arc::new(FailingTransport));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "channel_id": "100",
            "connection_id": "conn-1",
            "endpoint": "wss://rtc-2.example.test",
            "token": "jwt-2"
        }),
    });
    rt.block_on(async {
        for _ in 0..20 {
            if matches!(backend.voice_snapshot().state, VoiceConnectionState::Failed(_)) {
                break;
            }
            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        }
    });
    assert_eq!(
        backend.voice_snapshot().state,
        VoiceConnectionState::Failed(VoiceFailure::TransportFailed("reconnect failed".into()))
    );
    assert!(old_room.closed.load(Ordering::SeqCst));
    assert_eq!(backend.voice_snapshot().watched_video_track, None);
    assert!(!queue.push(DecodedVideoFrame {
        width: 2,
        height: 2,
        timestamp_us: 1,
        codec: None,
        data: vec![0; 16],
    }));
}

#[test]
fn view_channel_revocation_does_not_disconnect_newer_attempt() {
    let mut mgr = VoiceManager::new(Arc::new(UnconfiguredVoiceTransport));
    let _ = mgr.begin_join(Some(Snowflake(42)), Snowflake(100));
    let old_attempt = mgr.attempt_id;

    // Move to a newer attempt on channel 200.
    let _ = mgr.begin_join(Some(Snowflake(42)), Snowflake(200));
    let new_attempt = mgr.attempt_id;
    assert!(new_attempt > old_attempt);

    let (payload, handles) = mgr.begin_leave_if_attempt(old_attempt);
    assert!(payload.is_none());
    assert!(handles.is_empty());
    assert_eq!(mgr.channel_id, Some(Snowflake(200)));
    assert_eq!(mgr.state, VoiceConnectionState::Connecting);
}

#[test]
fn guild_update_and_bulk_role_channel_updates_trigger_voice_permission_revocation() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, _rx) = test_backend(&rt);

    let allow_all = (bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK).to_string();
    let seed = Dispatch {
        t: "READY".into(),
        s: 1,
        d: json!({
            "user": { "id": "10", "username": "tester" },
            "guilds": [{
                "id": "42",
                "name": "Guild",
                "owner_id": "1",
                "channels": [{ "id": "100", "type": 2, "name": "Voice" }],
                "roles": [{ "id": "42", "name": "@everyone", "permissions": allow_all }],
                "members": [{ "user": { "id": "10", "username": "tester" }, "roles": [] }]
            }]
        }),
    };
    backend.state().apply(&seed);

    // 1. GUILD_ROLE_UPDATE_BULK revoking CONNECT leaves voice.
    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    let bulk_role = Dispatch {
        t: "GUILD_ROLE_UPDATE_BULK".into(),
        s: 2,
        d: json!({
            "guild_id": "42",
            "roles": [{ "id": "42", "name": "@everyone", "permissions": bits::VIEW_CHANNEL.to_string() }]
        }),
    };
    backend.state().apply(&bulk_role);
    backend.feed_voice_dispatch_for_test(&bulk_role);
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);

    // 2. Restore role, rejoin, then CHANNEL_UPDATE_BULK denying VIEW_CHANNEL leaves voice.
    let restore_role = Dispatch {
        t: "GUILD_ROLE_UPDATE".into(),
        s: 3,
        d: json!({
            "guild_id": "42",
            "role": { "id": "42", "name": "@everyone", "permissions": allow_all }
        }),
    };
    backend.state().apply(&restore_role);
    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    let bulk_channel = Dispatch {
        t: "CHANNEL_UPDATE_BULK".into(),
        s: 4,
        d: json!({
            "guild_id": "42",
            "channels": [{
                "id": "100",
                "guild_id": "42",
                "type": 2,
                "name": "Voice",
                "permission_overwrites": [{
                    "id": "42",
                    "type": 0,
                    "allow": "0",
                    "deny": bits::VIEW_CHANNEL.to_string()
                }]
            }]
        }),
    };
    backend.state().apply(&bulk_channel);
    backend.feed_voice_dispatch_for_test(&bulk_channel);
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);

    // 3. Make user 10 owner with 0 role perms, rejoin, then GUILD_UPDATE transferring ownership leaves voice.
    let reset_channel = Dispatch {
        t: "CHANNEL_UPDATE".into(),
        s: 5,
        d: json!({
            "id": "100",
            "guild_id": "42",
            "type": 2,
            "name": "Voice",
            "permission_overwrites": []
        }),
    };
    let zero_role = Dispatch {
        t: "GUILD_ROLE_UPDATE".into(),
        s: 6,
        d: json!({
            "guild_id": "42",
            "role": { "id": "42", "name": "@everyone", "permissions": "0" }
        }),
    };
    let make_owner = Dispatch {
        t: "GUILD_UPDATE".into(),
        s: 7,
        d: json!({
            "id": "42",
            "name": "Guild",
            "owner_id": "10"
        }),
    };
    backend.state().apply(&reset_channel);
    backend.state().apply(&zero_role);
    backend.state().apply(&make_owner);

    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    let transfer_owner = Dispatch {
        t: "GUILD_UPDATE".into(),
        s: 8,
        d: json!({
            "id": "42",
            "name": "Guild",
            "owner_id": "99"
        }),
    };
    backend.state().apply(&transfer_owner);
    backend.feed_voice_dispatch_for_test(&transfer_owner);
    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
}

#[test]
fn p2p_update_without_sfu_credentials_fails_immediately_and_null_ice_servers_with_p2p_true_is_rejected() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, _rx) = test_backend(&rt);

    // 1. p2p: true without endpoint/token/connection_id must fail immediately.
    backend.join_voice(None, Snowflake(200));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 1,
        d: json!({
            "guild_id": null,
            "channel_id": "200",
            "p2p": true
        }),
    });
    assert_eq!(
        backend.voice_snapshot().state,
        VoiceConnectionState::Failed(VoiceFailure::UnsupportedP2pMesh)
    );

    // 2. Non-null ice_servers without endpoint/token/connection_id must fail immediately.
    backend.join_voice(None, Snowflake(201));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 2,
        d: json!({
            "guild_id": null,
            "channel_id": "201",
            "ice_servers": [{"urls": "stun:stun.example.test:3478"}]
        }),
    });
    assert_eq!(
        backend.voice_snapshot().state,
        VoiceConnectionState::Failed(VoiceFailure::UnsupportedP2pMesh)
    );

    // 3. p2p: true + ice_servers: null + SFU credentials must still be rejected as UnsupportedP2pMesh.
    backend.join_voice(None, Snowflake(202));
    backend.feed_voice_dispatch_for_test(&Dispatch {
        t: "VOICE_SERVER_UPDATE".into(),
        s: 3,
        d: json!({
            "guild_id": null,
            "channel_id": "202",
            "p2p": true,
            "ice_servers": null,
            "connection_id": "conn-p2p",
            "endpoint": "wss://rtc.example.test",
            "token": "jwt-p2p"
        }),
    });
    assert_eq!(
        backend.voice_snapshot().state,
        VoiceConnectionState::Failed(VoiceFailure::UnsupportedP2pMesh)
    );
}

#[test]
fn guild_refresh_removing_active_voice_channel_leaves_voice() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (backend, _rx) = test_backend(&rt);

    let allow_all = (bits::VIEW_CHANNEL | bits::CONNECT | bits::SPEAK).to_string();
    let ready = Dispatch {
        t: "READY".into(),
        s: 1,
        d: json!({
            "user": { "id": "10", "username": "me" },
            "guilds": [{
                "id": "42",
                "name": "Guild",
                "owner_id": "1",
                "roles": [{ "id": "42", "name": "@everyone", "permissions": allow_all }],
                "channels": [
                    { "id": "100", "guild_id": "42", "type": 2, "name": "Voice" },
                    { "id": "101", "guild_id": "42", "type": 2, "name": "Other" }
                ],
                "members": [{ "user": { "id": "10", "username": "me" }, "roles": [] }]
            }]
        }),
    };
    backend.state().apply(&ready);

    assert!(backend.join_voice(Some(Snowflake(42)), Snowflake(100)));
    assert_eq!(backend.voice_snapshot().channel_id, Some(Snowflake(100)));

    // Guild refresh (GUILD_CREATE) omits channel 100.
    let guild_refresh = Dispatch {
        t: "GUILD_CREATE".into(),
        s: 2,
        d: json!({
            "id": "42",
            "name": "Guild",
            "owner_id": "1",
            "roles": [{ "id": "42", "name": "@everyone", "permissions": allow_all }],
            "channels": [
                { "id": "101", "guild_id": "42", "type": 2, "name": "Other" }
            ],
            "members": [{ "user": { "id": "10", "username": "me" }, "roles": [] }],
            "voice_states": []
        }),
    };
    backend.state().apply(&guild_refresh);
    backend.feed_voice_dispatch_for_test(&guild_refresh);

    assert_eq!(backend.voice_snapshot().state, VoiceConnectionState::Disconnected);
    assert_eq!(backend.voice_snapshot().channel_id, None);
}
