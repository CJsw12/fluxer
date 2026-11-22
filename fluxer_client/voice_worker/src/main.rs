//! Isolated LiveKit voice worker process communicating over private stdio JSON-lines IPC.

use std::fmt;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::StreamExt as _;
use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use livekit::webrtc::desktop_capturer::{
    CaptureError, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
};
use livekit::webrtc::video_frame::{I420Buffer, VideoFormatType, VideoFrame, VideoRotation};
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::{RtcVideoSource, VideoResolution};
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

struct ActiveScreenShare {
    pub_sid: TrackSid,
    stop_flag: Arc<AtomicBool>,
    capture_thread: Option<std::thread::JoinHandle<()>>,
}

async fn stop_screen_share(active_screen_share: &mut Option<ActiveScreenShare>, room: &Room) {
    if let Some(mut share) = active_screen_share.take() {
        share.stop_flag.store(true, Ordering::Release);
        drop(share.capture_thread.take());
        let _ = room.local_participant().unpublish_track(&share.pub_sid).await;
    }
}

/// Converts a BGRA8888 buffer into planar I420 (YUV 4:2:0) using integer BT.601 math.
///
/// Returns `false` without panicking if dimensions are `< 2`, odd, or any buffer/stride
/// is smaller than required.
#[allow(clippy::too_many_arguments)]
fn bgra_to_i420(
    bgra: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    y_plane: &mut [u8],
    y_stride: usize,
    u_plane: &mut [u8],
    u_stride: usize,
    v_plane: &mut [u8],
    v_stride: usize,
) -> bool {
    if width < 2 || height < 2 || (width & 1) != 0 || (height & 1) != 0 {
        return false;
    }
    let w = width as usize;
    let h = height as usize;
    let cw = w / 2;
    let ch = h / 2;

    let Some(min_bgra_row) = w.checked_mul(4) else {
        return false;
    };
    if stride < min_bgra_row || y_stride < w || u_stride < cw || v_stride < cw {
        return false;
    }

    let Some(req_bgra) = (h - 1).checked_mul(stride).and_then(|o| o.checked_add(min_bgra_row)) else {
        return false;
    };
    let Some(req_y) = (h - 1).checked_mul(y_stride).and_then(|o| o.checked_add(w)) else {
        return false;
    };
    let Some(req_u) = (ch - 1).checked_mul(u_stride).and_then(|o| o.checked_add(cw)) else {
        return false;
    };
    let Some(req_v) = (ch - 1).checked_mul(v_stride).and_then(|o| o.checked_add(cw)) else {
        return false;
    };

    if bgra.len() < req_bgra
        || y_plane.len() < req_y
        || u_plane.len() < req_u
        || v_plane.len() < req_v
    {
        return false;
    }

    for row in 0..h {
        let src_row = &bgra[row * stride..row * stride + min_bgra_row];
        let dst_y = &mut y_plane[row * y_stride..row * y_stride + w];
        for (px, y_out) in src_row.chunks_exact(4).zip(dst_y.iter_mut()) {
            let b = i32::from(px[0]);
            let g = i32::from(px[1]);
            let r = i32::from(px[2]);
            let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            *y_out = y.clamp(0, 255) as u8;
        }
    }

    for cy in 0..ch {
        let r0 = &bgra[(cy * 2) * stride..(cy * 2) * stride + min_bgra_row];
        let r1 = &bgra[(cy * 2 + 1) * stride..(cy * 2 + 1) * stride + min_bgra_row];
        let dst_u = &mut u_plane[cy * u_stride..cy * u_stride + cw];
        let dst_v = &mut v_plane[cy * v_stride..cy * v_stride + cw];

        for cx in 0..cw {
            let x0 = cx * 8;
            let b = (i32::from(r0[x0])
                + i32::from(r0[x0 + 4])
                + i32::from(r1[x0])
                + i32::from(r1[x0 + 4])
                + 2)
                >> 2;
            let g = (i32::from(r0[x0 + 1])
                + i32::from(r0[x0 + 5])
                + i32::from(r1[x0 + 1])
                + i32::from(r1[x0 + 5])
                + 2)
                >> 2;
            let r = (i32::from(r0[x0 + 2])
                + i32::from(r0[x0 + 6])
                + i32::from(r1[x0 + 2])
                + i32::from(r1[x0 + 6])
                + 2)
                >> 2;

            let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
            let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
            dst_u[cx] = u.clamp(0, 255) as u8;
            dst_v[cx] = v.clamp(0, 255) as u8;
        }
    }

    true
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
    let mut active_screen_share: Option<ActiveScreenShare> = None;
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
                            WorkerCommand::SetScreenShare { active, .. } => {
                                if !active {
                                    stop_screen_share(&mut active_screen_share, &room).await;
                                } else if active_screen_share.is_none() {
                                    let mut opts = DesktopCapturerOptions::new(
                                        DesktopCaptureSourceType::Screen,
                                    );
                                    opts.set_include_cursor(true);
                                    let Some(mut capturer) = DesktopCapturer::new(opts) else {
                                        emit_event(
                                            &mut stdout,
                                            &WorkerEvent::Error {
                                                message: "screen capture unavailable".to_owned(),
                                            },
                                        )
                                        .await;
                                        continue;
                                    };
                                    let primary_source =
                                        capturer.get_source_list().into_iter().next();
                                    let source = NativeVideoSource::new(
                                        VideoResolution {
                                            width: 1280,
                                            height: 720,
                                        },
                                        true,
                                    );
                                    let track = LocalVideoTrack::create_video_track(
                                        "screen_share",
                                        RtcVideoSource::Native(source.clone()),
                                    );
                                    let publish_opts = TrackPublishOptions {
                                        source: TrackSource::Screenshare,
                                        ..Default::default()
                                    };
                                    let pub_handle = match room
                                        .local_participant()
                                        .publish_track(LocalTrack::Video(track), publish_opts)
                                        .await
                                    {
                                        Ok(handle) => handle,
                                        Err(_) => {
                                            emit_event(
                                                &mut stdout,
                                                &WorkerEvent::Error {
                                                    message: "failed to publish screen share track"
                                                        .to_owned(),
                                                },
                                            )
                                            .await;
                                            continue;
                                        }
                                    };
                                    let pub_sid = pub_handle.sid();
                                    let stop_flag = Arc::new(AtomicBool::new(false));
                                    let thread_stop_flag = Arc::clone(&stop_flag);
                                    let callback_stop_flag = Arc::clone(&stop_flag);
                                    let start_instant = Instant::now();
                                    capturer.start_capture(primary_source, move |result| {
                                        match result {
                                            Ok(frame) => {
                                                if callback_stop_flag.load(Ordering::Acquire) {
                                                    return;
                                                }
                                                let raw_w = frame.width();
                                                let raw_h = frame.height();
                                                if raw_w <= 0 || raw_h <= 0 {
                                                    return;
                                                }
                                                let w = (raw_w as u32) & !1;
                                                let h = (raw_h as u32) & !1;
                                                if w < 2 || h < 2 {
                                                    return;
                                                }
                                                let src_stride = frame.stride() as usize;
                                                let src_data = frame.data();
                                                let mut i420 = I420Buffer::new(w, h);
                                                let (y_stride, u_stride, v_stride) = i420.strides();
                                                let (y_plane, u_plane, v_plane) = i420.data_mut();
                                                if !bgra_to_i420(
                                                    src_data,
                                                    src_stride,
                                                    w,
                                                    h,
                                                    y_plane,
                                                    y_stride as usize,
                                                    u_plane,
                                                    u_stride as usize,
                                                    v_plane,
                                                    v_stride as usize,
                                                ) {
                                                    return;
                                                }
                                                let timestamp_us = i64::try_from(
                                                    start_instant.elapsed().as_micros(),
                                                )
                                                .unwrap_or(i64::MAX);
                                                let video_frame = VideoFrame {
                                                    rotation: VideoRotation::VideoRotation0,
                                                    timestamp_us,
                                                    frame_metadata: None,
                                                    buffer: i420,
                                                };
                                                let _ = source.capture_frame(&video_frame);
                                            }
                                            Err(CaptureError::Temporary) => {}
                                            Err(CaptureError::Permanent) => {
                                                callback_stop_flag.store(true, Ordering::Release);
                                            }
                                        }
                                    });
                                    let capture_thread = std::thread::spawn(move || {
                                        while !thread_stop_flag.load(Ordering::Acquire) {
                                            capturer.capture_frame();
                                            if thread_stop_flag.load(Ordering::Acquire) {
                                                break;
                                            }
                                            std::thread::sleep(VIDEO_FRAME_MIN_INTERVAL);
                                        }
                                    });
                                    active_screen_share = Some(ActiveScreenShare {
                                        pub_sid,
                                        stop_flag,
                                        capture_thread: Some(capture_thread),
                                    });
                                }
                            }
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
                                stop_screen_share(&mut active_screen_share, &room).await;
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
                        stop_screen_share(&mut active_screen_share, &room).await;
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
    stop_screen_share(&mut active_screen_share, &room).await;

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

    fn make_solid_bgra_4x4(bgra_pixel: [u8; 4], stride: usize) -> Vec<u8> {
        let mut buf = vec![0xAAu8; stride * 4];
        for row in 0..4 {
            for col in 0..4 {
                let off = row * stride + col * 4;
                buf[off..off + 4].copy_from_slice(&bgra_pixel);
            }
        }
        buf
    }

    #[test]
    fn bgra_to_i420_converts_white_black_and_color_with_padded_stride() {
        let stride = 20; // 4 * 4 + 4 padding bytes
        let y_stride = 6; // 4 + 2 padding bytes
        let u_stride = 3; // 2 + 1 padding byte
        let v_stride = 3;

        for (pixel, exp_y, exp_u, exp_v) in [
            ([255, 255, 255, 255], 235u8, 128u8, 128u8), // pure white
            ([0, 0, 0, 255], 16u8, 128u8, 128u8),        // pure black
            ([0, 0, 255, 255], 82u8, 90u8, 240u8),       // pure red (BGRA)
            ([255, 0, 0, 255], 41u8, 240u8, 110u8),      // pure blue (BGRA)
        ] {
            let bgra = make_solid_bgra_4x4(pixel, stride);
            let mut y = vec![0x55u8; y_stride * 4];
            let mut u = vec![0x55u8; u_stride * 2];
            let mut v = vec![0x55u8; v_stride * 2];

            assert!(bgra_to_i420(
                &bgra, stride, 4, 4, &mut y, y_stride, &mut u, u_stride, &mut v, v_stride
            ));

            for row in 0..4 {
                for col in 0..4 {
                    assert_eq!(y[row * y_stride + col], exp_y);
                }
                assert_eq!(y[row * y_stride + 4], 0x55);
                assert_eq!(y[row * y_stride + 5], 0x55);
            }
            for crow in 0..2 {
                for ccol in 0..2 {
                    assert_eq!(u[crow * u_stride + ccol], exp_u);
                    assert_eq!(v[crow * v_stride + ccol], exp_v);
                }
                assert_eq!(u[crow * u_stride + 2], 0x55);
                assert_eq!(v[crow * v_stride + 2], 0x55);
            }
        }
    }

    #[test]
    fn bgra_to_i420_rejects_invalid_dimensions_and_short_buffers_without_panicking() {
        let mut y = vec![0u8; 16];
        let mut u = vec![0u8; 4];
        let mut v = vec![0u8; 4];
        let bgra = vec![0u8; 64];

        // Invalid dimensions (< 2 or odd).
        assert!(!bgra_to_i420(&bgra, 16, 0, 4, &mut y, 4, &mut u, 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 4, 0, &mut y, 4, &mut u, 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 1, 4, &mut y, 4, &mut u, 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 3, 4, &mut y, 4, &mut u, 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 4, 3, &mut y, 4, &mut u, 2, &mut v, 2));

        // Invalid strides.
        assert!(!bgra_to_i420(&bgra, 12, 4, 4, &mut y, 4, &mut u, 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 4, 4, &mut y, 3, &mut u, 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 4, 4, &mut y, 4, &mut u, 1, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 4, 4, &mut y, 4, &mut u, 2, &mut v, 1));

        // Short buffers.
        assert!(!bgra_to_i420(&bgra[..63], 16, 4, 4, &mut y, 4, &mut u, 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 4, 4, &mut y[..15], 4, &mut u, 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 4, 4, &mut y, 4, &mut u[..3], 2, &mut v, 2));
        assert!(!bgra_to_i420(&bgra, 16, 4, 4, &mut y, 4, &mut u, 2, &mut v[..3], 2));
    }
}
