//! Isolated LiveKit voice worker process communicating over private stdio JSON-lines IPC.

use std::fmt;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::StreamExt as _;
use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use livekit::webrtc::video_frame::VideoFormatType;
use livekit::webrtc::video_stream::native::{NativeVideoStream, NativeVideoStreamOptions};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use url::Url;

const MAX_VIDEO_WIDTH: u32 = 960;
const MAX_VIDEO_HEIGHT: u32 = 540;
const VIDEO_FRAME_MIN_INTERVAL: Duration = Duration::from_millis(66);
const MAX_WORKER_COMMAND_LINE_BYTES: usize = 128 * 1024;

fn default_true() -> bool {
    true
}

/// Commands received from `fluxer_client` over private stdin.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WorkerCommand {
    Connect {
        endpoint: String,
        token: String,
        connection_id: String,
        #[serde(default = "default_true")]
        initial_mic_muted: bool,
        #[serde(default)]
        initial_playback_muted: bool,
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

impl fmt::Debug for WorkerCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect {
                connection_id,
                initial_mic_muted,
                initial_playback_muted,
                ..
            } => f
                .debug_struct("Connect")
                .field("endpoint", &"<redacted>")
                .field("token", &"<redacted>")
                .field("connection_id", connection_id)
                .field("initial_mic_muted", initial_mic_muted)
                .field("initial_playback_muted", initial_playback_muted)
                .finish(),
            Self::SetMicMuted { muted } => {
                f.debug_struct("SetMicMuted").field("muted", muted).finish()
            }
            Self::SetPlaybackMuted { muted } => f
                .debug_struct("SetPlaybackMuted")
                .field("muted", muted)
                .finish(),
            Self::SetScreenShare { active, with_audio } => f
                .debug_struct("SetScreenShare")
                .field("active", active)
                .field("with_audio", with_audio)
                .finish(),
            Self::SetAudioSettings {
                input_device,
                output_device,
                input_volume,
                output_volume,
            } => f
                .debug_struct("SetAudioSettings")
                .field("input_device", input_device)
                .field("output_device", output_device)
                .field("input_volume", input_volume)
                .field("output_volume", output_volume)
                .finish(),
            Self::SetWatchedVideoTrack { track_sid } => f
                .debug_struct("SetWatchedVideoTrack")
                .field("track_sid", track_sid)
                .finish(),
            Self::Close => f.write_str("Close"),
        }
    }
}

/// Events emitted to `fluxer_client` over private stdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WorkerEvent {
    Connected,
    Error { message: String },
    Disconnected { reason: String },
    TrackPublished {
        participant_identity: String,
        participant_sid: String,
        track_sid: String,
        source: String,
    },
    TrackUnpublished {
        participant_identity: String,
        participant_sid: String,
        track_sid: String,
        source: String,
    },
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
    VideoFrame {
        track_sid: String,
        width: u32,
        height: u32,
        rgba_b64: String,
    },
}

fn compute_scaled_dimensions(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> Option<(u32, u32)> {
    if src_w == 0 || src_h == 0 || max_w == 0 || max_h == 0 {
        return None;
    }
    if src_w <= max_w && src_h <= max_h {
        return Some((src_w, src_h));
    }
    let scale = (max_w as f64 / src_w as f64).min(max_h as f64 / src_h as f64);
    let dst_w = ((src_w as f64 * scale).round() as u32).clamp(1, max_w);
    let dst_h = ((src_h as f64 * scale).round() as u32).clamp(1, max_h);
    Some((dst_w, dst_h))
}

fn abort_video_task(active_video_task: &mut Option<(String, tokio::task::JoinHandle<()>)>) {
    if let Some((_, handle)) = active_video_task.take() {
        handle.abort();
    }
}

fn spawn_video_decode_task(
    track: livekit::track::RemoteVideoTrack,
    track_sid: String,
    video_frame_tx: mpsc::Sender<WorkerEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut stream = NativeVideoStream::with_options(
            track.rtc_track(),
            NativeVideoStreamOptions {
                queue_size_frames: Some(1),
            },
        );
        let mut rgba = Vec::new();
        let mut last_emitted: Option<Instant> = None;
        while let Some(frame) = stream.next().await {
            let now = Instant::now();
            if let Some(prev) = last_emitted
                && now.duration_since(prev) < VIDEO_FRAME_MIN_INTERVAL
            {
                continue;
            }
            let src_w = frame.buffer.width();
            let src_h = frame.buffer.height();
            let Some((dst_w, dst_h)) =
                compute_scaled_dimensions(src_w, src_h, MAX_VIDEO_WIDTH, MAX_VIDEO_HEIGHT)
            else {
                continue;
            };
            let Some(byte_len) = (dst_w as usize)
                .checked_mul(dst_h as usize)
                .and_then(|px| px.checked_mul(4))
            else {
                continue;
            };
            let Some(stride) = dst_w.checked_mul(4) else {
                continue;
            };
            let (Ok(dst_w_i32), Ok(dst_h_i32)) = (i32::try_from(dst_w), i32::try_from(dst_h))
            else {
                continue;
            };
            rgba.resize(byte_len, 0);
            frame.buffer.to_argb(
                VideoFormatType::RGBA,
                &mut rgba,
                stride,
                dst_w_i32,
                dst_h_i32,
            );
            let rgba_b64 = base64::engine::general_purpose::STANDARD.encode(&rgba);
            last_emitted = Some(now);
            let _ = video_frame_tx.try_send(WorkerEvent::VideoFrame {
                track_sid: track_sid.clone(),
                width: dst_w,
                height: dst_h,
                rgba_b64,
            });
        }
    })
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

