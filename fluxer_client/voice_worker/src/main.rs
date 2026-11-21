//! Isolated LiveKit voice worker process communicating over private stdio JSON-lines IPC.

use std::net::IpAddr;

use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use url::Url;

/// Commands received from `fluxer_client` over private stdin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WorkerCommand {
    Connect {
        endpoint: String,
        token: String,
        connection_id: String,
    },
    SetMicMuted {
        muted: bool,
    },
    SetPlaybackMuted {
        muted: bool,
    },
    SetScreenShare {
        active: bool,
        with_audio: bool,
    },
    SetAudioSettings {
        input_device: Option<String>,
        output_device: Option<String>,
        input_volume: u16,
        output_volume: u16,
    },
    SetWatchedVideoTrack {
        track_sid: Option<String>,
    },
    Close,
}

/// Events emitted to `fluxer_client` over private stdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WorkerEvent {
    Connected,
    Error { message: String },
    Disconnected { reason: String },
    TrackSubscribed {
        participant_identity: String,
        participant_sid: String,
        track_sid: String,
        source: String,
    },
    TrackUnsubscribed {
        participant_identity: String,
        participant_sid: String,
        track_sid: String,
        source: String,
    },
}

fn track_source_label(source: TrackSource) -> &'static str {
    match source {
        TrackSource::Microphone => "microphone",
        TrackSource::Camera => "camera",
        TrackSource::Screenshare => "screen_share",
        TrackSource::ScreenshareAudio => "screen_share_audio",
        TrackSource::Unknown => "unknown",
    }
}

fn validate_connect(endpoint: &str, token: &str, connection_id: &str) -> Result<(), &'static str> {
    if token.trim().is_empty() {
        return Err("missing voice token");
    }
    if connection_id.trim().is_empty() {
        return Err("missing connection_id");
    }
    let parsed = Url::parse(endpoint).map_err(|_| "invalid voice endpoint url")?;
    let is_loopback = parsed.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !(parsed.scheme() == "wss" || (parsed.scheme() == "ws" && is_loopback)) {
        return Err("voice endpoint requires wss (or loopback ws)");
    }
    if parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("voice endpoint contains unsupported URL fields");
    }
    Ok(())
}

fn sanitize_error(raw: &str, endpoint: &str, token: &str) -> String {
    let mut sanitized = raw.to_owned();
    if !token.is_empty() {
        sanitized = sanitized.replace(token, "<redacted>");
    }
    if !endpoint.is_empty() {
        sanitized = sanitized.replace(endpoint, "<redacted>");
    }
    sanitized
}

async fn emit_event(stdout: &mut tokio::io::Stdout, event: &WorkerEvent) {
    if let Ok(mut line) = serde_json::to_string(event) {
        line.push('\n');
        let _ = stdout.write_all(line.as_bytes()).await;
        let _ = stdout.flush().await;
    }
}

fn apply_playback_mute(room: &Room, playback_muted: bool) {
    for participant in room.remote_participants().values() {
        for publication in participant.track_publications().values() {
            if publication.source() == TrackSource::Microphone {
                publication.set_subscribed(!playback_muted);
            }
        }
    }
}

fn apply_watched_video_track(room: &Room, watched_sid: Option<&str>) {
    for participant in room.remote_participants().values() {
        for publication in participant.track_publications().values() {
            if matches!(publication.source(), TrackSource::Camera | TrackSource::Screenshare) {
                publication.set_subscribed(watched_sid == Some(publication.sid().as_str()));
            }
        }
    }
}

struct WorkerAudioState {
    input_device: Option<String>,
    output_device: Option<String>,
    #[allow(dead_code)]
    input_volume: u16,
    #[allow(dead_code)]
    output_volume: u16,
}

fn match_device_index(requested: Option<&str>, available_names: &[&str]) -> Option<usize> {
    let query = requested?.trim();
    if query.is_empty() {
        return None;
    }

    let query_lower = query.to_lowercase();
    if let Some((idx, _)) = available_names
        .iter()
        .enumerate()
        .find(|(_, name)| name.to_lowercase() == query_lower)
    {
        return Some(idx);
    }

    let mut matches = available_names
        .iter()
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(&query_lower));
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first.0)
}