fn format_room_connect_error(err: &RoomError) -> &'static str {
    match err {
        RoomError::Engine(_) => "livekit room connect failed: engine error",
        RoomError::Internal(_) => "livekit room connect failed: internal error",
        RoomError::Rtc(_) => "livekit room connect failed: rtc error",
        RoomError::TrackAlreadyPublished => {
            "livekit room connect failed: track already published"
        }
        RoomError::AlreadyClosed => "livekit room connect failed: already closed",
        RoomError::Request { .. } => "livekit room connect failed: request error",
    }
}

async fn read_line_limited<R>(
    reader: &mut BufReader<R>,
    buf: &mut Vec<u8>,
    max_bytes: usize,
) -> std::io::Result<Option<String>>
where
    R: AsyncRead + Unpin,
{
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if buf.is_empty() {
                return Ok(None);
            }
            break;
        }
        if let Some(newline_pos) = available.iter().position(|&b| b == b'\n') {
            let needed = buf.len().saturating_add(newline_pos);
            if needed > max_bytes {
                reader.consume(newline_pos + 1);
                buf.clear();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "ipc line exceeds maximum length",
                ));
            }
            buf.extend_from_slice(&available[..newline_pos]);
            reader.consume(newline_pos + 1);
            break;
        }
        let needed = buf.len().saturating_add(available.len());
        if needed > max_bytes {
            let len = available.len();
            reader.consume(len);
            buf.clear();
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ipc line exceeds maximum length",
            ));
        }
        buf.extend_from_slice(available);
        let len = available.len();
        reader.consume(len);
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    let line_bytes = std::mem::take(buf);
    String::from_utf8(line_bytes).map(Some).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ipc line is not valid utf-8",
        )
    })
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
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut stdin_line_buf = Vec::new();

    let first_line =
        match read_line_limited(&mut reader, &mut stdin_line_buf, MAX_WORKER_COMMAND_LINE_BYTES)
            .await
        {
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

    let (endpoint, token, _connection_id, _initial_mic_muted, initial_playback_muted) =
        match serde_json::from_str::<WorkerCommand>(&first_line) {
            Ok(WorkerCommand::Connect {
                endpoint,
                token,
                connection_id,
                initial_mic_muted,
                initial_playback_muted,
            }) => (
                endpoint,
                token,
                connection_id,
                initial_mic_muted,
                initial_playback_muted,
            ),
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
                    message: format_room_connect_error(&err).to_owned(),
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
        // Fail closed: always publish muted and only unmute on explicit parent command.
        track.mute();
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

    let mut playback_muted = initial_playback_muted;
    apply_playback_mute(&room, playback_muted);
    for participant in room.remote_participants().values() {
        for publication in participant.track_publications().values() {
            emit_event(
                &mut stdout,
                &WorkerEvent::TrackPublished {
                    participant_identity: participant.identity().to_string(),
                    participant_sid: participant.sid().to_string(),
                    track_sid: publication.sid().to_string(),
                    source: track_source_label(publication.source()).to_owned(),
                },
            )
            .await;
        }
    }
    let mut watched_video_track_sid: Option<String> = None;
    let (video_frame_tx, mut video_frame_rx) = mpsc::channel::<WorkerEvent>(1);
    let mut active_video_task: Option<(String, tokio::task::JoinHandle<()>)> = None;
    let mut audio_state = WorkerAudioState {
        input_device: None,
        output_device: None,
        input_volume: 100,
        output_volume: 100,
    };

    loop {
        tokio::select! {
            line_res = read_line_limited(&mut reader, &mut stdin_line_buf, MAX_WORKER_COMMAND_LINE_BYTES) => {
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
                                let next_sid = track_sid
                                    .map(|sid| sid.trim().to_owned())
                                    .filter(|sid| !sid.is_empty());
                                if active_video_task
                                    .as_ref()
                                    .is_some_and(|(active_sid, _)| next_sid.as_deref() != Some(active_sid.as_str()))
                                {
                                    abort_video_task(&mut active_video_task);
                                }
                                watched_video_track_sid = next_sid;
                                apply_watched_video_track(&room, watched_video_track_sid.as_deref());
                            }
                            WorkerCommand::Close => {
                                abort_video_task(&mut active_video_task);
                                break;
                            }
                            WorkerCommand::Connect { .. } => {}
                        }
                    }
                    Ok(None) | Err(_) => {
                        abort_video_task(&mut active_video_task);
                        break;
                    }
                }
            }
            event_opt = room_events.recv() => {
                match event_opt {
                    Some(RoomEvent::TrackPublished {
                        publication,
                        participant,
                    }) => {
                        emit_event(
                            &mut stdout,
                            &WorkerEvent::TrackPublished {
                                participant_identity: participant.identity().to_string(),
                                participant_sid: participant.sid().to_string(),
                                track_sid: publication.sid().to_string(),
                                source: track_source_label(publication.source()).to_owned(),
                            },
                        )
                        .await;
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
                    Some(RoomEvent::TrackUnpublished {
                        publication,
                        participant,
                    }) => {
                        let track_sid = publication.sid().to_string();
                        if active_video_task
                            .as_ref()
                            .is_some_and(|(active_sid, _)| active_sid == &track_sid)
                        {
                            abort_video_task(&mut active_video_task);
                        }
                        emit_event(
                            &mut stdout,
                            &WorkerEvent::TrackUnpublished {
                                participant_identity: participant.identity().to_string(),
                                participant_sid: participant.sid().to_string(),
                                track_sid,
                                source: track_source_label(publication.source()).to_owned(),
                            },
                        )
                        .await;
                    }
                    Some(RoomEvent::TrackSubscribed {
                        track,
                        publication,
                        participant,
                    }) => {
                        let track_sid = publication.sid().to_string();
                        emit_event(
                            &mut stdout,
                            &WorkerEvent::TrackSubscribed {
                                participant_identity: participant.identity().to_string(),
                                participant_sid: participant.sid().to_string(),
                                track_sid: track_sid.clone(),
                                source: track_source_label(publication.source()).to_owned(),
                            },
                        )
                        .await;
                        if let RemoteTrack::Video(video_track) = track
                            && watched_video_track_sid.as_deref() == Some(track_sid.as_str())
                        {
                            abort_video_task(&mut active_video_task);
                            let handle = spawn_video_decode_task(
                                video_track,
                                track_sid.clone(),
                                video_frame_tx.clone(),
                            );
                            active_video_task = Some((track_sid, handle));
                        }
                    }
                    Some(RoomEvent::TrackUnsubscribed {
                        publication,
                        participant,
                        ..
                    }) => {
                        let track_sid = publication.sid().to_string();
                        if active_video_task
                            .as_ref()
                            .is_some_and(|(active_sid, _)| active_sid == &track_sid)
                        {
                            abort_video_task(&mut active_video_task);
                        }
                        emit_event(
                            &mut stdout,
                            &WorkerEvent::TrackUnsubscribed {
                                participant_identity: participant.identity().to_string(),
                                participant_sid: participant.sid().to_string(),
                                track_sid,
                                source: track_source_label(publication.source()).to_owned(),
                            },
                        )
                        .await;
                    }
                    Some(RoomEvent::Disconnected { reason }) => {
                        abort_video_task(&mut active_video_task);
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
                    None => {
                        abort_video_task(&mut active_video_task);
                        break;
                    }
                }
            }
            Some(frame_ev) = video_frame_rx.recv() => {
                emit_event(&mut stdout, &frame_ev).await;
            }
        }
    }

    abort_video_task(&mut active_video_task);

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
            initial_mic_muted: true,
            initial_playback_muted: false,
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
                WorkerEvent::TrackPublished {
                    participant_identity: "user-1".to_owned(),
                    participant_sid: "PA_1".to_owned(),
                    track_sid: "TR_pub_1".to_owned(),
                    source: "screen_share".to_owned(),
                },
                "\"type\":\"track_published\"",
            ),
            (
                WorkerEvent::TrackUnpublished {
                    participant_identity: "user-1".to_owned(),
                    participant_sid: "PA_1".to_owned(),
                    track_sid: "TR_pub_1".to_owned(),
                    source: "camera".to_owned(),
                },
                "\"type\":\"track_unpublished\"",
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
            (
                WorkerEvent::VideoFrame {
                    track_sid: "TR_cam_1".to_owned(),
                    width: 640,
                    height: 360,
                    rgba_b64: "AAAA".to_owned(),
                },
                "\"type\":\"video_frame\"",
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

    #[test]
    fn compute_scaled_dimensions_preserves_aspect_ratio_and_bounds() {
        assert_eq!(compute_scaled_dimensions(0, 720, 960, 540), None);
        assert_eq!(compute_scaled_dimensions(1280, 0, 960, 540), None);
        assert_eq!(compute_scaled_dimensions(1280, 720, 0, 540), None);
        assert_eq!(compute_scaled_dimensions(1280, 720, 960, 0), None);

        // Within bounds: unchanged.
        assert_eq!(compute_scaled_dimensions(640, 360, 960, 540), Some((640, 360)));
        assert_eq!(compute_scaled_dimensions(960, 540, 960, 540), Some((960, 540)));

        // Landscape downscale.
        assert_eq!(compute_scaled_dimensions(1920, 1080, 960, 540), Some((960, 540)));
        assert_eq!(compute_scaled_dimensions(1280, 720, 960, 540), Some((960, 540)));

        // Portrait downscale constrained by max_h.
        assert_eq!(compute_scaled_dimensions(1080, 1920, 960, 540), Some((304, 540)));

        // Extreme aspect ratio clamps smaller dimension to >= 1.
        assert_eq!(compute_scaled_dimensions(4000, 1, 960, 540), Some((960, 1)));
    }

    #[test]
    fn worker_command_connect_debug_redacts_endpoint_and_token() {
        let cmd = WorkerCommand::Connect {
            endpoint: "wss://secret-voice.example.test/rtc".to_owned(),
            token: "super-secret-livekit-jwt-token".to_owned(),
            connection_id: "conn-123".to_owned(),
            initial_mic_muted: true,
            initial_playback_muted: false,
        };
        let rendered = format!("{cmd:?}");
        assert!(!rendered.contains("secret-voice.example.test"));
        assert!(!rendered.contains("super-secret-livekit-jwt-token"));
        assert!(rendered.contains("<redacted>"));
        assert!(rendered.contains("conn-123"));
    }

    #[test]
    fn worker_command_connect_initial_mic_muted_defaults_to_true_when_omitted() {
        let json = r#"{"type":"connect","endpoint":"wss://rtc.example.test","token":"jwt-1","connection_id":"conn-1"}"#;
        let decoded: WorkerCommand = serde_json::from_str(json).unwrap();
        assert_eq!(
            decoded,
            WorkerCommand::Connect {
                endpoint: "wss://rtc.example.test".to_owned(),
                token: "jwt-1".to_owned(),
                connection_id: "conn-1".to_owned(),
                initial_mic_muted: true,
                initial_playback_muted: false,
            }
        );
    }

    #[test]
    fn read_line_limited_rejects_oversized_lines() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let data = b"hello\nworld_too_long\n";
            let mut reader = BufReader::new(&data[..]);
            let mut buf = Vec::new();
            assert_eq!(
                read_line_limited(&mut reader, &mut buf, 5)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("hello")
            );
            let err = read_line_limited(&mut reader, &mut buf, 5)
                .await
                .expect_err("oversized line must fail");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        });
    }

    #[test]
    fn read_line_limited_resumes_partial_line_across_select_cancellation() {
        use tokio::io::AsyncWriteExt;

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (mut writer, reader_half) = tokio::io::duplex(64);
            let mut reader = BufReader::new(reader_half);
            let mut buf = Vec::new();
            let (notify_tx, mut notify_rx) = mpsc::channel::<()>(1);

            writer.write_all(b"{\"type\":\"set_mic").await.unwrap();
            writer.flush().await.unwrap();
            notify_tx.send(()).await.unwrap();

            tokio::select! {
                biased;
                _ = read_line_limited(&mut reader, &mut buf, 128) => {
                    panic!("incomplete line should not resolve before newline");
                }
                _ = notify_rx.recv() => {}
            }
            assert!(!buf.is_empty(), "partial bytes must remain in caller buffer");

            writer.write_all(b"_muted\",\"muted\":false}\n").await.unwrap();
            writer.flush().await.unwrap();

            let line = read_line_limited(&mut reader, &mut buf, 128)
                .await
                .unwrap()
                .expect("completed line");
            assert_eq!(line, r#"{"type":"set_mic_muted","muted":false}"#);
            assert!(buf.is_empty(), "caller buffer must be cleared after completing line");
        });
    }
}