fn apply_audio_settings(platform_audio: Option<&PlatformAudio>, state: &WorkerAudioState) {
    let Some(audio) = platform_audio else {
        return;
    };

    if state.input_device.is_some() {
        let devices: Vec<_> = audio.recording_devices().collect();
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        if let Some(idx) = match_device_index(state.input_device.as_deref(), &names) {
            if let Some(device) = devices.get(idx) {
                let _ = audio.switch_recording_device(&device.id);
            }
        }
    }

    if state.output_device.is_some() {
        let devices: Vec<_> = audio.playout_devices().collect();
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        if let Some(idx) = match_device_index(state.output_device.as_deref(), &names) {
            if let Some(device) = devices.get(idx) {
                let _ = audio.switch_playout_device(&device.id);
            }
        }
    }
}

async fn run_worker() -> i32 {
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    let first_line = match lines.next_line().await {
        Ok(Some(line)) => line,
        Ok(None) => return 0,
        Err(_) => {
            emit_event(
                &mut stdout,
                &WorkerEvent::Error {
                    message: "failed to read initial worker command".to_owned(),
                },
            )
            .await;
            return 1;
        }
    };

    let (endpoint, token, _connection_id) = match serde_json::from_str::<WorkerCommand>(&first_line) {
        Ok(WorkerCommand::Connect {
            endpoint,
            token,
            connection_id,
        }) => (endpoint, token, connection_id),
        Ok(_) => {
            emit_event(
                &mut stdout,
                &WorkerEvent::Error {
                    message: "expected connect as first worker command".to_owned(),
                },
            )
            .await;
            return 1;
        }
        Err(_) => {
            emit_event(
                &mut stdout,
                &WorkerEvent::Error {
                    message: "invalid connect command payload".to_owned(),
                },
            )
            .await;
            return 1;
        }
    };

    if let Err(reason) = validate_connect(&endpoint, &token, &_connection_id) {
        emit_event(
            &mut stdout,
            &WorkerEvent::Error {
                message: reason.to_owned(),
            },
        )
        .await;
        return 1;
    }

    let mut options = RoomOptions::default();
    options.auto_subscribe = false;
    options.adaptive_stream = false;
    options.dynacast = true;

    let (room, mut room_events) = match Room::connect(&endpoint, &token, options).await {
        Ok(pair) => pair,
        Err(err) => {
            emit_event(
                &mut stdout,
                &WorkerEvent::Error {
                    message: format!(
                        "livekit room connect failed: {}",
                        sanitize_error(&err.to_string(), &endpoint, &token)
                    ),
                },
            )
            .await;
            return 1;
        }
    };

    emit_event(&mut stdout, &WorkerEvent::Connected).await;

    // Best-effort microphone setup: if no capture device is available, keep
    // running so remote audio subscription/playback continues to work.
    let platform_audio = PlatformAudio::new().ok();
    let mut mic_track: Option<LocalAudioTrack> = None;
    let mut mic_pub_sid: Option<TrackSid> = None;
    if let Some(audio) = &platform_audio {
        let track = LocalAudioTrack::create_audio_track("microphone", audio.rtc_source());
        let publish_opts = TrackPublishOptions {
            source: TrackSource::Microphone,
            ..Default::default()
        };
        if let Ok(pub_handle) = room
            .local_participant()
            .publish_track(LocalTrack::Audio(track.clone()), publish_opts)
            .await
        {
            mic_pub_sid = Some(pub_handle.sid());
            mic_track = Some(track);
        }
    }

    let mut playback_muted = false;
    let mut watched_video_track_sid: Option<String> = None;
    let mut audio_state = WorkerAudioState {
        input_device: None,
        output_device: None,
        input_volume: 100,
        output_volume: 100,
    };

    loop {
        tokio::select! {
            line_res = lines.next_line() => {
                match line_res {
                    Ok(Some(line)) => {
                        let Ok(cmd) = serde_json::from_str::<WorkerCommand>(&line) else {
                            continue;
                        };
                        match cmd {
                            WorkerCommand::SetMicMuted { muted } => {
                                if let Some(track) = &mic_track {
                                    if muted {
                                        track.mute();
                                    } else {
                                        track.unmute();
                                    }
                                }
                            }
                            WorkerCommand::SetPlaybackMuted { muted } => {
                                playback_muted = muted;
                                apply_playback_mute(&room, playback_muted);
                            }
                            WorkerCommand::SetScreenShare { .. } => {}
                            WorkerCommand::SetAudioSettings {
                                input_device,
                                output_device,
                                input_volume,
                                output_volume,
                            } => {
                                audio_state.input_device = input_device.filter(|d| !d.trim().is_empty());
                                audio_state.output_device = output_device.filter(|d| !d.trim().is_empty());
                                audio_state.input_volume = input_volume.min(200);
                                audio_state.output_volume = output_volume.min(200);
                                apply_audio_settings(platform_audio.as_ref(), &audio_state);
                            }
                            WorkerCommand::SetWatchedVideoTrack { track_sid } => {
                                watched_video_track_sid = track_sid
                                    .map(|sid| sid.trim().to_owned())
                                    .filter(|sid| !sid.is_empty());
                                apply_watched_video_track(&room, watched_video_track_sid.as_deref());
                            }
                            WorkerCommand::Close => break,
                            WorkerCommand::Connect { .. } => {}
                        }
                    }
                    Ok(None) | Err(_) => break,
                }
            }
            event_opt = room_events.recv() => {
                match event_opt {
                    Some(RoomEvent::TrackPublished { publication, .. }) => {
                        if publication.source() == TrackSource::Microphone && !playback_muted {
                            publication.set_subscribed(true);
                        } else if matches!(
                            publication.source(),
                            TrackSource::Camera | TrackSource::Screenshare
                        ) && watched_video_track_sid.as_deref()
                            == Some(publication.sid().as_str())
                        {
                            publication.set_subscribed(true);
                        }
                    }
                    Some(RoomEvent::TrackSubscribed {
                        publication,
                        participant,
                        ..
                    }) => {
                        emit_event(
                            &mut stdout,
                            &WorkerEvent::TrackSubscribed {
                                participant_identity: participant.identity().to_string(),
                                participant_sid: participant.sid().to_string(),
                                track_sid: publication.sid().to_string(),
                                source: track_source_label(publication.source()).to_owned(),
                            },
                        )
                        .await;
                    }
                    Some(RoomEvent::TrackUnsubscribed {
                        publication,
                        participant,
                        ..
                    }) => {
                        emit_event(
                            &mut stdout,
                            &WorkerEvent::TrackUnsubscribed {
                                participant_identity: participant.identity().to_string(),
                                participant_sid: participant.sid().to_string(),
                                track_sid: publication.sid().to_string(),
                                source: track_source_label(publication.source()).to_owned(),
                            },
                        )
                        .await;
                    }
                    Some(RoomEvent::Disconnected { reason }) => {
                        emit_event(
                            &mut stdout,
                            &WorkerEvent::Disconnected {
                                reason: format!("{reason:?}"),
                            },
                        )
                        .await;
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
        }
    }

    if let Some(sid) = mic_pub_sid {
        let _ = room.local_participant().unpublish_track(&sid).await;
    }
    drop(mic_track);
    drop(platform_audio);
    let _ = room.close().await;
    0
}

#[tokio::main]
async fn main() {
    let code = run_worker().await;
    if code != 0 {
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_command_and_event_json_match_voice_ipc_schema() {
        let cmd = WorkerCommand::Connect {
            endpoint: "wss://rtc.example.test".to_owned(),
            token: "jwt-1".to_owned(),
            connection_id: "conn-1".to_owned(),
        };
        let json_cmd = serde_json::to_string(&cmd).unwrap();
        assert!(json_cmd.contains("\"type\":\"connect\""));
        let decoded_cmd: WorkerCommand = serde_json::from_str(&json_cmd).unwrap();
        assert_eq!(decoded_cmd, cmd);

        let audio_cmd = WorkerCommand::SetAudioSettings {
            input_device: Some("Mic 1".to_owned()),
            output_device: None,
            input_volume: 120,
            output_volume: 85,
        };
        let json_audio_cmd = serde_json::to_string(&audio_cmd).unwrap();
        assert!(json_audio_cmd.contains("\"type\":\"set_audio_settings\""));
        let decoded_audio_cmd: WorkerCommand = serde_json::from_str(&json_audio_cmd).unwrap();
        assert_eq!(decoded_audio_cmd, audio_cmd);

        let watch_some: WorkerCommand =
            serde_json::from_str("{\"type\":\"set_watched_video_track\",\"track_sid\":\"TR_cam_1\"}")
                .unwrap();
        assert_eq!(
            watch_some,
            WorkerCommand::SetWatchedVideoTrack {
                track_sid: Some("TR_cam_1".to_owned()),
            }
        );
        let watch_none: WorkerCommand =
            serde_json::from_str("{\"type\":\"set_watched_video_track\",\"track_sid\":null}")
                .unwrap();
        assert_eq!(
            watch_none,
            WorkerCommand::SetWatchedVideoTrack { track_sid: None }
        );

        for (ev, expected_type) in [
            (WorkerEvent::Connected, "\"type\":\"connected\""),
            (
                WorkerEvent::Error {
                    message: "boom".to_owned(),
                },
                "\"type\":\"error\"",
            ),
            (
                WorkerEvent::Disconnected {
                    reason: "ClientInitiated".to_owned(),
                },
                "\"type\":\"disconnected\"",
            ),
            (
                WorkerEvent::TrackSubscribed {
                    participant_identity: "user-1".to_owned(),
                    participant_sid: "PA_1".to_owned(),
                    track_sid: "TR_1".to_owned(),
                    source: "microphone".to_owned(),
                },
                "\"type\":\"track_subscribed\"",
            ),
            (
                WorkerEvent::TrackUnsubscribed {
                    participant_identity: "user-1".to_owned(),
                    participant_sid: "PA_1".to_owned(),
                    track_sid: "TR_1".to_owned(),
                    source: "screen_share_audio".to_owned(),
                },
                "\"type\":\"track_unsubscribed\"",
            ),
        ] {
            let encoded = serde_json::to_string(&ev).unwrap();
            assert!(encoded.contains(expected_type));
            let decoded: WorkerEvent = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, ev);
        }
    }

    #[test]
    fn validate_connect_rejects_unsafe_urls_and_empty_secrets_without_leaking() {
        let secret_token = "super-secret-jwt-token";
        for bad_endpoint in [
            "ws://rtc.example.test",
            "https://rtc.example.test",
            "wss://user:secret@rtc.example.test",
            "wss://rtc.example.test?token=secret",
            "wss://rtc.example.test#frag",
        ] {
            let err = validate_connect(bad_endpoint, secret_token, "conn-1")
                .err()
                .expect("unsafe endpoint must be rejected");
            assert!(!err.contains("secret"));
            assert!(!err.contains(secret_token));
        }

        assert!(validate_connect("wss://rtc.example.test", "", "conn-1").is_err());
        assert!(validate_connect("wss://rtc.example.test", secret_token, "   ").is_err());

        for ok_endpoint in [
            "wss://rtc.example.test/rtc",
            "ws://127.0.0.1:7880",
            "ws://[::1]:7880",
            "ws://localhost:7880",
        ] {
            assert!(validate_connect(ok_endpoint, secret_token, "conn-1").is_ok());
        }
    }

    #[test]
    fn match_device_index_handles_exact_substring_and_ambiguous_queries() {
        let devices = ["Built-in Mic", "USB Headset Mic", "USB Headset Pro"];

        assert_eq!(match_device_index(None, &devices), None);
        assert_eq!(match_device_index(Some(""), &devices), None);
        assert_eq!(match_device_index(Some("   "), &devices), None);
        assert_eq!(match_device_index(Some("Mic"), &[]), None);

        // Exact case-insensitive match (with surrounding whitespace trimmed).
        assert_eq!(
            match_device_index(Some("  usb headset mic "), &devices),
            Some(1)
        );

        // Exact match takes precedence over substring matches.
        let exact_vs_sub = ["Mic", "Mic (USB)"];
        assert_eq!(match_device_index(Some("mic"), &exact_vs_sub), Some(0));

        // Unique case-insensitive substring fallback.
        assert_eq!(match_device_index(Some("built-in"), &devices), Some(0));
        assert_eq!(match_device_index(Some("pro"), &devices), Some(2));

        // Ambiguous or missing substring returns None.
        assert_eq!(match_device_index(Some("usb headset"), &devices), None);
        assert_eq!(match_device_index(Some("mic"), &devices), None);
        assert_eq!(match_device_index(Some("nonexistent"), &devices), None);
    }
}
