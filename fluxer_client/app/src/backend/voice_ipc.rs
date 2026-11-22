//! Process-isolated voice transport over private stdio JSON-lines IPC.
//!
//! LiveKit/webrtc-sys requires static CRT (`+crt-static`) on Windows MSVC, so it
//! runs in a separate helper process (`fluxer_voice_worker`) while the GPUI app
//! (`/MD`) communicates exclusively over private stdin/stdout pipes.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use super::voice::{
    TrackSubscriptionKey, UnconfiguredVoiceTransport, VoiceAudioSettings, VoiceCredentials,
    VoiceRoom, VoiceRoomEvent, VoiceTrackSource, VoiceTransport,
};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const COMMAND_CHANNEL_CAPACITY: usize = 32;
const MAX_IPC_VIDEO_WIDTH: u32 = 1920;
const MAX_IPC_VIDEO_HEIGHT: u32 = 1080;
const MAX_IPC_VIDEO_B64_LEN: usize = 12 * 1024 * 1024;
const MAX_WORKER_EVENT_LINE_BYTES: usize = 13 * 1024 * 1024;
const VOICE_WORKER_ENV_VAR: &str = "FLUXER_VOICE_WORKER_PATH";
#[cfg(windows)]
const VOICE_WORKER_BIN: &str = "fluxer_voice_worker.exe";
#[cfg(not(windows))]
const VOICE_WORKER_BIN: &str = "fluxer_voice_worker";

/// Pure helper to resolve the voice worker binary path from an optional env
/// override and executable directory.
pub(crate) fn resolve_worker_path_in(
    env_override: Option<&OsStr>,
    exe_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(raw) = env_override.filter(|v| !v.is_empty()) {
        let path = PathBuf::from(raw);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(dir) = exe_dir {
        let candidate = dir.join(VOICE_WORKER_BIN);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolves the voice worker executable from `FLUXER_VOICE_WORKER_PATH` or next
/// to the current executable.
pub(crate) fn resolve_voice_worker_path() -> Option<PathBuf> {
    let env_override = std::env::var_os(VOICE_WORKER_ENV_VAR);
    let exe_path = std::env::current_exe().ok();
    let exe_dir = exe_path.as_deref().and_then(Path::parent);
    resolve_worker_path_in(env_override.as_deref(), exe_dir)
}

/// Returns the default `VoiceTransport`: `ProcessVoiceTransport` when a worker
/// binary is present, otherwise `UnconfiguredVoiceTransport`.
pub(crate) fn default_voice_transport() -> Arc<dyn VoiceTransport> {
    match resolve_voice_worker_path() {
        Some(path) => Arc::new(ProcessVoiceTransport::new(path)),
        None => Arc::new(UnconfiguredVoiceTransport),
    }
}

fn default_true() -> bool {
    true
}

/// Commands sent from the GPUI app to the voice worker over private stdin.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum WorkerCommand {
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
            Self::SetPlaybackMuted { muted } => {
                f.debug_struct("SetPlaybackMuted").field("muted", muted).finish()
            }
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

/// Events emitted by the voice worker over stdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum WorkerEvent {
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

fn decode_worker_video_frame(
    track_sid: String,
    width: u32,
    height: u32,
    rgba_b64: &str,
) -> Option<(String, u32, u32, Vec<u8>)> {
    if width < 1
        || height < 1
        || width > MAX_IPC_VIDEO_WIDTH
        || height > MAX_IPC_VIDEO_HEIGHT
        || rgba_b64.len() > MAX_IPC_VIDEO_B64_LEN
    {
        return None;
    }
    let expected_len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    let rgba = base64::engine::general_purpose::STANDARD
        .decode(rgba_b64)
        .ok()?;
    if rgba.len() != expected_len {
        return None;
    }
    Some((track_sid, width, height, rgba))
}

pub(crate) fn parse_track_source(raw: &str) -> VoiceTrackSource {
    match raw {
        "microphone" => VoiceTrackSource::Microphone,
        "camera" => VoiceTrackSource::Camera,
        "screen_share" => VoiceTrackSource::ScreenShare,
        "screen_share_audio" => VoiceTrackSource::ScreenShareAudio,
        _ => VoiceTrackSource::Unknown,
    }
}

enum TrackEventKind {
    Published,
    Unpublished,
    Subscribed,
    Unsubscribed,
}

fn build_track_key(
    participant_identity: String,
    participant_sid: String,
    track_sid: String,
    source: String,
) -> TrackSubscriptionKey {
    TrackSubscriptionKey {
        participant_identity,
        participant_sid,
        track_sid,
        source: parse_track_source(&source),
    }
}

impl WorkerEvent {
    fn into_track_update(self) -> Option<(TrackEventKind, TrackSubscriptionKey)> {
        match self {
            Self::TrackPublished {
                participant_identity,
                participant_sid,
                track_sid,
                source,
            } => Some((
                TrackEventKind::Published,
                build_track_key(participant_identity, participant_sid, track_sid, source),
            )),
            Self::TrackUnpublished {
                participant_identity,
                participant_sid,
                track_sid,
                source,
            } => Some((
                TrackEventKind::Unpublished,
                build_track_key(participant_identity, participant_sid, track_sid, source),
            )),
            Self::TrackSubscribed {
                participant_identity,
                participant_sid,
                track_sid,
                source,
            } => Some((
                TrackEventKind::Subscribed,
                build_track_key(participant_identity, participant_sid, track_sid, source),
            )),
            Self::TrackUnsubscribed {
                participant_identity,
                participant_sid,
                track_sid,
                source,
            } => Some((
                TrackEventKind::Unsubscribed,
                build_track_key(participant_identity, participant_sid, track_sid, source),
            )),
            Self::Connected
            | Self::Error { .. }
            | Self::Disconnected { .. }
            | Self::VideoFrame { .. } => None,
        }
    }
}

/// Spawns an isolated `fluxer_voice_worker` subprocess per connection attempt.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ProcessVoiceTransport {
    worker_path: PathBuf,
    connect_timeout: Duration,
}

#[allow(dead_code)]
impl ProcessVoiceTransport {
    pub(crate) fn new(worker_path: impl Into<PathBuf>) -> Self {
        Self {
            worker_path: worker_path.into(),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }

    pub(crate) fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }
}

impl VoiceTransport for ProcessVoiceTransport {
    fn connect(
        &self,
        credentials: VoiceCredentials,
    ) -> Pin<Box<dyn Future<Output = Result<Arc<dyn VoiceRoom>, String>> + Send>> {
        let worker_path = self.worker_path.clone();
        let connect_timeout = self.connect_timeout;
        Box::pin(async move {
            let mut cmd = tokio::process::Command::new(&worker_path);
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            #[cfg(windows)]
            {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            if credentials.endpoint.starts_with("ws://") {
                for var in [
                    "HTTP_PROXY",
                    "HTTPS_PROXY",
                    "ALL_PROXY",
                    "http_proxy",
                    "https_proxy",
                    "all_proxy",
                ] {
                    cmd.env_remove(var);
                }
            }

            let mut child = cmd
                .spawn()
                .map_err(|err| format!("failed to spawn voice worker: {err}"))?;
            let Some(stdin) = child.stdin.take() else {
                let _ = child.kill().await;
                return Err("voice worker stdin pipe unavailable".to_owned());
            };
            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill().await;
                return Err("voice worker stdout pipe unavailable".to_owned());
            };

            let connect_cmd = WorkerCommand::Connect {
                endpoint: credentials.endpoint,
                token: credentials.token.expose().to_owned(),
                connection_id: credentials.connection_id,
                initial_mic_muted: credentials.initial_mic_muted,
                initial_playback_muted: credentials.initial_playback_muted,
            };

            let (kill_tx, kill_rx) = oneshot::channel::<()>();
            match connect_over_streams(stdin, stdout, connect_cmd, connect_timeout, Some(kill_tx)).await {
                Ok(room) => {
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = kill_rx => {
                                let _ = child.kill().await;
                            }
                            _ = child.wait() => {}
                        }
                    });
                    Ok(Arc::new(room) as Arc<dyn VoiceRoom>)
                }
                Err(err) => {
                    let _ = child.kill().await;
                    Err(err)
                }
            }
        })
    }
}

struct MuteSyncState {
    mic_muted: AtomicBool,
    mic_dirty: AtomicBool,
    playback_muted: AtomicBool,
    playback_dirty: AtomicBool,
    close_requested: AtomicBool,
    notify: tokio::sync::Notify,
}

impl MuteSyncState {
    fn new() -> Self {
        Self {
            mic_muted: AtomicBool::new(false),
            mic_dirty: AtomicBool::new(false),
            playback_muted: AtomicBool::new(false),
            playback_dirty: AtomicBool::new(false),
            close_requested: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }
}

/// Active voice room backed by a private stdio IPC channel to the worker process.
pub(crate) struct ProcessVoiceRoom {
    tx: mpsc::Sender<WorkerCommand>,
    closed: Arc<AtomicBool>,
    kill_tx: Mutex<Option<oneshot::Sender<()>>>,
    mute_state: Arc<MuteSyncState>,
    subscribed_tracks: Arc<Mutex<BTreeSet<TrackSubscriptionKey>>>,
    published_tracks: Arc<Mutex<BTreeSet<TrackSubscriptionKey>>>,
    pending_video_frame: Arc<Mutex<Option<(String, u32, u32, Vec<u8>)>>>,
    video_frame_notified: Arc<AtomicBool>,
    event_rx: Mutex<Option<mpsc::UnboundedReceiver<VoiceRoomEvent>>>,
}

impl ProcessVoiceRoom {
    fn send_command(&self, cmd: WorkerCommand) {
        if !self.closed.load(Ordering::SeqCst) {
            let _ = self.tx.try_send(cmd);
        }
    }

    pub(crate) fn subscribed_tracks(&self) -> Vec<TrackSubscriptionKey> {
        self.subscribed_tracks
            .lock()
            .map_or_else(|_| Vec::new(), |guard| guard.iter().cloned().collect())
    }

    pub(crate) fn published_tracks(&self) -> Vec<TrackSubscriptionKey> {
        self.published_tracks
            .lock()
            .map_or_else(|_| Vec::new(), |guard| guard.iter().cloned().collect())
    }
}

impl VoiceRoom for ProcessVoiceRoom {
    fn set_microphone_muted(&self, muted: bool) {
        if !self.closed.load(Ordering::SeqCst) {
            self.mute_state.mic_muted.store(muted, Ordering::Release);
            self.mute_state.mic_dirty.store(true, Ordering::Release);
            self.mute_state.notify.notify_one();
        }
    }

    fn set_playback_muted(&self, muted: bool) {
        if !self.closed.load(Ordering::SeqCst) {
            self.mute_state.playback_muted.store(muted, Ordering::Release);
            self.mute_state.playback_dirty.store(true, Ordering::Release);
            self.mute_state.notify.notify_one();
        }
    }

    fn set_screen_share(&self, active: bool, with_audio: bool) {
        self.send_command(WorkerCommand::SetScreenShare { active, with_audio });
    }

    fn set_audio_settings(&self, settings: &VoiceAudioSettings) {
        self.send_command(WorkerCommand::SetAudioSettings {
            input_device: settings.input_device.clone(),
            output_device: settings.output_device.clone(),
            input_volume: settings.input_volume,
            output_volume: settings.output_volume,
        });
    }

    fn set_watched_video_track(&self, track_sid: Option<String>) {
        self.send_command(WorkerCommand::SetWatchedVideoTrack { track_sid });
    }

    fn subscribed_tracks(&self) -> Vec<TrackSubscriptionKey> {
        ProcessVoiceRoom::subscribed_tracks(self)
    }

    fn published_tracks(&self) -> Vec<TrackSubscriptionKey> {
        ProcessVoiceRoom::published_tracks(self)
    }

    fn take_event_rx(&self) -> Option<mpsc::UnboundedReceiver<VoiceRoomEvent>> {
        self.event_rx.lock().ok().and_then(|mut guard| guard.take())
    }

    fn take_pending_video_frame(&self) -> Option<(String, u32, u32, Vec<u8>)> {
        self.video_frame_notified.store(false, Ordering::Release);
        self.pending_video_frame
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.tx.try_send(WorkerCommand::Close).is_err() {
            self.mute_state.close_requested.store(true, Ordering::Release);
            self.mute_state.notify.notify_one();
        }
        if let Ok(mut guard) = self.kill_tx.lock()
            && let Some(kill_tx) = guard.take()
        {
            let _ = kill_tx.send(());
        }
    }
}

impl Drop for ProcessVoiceRoom {
    fn drop(&mut self) {
        self.close();
    }
}

async fn write_command_line<W>(writer: &mut W, cmd: &WorkerCommand) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
{
    let mut line = serde_json::to_string(cmd)
        .map_err(|err| format!("failed to serialize voice worker command: {err}"))?;
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|err| format!("failed to write voice worker command: {err}"))?;
    writer
        .flush()
        .await
        .map_err(|err| format!("failed to flush voice worker command: {err}"))?;
    Ok(())
}

async fn flush_dirty_mutes<W>(writer: &mut W, mute_state: &MuteSyncState) -> Result<(), ()>
where
    W: AsyncWrite + Unpin,
{
    while mute_state.mic_dirty.swap(false, Ordering::AcqRel) {
        let muted = mute_state.mic_muted.load(Ordering::Acquire);
        if write_command_line(writer, &WorkerCommand::SetMicMuted { muted })
            .await
            .is_err()
        {
            return Err(());
        }
    }
    while mute_state.playback_dirty.swap(false, Ordering::AcqRel) {
        let muted = mute_state.playback_muted.load(Ordering::Acquire);
        if write_command_line(writer, &WorkerCommand::SetPlaybackMuted { muted })
            .await
            .is_err()
        {
            return Err(());
        }
    }
    Ok(())
}

async fn run_writer<W>(
    mut writer: W,
    mut rx: mpsc::Receiver<WorkerCommand>,
    mute_state: Arc<MuteSyncState>,
) where
    W: AsyncWrite + Unpin + Send + 'static,
{
    loop {
        if flush_dirty_mutes(&mut writer, &mute_state).await.is_err() {
            break;
        }
        if mute_state.close_requested.load(Ordering::Acquire) {
            let _ = write_command_line(&mut writer, &WorkerCommand::Close).await;
            break;
        }
        tokio::select! {
            biased;
            _ = mute_state.notify.notified() => {}
            maybe_cmd = rx.recv() => {
                match maybe_cmd {
                    Some(cmd) => {
                        let is_close = matches!(cmd, WorkerCommand::Close);
                        if write_command_line(&mut writer, &cmd).await.is_err() {
                            break;
                        }
                        if is_close {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
    let _ = writer.shutdown().await;
}

async fn read_line_limited<R>(
    reader: &mut BufReader<R>,
    max_bytes: usize,
) -> std::io::Result<Option<String>>
where
    R: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
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
    String::from_utf8(buf).map(Some).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ipc line is not valid utf-8",
        )
    })
}

async fn run_event_reader<R>(
    mut reader: BufReader<R>,
    subscribed_tracks: Arc<Mutex<BTreeSet<TrackSubscriptionKey>>>,
    published_tracks: Arc<Mutex<BTreeSet<TrackSubscriptionKey>>>,
    pending_video_frame: Arc<Mutex<Option<(String, u32, u32, Vec<u8>)>>>,
    video_frame_notified: Arc<AtomicBool>,
    event_tx: mpsc::UnboundedSender<VoiceRoomEvent>,
    closed: Arc<AtomicBool>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut received_disconnect = false;
    while let Ok(Some(line)) = read_line_limited(&mut reader, MAX_WORKER_EVENT_LINE_BYTES).await {
        let Ok(event) = serde_json::from_str::<WorkerEvent>(&line) else {
            continue;
        };
        match event {
            WorkerEvent::Disconnected { reason } => {
                received_disconnect = true;
                let _ = event_tx.send(VoiceRoomEvent::Disconnected(reason));
                break;
            }
            WorkerEvent::VideoFrame {
                track_sid,
                width,
                height,
                rgba_b64,
            } => {
                if let Some(frame) = decode_worker_video_frame(track_sid, width, height, &rgba_b64)
                {
                    if let Ok(mut guard) = pending_video_frame.lock() {
                        *guard = Some(frame);
                    }
                    if !video_frame_notified.swap(true, Ordering::AcqRel) {
                        let _ = event_tx.send(VoiceRoomEvent::VideoFrameReady);
                    }
                }
            }
            other => {
                if let Some((kind, key)) = other.into_track_update() {
                    let changed = match kind {
                        TrackEventKind::Published => published_tracks
                            .lock()
                            .is_ok_and(|mut guard| guard.insert(key)),
                        TrackEventKind::Unpublished => {
                            let pub_removed = published_tracks
                                .lock()
                                .is_ok_and(|mut guard| guard.remove(&key));
                            let sub_removed = subscribed_tracks
                                .lock()
                                .is_ok_and(|mut guard| guard.remove(&key));
                            pub_removed || sub_removed
                        }
                        TrackEventKind::Subscribed => {
                            let pub_inserted = published_tracks
                                .lock()
                                .is_ok_and(|mut guard| guard.insert(key.clone()));
                            let sub_inserted = subscribed_tracks
                                .lock()
                                .is_ok_and(|mut guard| guard.insert(key));
                            pub_inserted || sub_inserted
                        }
                        TrackEventKind::Unsubscribed => subscribed_tracks
                            .lock()
                            .is_ok_and(|mut guard| guard.remove(&key)),
                    };
                    if changed {
                        let _ = event_tx.send(VoiceRoomEvent::TracksChanged);
                    }
                }
            }
        }
    }
    if let Ok(mut guard) = subscribed_tracks.lock() {
        guard.clear();
    }
    if let Ok(mut guard) = published_tracks.lock() {
        guard.clear();
    }
    if !received_disconnect && !closed.load(Ordering::SeqCst) {
        let _ = event_tx.send(VoiceRoomEvent::Disconnected(
            "voice worker exited unexpectedly".to_owned(),
        ));
    }
}

/// Performs the initial `Connect` handshake over arbitrary `AsyncWrite`/`AsyncRead` streams.
pub(crate) async fn connect_over_streams<W, R>(
    mut writer: W,
    reader: R,
    connect_cmd: WorkerCommand,
    connect_timeout: Duration,
    kill_tx: Option<oneshot::Sender<()>>,
) -> Result<ProcessVoiceRoom, String>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    let line = tokio::time::timeout(connect_timeout, async {
        write_command_line(&mut writer, &connect_cmd).await?;
        read_line_limited(&mut reader, MAX_WORKER_EVENT_LINE_BYTES)
            .await
            .map_err(|err| format!("failed to read voice worker stdout: {err}"))?
            .ok_or_else(|| "voice worker exited before connecting".to_owned())
    })
    .await
    .map_err(|_| "voice worker connect timed out".to_owned())??;

    let event: WorkerEvent = serde_json::from_str(&line)
        .map_err(|err| format!("invalid voice worker event: {err}"))?;

    match event {
        WorkerEvent::Connected => {
            let (tx, rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
            let (event_tx, event_rx) = mpsc::unbounded_channel();
            let closed = Arc::new(AtomicBool::new(false));
            let mute_state = Arc::new(MuteSyncState::new());
            let subscribed_tracks = Arc::new(Mutex::new(BTreeSet::new()));
            let published_tracks = Arc::new(Mutex::new(BTreeSet::new()));
            let pending_video_frame = Arc::new(Mutex::new(None));
            let video_frame_notified = Arc::new(AtomicBool::new(false));
            tokio::spawn(run_writer(writer, rx, mute_state.clone()));
            tokio::spawn(run_event_reader(
                reader,
                subscribed_tracks.clone(),
                published_tracks.clone(),
                pending_video_frame.clone(),
                video_frame_notified.clone(),
                event_tx,
                closed.clone(),
            ));
            Ok(ProcessVoiceRoom {
                tx,
                closed,
                kill_tx: Mutex::new(kill_tx),
                mute_state,
                subscribed_tracks,
                published_tracks,
                pending_video_frame,
                video_frame_notified,
                event_rx: Mutex::new(Some(event_rx)),
            })
        }
        WorkerEvent::Error { message } => Err(message),
        WorkerEvent::Disconnected { reason } => {
            Err(format!("voice worker disconnected during connect: {reason}"))
        }
        WorkerEvent::TrackPublished { .. }
        | WorkerEvent::TrackUnpublished { .. }
        | WorkerEvent::TrackSubscribed { .. }
        | WorkerEvent::TrackUnsubscribed { .. }
        | WorkerEvent::VideoFrame { .. } => {
            Err("unexpected track event before voice worker connected".to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::rest::SecretToken;
    use fluxer_client_core::state::Snowflake;

    use super::*;

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
    fn duplex_handshake_and_commands_emit_exact_json_lines() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (client_to_worker_w, client_to_worker_r) = tokio::io::duplex(4096);
            let (worker_to_client_w, worker_to_client_r) = tokio::io::duplex(4096);

            let worker_task = tokio::spawn(async move {
                let mut reader = BufReader::new(client_to_worker_r).lines();
                let mut writer = worker_to_client_w;

                let first = reader.next_line().await.unwrap().unwrap();
                let cmd: WorkerCommand = serde_json::from_str(&first).unwrap();
                assert_eq!(
                    cmd,
                    WorkerCommand::Connect {
                        endpoint: "wss://rtc.example.test".to_owned(),
                        token: "secret-jwt".to_owned(),
                        connection_id: "conn-1".to_owned(),
                        initial_mic_muted: true,
                        initial_playback_muted: false,
                    }
                );

                let connected = format!("{}\n", serde_json::to_string(&WorkerEvent::Connected).unwrap());
                writer.write_all(connected.as_bytes()).await.unwrap();
                writer.flush().await.unwrap();

                let mut received = Vec::new();
                while let Some(line) = reader.next_line().await.unwrap() {
                    let cmd: WorkerCommand = serde_json::from_str(&line).unwrap();
                    let is_close = matches!(cmd, WorkerCommand::Close);
                    received.push(cmd);
                    if is_close {
                        break;
                    }
                }
                received
            });

            let room = connect_over_streams(
                client_to_worker_w,
                worker_to_client_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: "secret-jwt".to_owned(),
                    connection_id: "conn-1".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .expect("handshake must succeed");

            room.set_microphone_muted(true);
            room.set_playback_muted(true);
            room.set_screen_share(true, true);
            room.set_audio_settings(&VoiceAudioSettings {
                input_device: Some("USB Mic".to_owned()),
                output_device: None,
                input_volume: 135,
                output_volume: 80,
            });
            room.set_watched_video_track(Some("TR_screen_1".to_owned()));
            room.set_watched_video_track(None);
            room.close();
            // Duplicate close must be a safe no-op.
            room.close();

            let received = worker_task.await.unwrap();
            assert_eq!(
                received,
                vec![
                    WorkerCommand::SetMicMuted { muted: true },
                    WorkerCommand::SetPlaybackMuted { muted: true },
                    WorkerCommand::SetScreenShare { active: true, with_audio: true },
                    WorkerCommand::SetAudioSettings {
                        input_device: Some("USB Mic".to_owned()),
                        output_device: None,
                        input_volume: 135,
                        output_volume: 80,
                    },
                    WorkerCommand::SetWatchedVideoTrack {
                        track_sid: Some("TR_screen_1".to_owned()),
                    },
                    WorkerCommand::SetWatchedVideoTrack { track_sid: None },
                    WorkerCommand::Close,
                ]
            );
        });
    }

    #[test]
    fn worker_command_set_watched_video_track_serializes_and_formats_debug() {
        let watch = WorkerCommand::SetWatchedVideoTrack {
            track_sid: Some("TR_cam_42".to_owned()),
        };
        let watch_json = serde_json::to_string(&watch).unwrap();
        assert!(watch_json.contains("\"type\":\"set_watched_video_track\""));
        assert!(watch_json.contains("\"track_sid\":\"TR_cam_42\""));
        assert_eq!(
            serde_json::from_str::<WorkerCommand>(&watch_json).unwrap(),
            watch
        );
        assert!(format!("{watch:?}").contains("SetWatchedVideoTrack"));
        assert!(format!("{watch:?}").contains("TR_cam_42"));

        let unwatch = WorkerCommand::SetWatchedVideoTrack { track_sid: None };
        let unwatch_json = serde_json::to_string(&unwatch).unwrap();
        assert!(unwatch_json.contains("\"type\":\"set_watched_video_track\""));
        assert!(unwatch_json.contains("\"track_sid\":null"));
        assert_eq!(
            serde_json::from_str::<WorkerCommand>(&unwatch_json).unwrap(),
            unwatch
        );
    }

    #[test]
    fn duplex_error_early_eof_and_timeout_fail_without_exposing_token() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let secret_token = "never-leak-this-token-12345";

        // 1. Worker returns WorkerEvent::Error.
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(1024);
            let (mut w_w, w_r) = tokio::io::duplex(1024);
            tokio::spawn(async move {
                let err_line = format!(
                    "{}\n",
                    serde_json::to_string(&WorkerEvent::Error {
                        message: "room auth failed".to_owned(),
                    })
                    .unwrap()
                );
                w_w.write_all(err_line.as_bytes()).await.unwrap();
                w_w.flush().await.unwrap();
            });
            let err = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: secret_token.to_owned(),
                    connection_id: "conn-err".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .err()
            .expect("error event must fail connect");
            assert_eq!(err, "room auth failed");
            assert!(!err.contains(secret_token));
        });

        // 2. Worker closes stdout immediately (early EOF).
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(1024);
            let (w_w, w_r) = tokio::io::duplex(1024);
            drop(w_w);
            let err = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: secret_token.to_owned(),
                    connection_id: "conn-eof".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .err()
            .expect("early EOF must fail connect");
            assert!(err.contains("exited before connecting"));
            assert!(!err.contains(secret_token));
        });

        // 3. Worker stalls past connect_timeout.
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(1024);
            let (_w_w, w_r) = tokio::io::duplex(1024);
            let err = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: secret_token.to_owned(),
                    connection_id: "conn-timeout".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_millis(20),
                None,
            )
            .await
            .err()
            .expect("timeout must fail connect");
            assert!(err.contains("timed out"));
            assert!(!err.contains(secret_token));
        });
    }

    #[test]
    fn missing_worker_binary_returns_clean_error_without_secrets() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let transport = ProcessVoiceTransport::new("nonexistent_fluxer_voice_worker_binary_xyz.exe")
            .with_connect_timeout(Duration::from_millis(100));
        let secret_token = "super-secret-spawn-token";
        let secret_endpoint = "wss://secret-spawn.example.test";
        let creds = VoiceCredentials {
            attempt_id: 1,
            guild_id: Some(Snowflake(42)),
            channel_id: Snowflake(100),
            connection_id: "conn-missing".to_owned(),
            endpoint: secret_endpoint.to_owned(),
            token: SecretToken::new(secret_token),
            initial_mic_muted: true,
            initial_playback_muted: false,
        };
        let err = rt
            .block_on(transport.connect(creds))
            .err()
            .expect("missing worker binary must return Err");
        assert!(err.contains("failed to spawn voice worker"));
        assert!(!err.contains(secret_token));
        assert!(!err.contains(secret_endpoint));
    }

    #[test]
    fn resolve_worker_path_in_prefers_env_then_exe_dir_then_none() {
        let unique = format!(
            "fluxer_voice_ipc_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let exe_dir = root.join("bin");
        let empty_dir = root.join("empty");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&empty_dir).unwrap();

        let env_worker = root.join("custom_worker.exe");
        let exe_worker = exe_dir.join(VOICE_WORKER_BIN);
        std::fs::write(&env_worker, b"").unwrap();
        std::fs::write(&exe_worker, b"").unwrap();

        // 1. Env override takes precedence over exe_dir when it is a real file.
        let resolved = resolve_worker_path_in(Some(env_worker.as_os_str()), Some(&exe_dir));
        assert_eq!(resolved.as_deref(), Some(env_worker.as_path()));

        // 2. Empty or missing env override falls back to exe_dir/VOICE_WORKER_BIN.
        assert_eq!(
            resolve_worker_path_in(Some(OsStr::new("")), Some(&exe_dir)).as_deref(),
            Some(exe_worker.as_path())
        );
        let nonexistent_env = root.join("does_not_exist.exe");
        assert_eq!(
            resolve_worker_path_in(Some(nonexistent_env.as_os_str()), Some(&exe_dir)).as_deref(),
            Some(exe_worker.as_path())
        );
        assert_eq!(
            resolve_worker_path_in(None, Some(&exe_dir)).as_deref(),
            Some(exe_worker.as_path())
        );

        // 3. Returns None when neither env override nor exe_dir contains a worker file.
        assert_eq!(resolve_worker_path_in(None, Some(&empty_dir)), None);
        assert_eq!(resolve_worker_path_in(None, None), None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplex_track_subscribed_and_unsubscribed_update_room_tracks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(4096);
            let (mut w_w, w_r) = tokio::io::duplex(4096);

            let connected = format!("{}\n", serde_json::to_string(&WorkerEvent::Connected).unwrap());
            w_w.write_all(connected.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            let room = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: "secret-jwt".to_owned(),
                    connection_id: "conn-tracks".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .expect("handshake must succeed");

            let mic_sub = WorkerEvent::TrackSubscribed {
                participant_identity: "user-2:conn-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_mic_2".to_owned(),
                source: "microphone".to_owned(),
            };
            let screen_sub = WorkerEvent::TrackSubscribed {
                participant_identity: "user-3:conn-3".to_owned(),
                participant_sid: "PA_3".to_owned(),
                track_sid: "TR_screen_3".to_owned(),
                source: "screen_share_audio".to_owned(),
            };
            for ev in [&mic_sub, &screen_sub] {
                let line = format!("{}\n", serde_json::to_string(ev).unwrap());
                w_w.write_all(line.as_bytes()).await.unwrap();
            }
            w_w.flush().await.unwrap();

            for _ in 0..50 {
                if room.subscribed_tracks().len() == 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(
                VoiceRoom::subscribed_tracks(&room),
                vec![
                    TrackSubscriptionKey {
                        participant_identity: "user-2:conn-2".to_owned(),
                        participant_sid: "PA_2".to_owned(),
                        track_sid: "TR_mic_2".to_owned(),
                        source: VoiceTrackSource::Microphone,
                    },
                    TrackSubscriptionKey {
                        participant_identity: "user-3:conn-3".to_owned(),
                        participant_sid: "PA_3".to_owned(),
                        track_sid: "TR_screen_3".to_owned(),
                        source: VoiceTrackSource::ScreenShareAudio,
                    },
                ]
            );

            let mic_unsub = WorkerEvent::TrackUnsubscribed {
                participant_identity: "user-2:conn-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_mic_2".to_owned(),
                source: "microphone".to_owned(),
            };
            let line = format!("{}\n", serde_json::to_string(&mic_unsub).unwrap());
            w_w.write_all(line.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            for _ in 0..50 {
                if room.subscribed_tracks().len() == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(
                room.subscribed_tracks(),
                vec![TrackSubscriptionKey {
                    participant_identity: "user-3:conn-3".to_owned(),
                    participant_sid: "PA_3".to_owned(),
                    track_sid: "TR_screen_3".to_owned(),
                    source: VoiceTrackSource::ScreenShareAudio,
                }]
            );

            let disc = format!(
                "{}\n",
                serde_json::to_string(&WorkerEvent::Disconnected {
                    reason: "ServerShutdown".to_owned(),
                })
                .unwrap()
            );
            w_w.write_all(disc.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            for _ in 0..50 {
                if room.subscribed_tracks().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(room.subscribed_tracks().is_empty());
        });
    }

    #[test]
    fn duplex_track_event_and_unexpected_eof_emit_room_events() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(4096);
            let (mut w_w, w_r) = tokio::io::duplex(4096);

            let connected = format!("{}\n", serde_json::to_string(&WorkerEvent::Connected).unwrap());
            w_w.write_all(connected.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            let room = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: "secret-jwt".to_owned(),
                    connection_id: "conn-eof-ev".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .expect("handshake must succeed");

            let mut ev_rx = VoiceRoom::take_event_rx(&room).expect("event_rx must be present");

            let sub = WorkerEvent::TrackSubscribed {
                participant_identity: "user-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_2".to_owned(),
                source: "microphone".to_owned(),
            };
            let line = format!("{}\n", serde_json::to_string(&sub).unwrap());
            w_w.write_all(line.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap(),
                Some(VoiceRoomEvent::TracksChanged)
            );

            // Drop worker stdout without close() -> unexpected EOF emits Disconnected.
            drop(w_w);
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap(),
                Some(VoiceRoomEvent::Disconnected(
                    "voice worker exited unexpectedly".to_owned()
                ))
            );
        });
    }

    #[test]
    fn duplex_video_frame_decodes_valid_and_drops_invalid_without_breaking_loop() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(65536);
            let (mut w_w, w_r) = tokio::io::duplex(65536);

            let connected = format!("{}\n", serde_json::to_string(&WorkerEvent::Connected).unwrap());
            w_w.write_all(connected.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            let room = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: "secret-jwt".to_owned(),
                    connection_id: "conn-video".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .expect("handshake must succeed");

            let mut ev_rx = VoiceRoom::take_event_rx(&room).expect("event_rx must be present");

            let valid_rgba = vec![10, 20, 30, 255, 40, 50, 60, 255];
            let valid_ev = WorkerEvent::VideoFrame {
                track_sid: "TR_cam_1".to_owned(),
                width: 2,
                height: 1,
                rgba_b64: base64::engine::general_purpose::STANDARD.encode(&valid_rgba),
            };

            let invalid_zero_dim = WorkerEvent::VideoFrame {
                track_sid: "TR_cam_1".to_owned(),
                width: 0,
                height: 1,
                rgba_b64: String::new(),
            };
            let invalid_oversized_dim = WorkerEvent::VideoFrame {
                track_sid: "TR_cam_1".to_owned(),
                width: 1921,
                height: 1080,
                rgba_b64: "AAAA".to_owned(),
            };
            let invalid_len_mismatch = WorkerEvent::VideoFrame {
                track_sid: "TR_cam_1".to_owned(),
                width: 2,
                height: 2,
                rgba_b64: base64::engine::general_purpose::STANDARD.encode(&valid_rgba),
            };
            let invalid_b64 = WorkerEvent::VideoFrame {
                track_sid: "TR_cam_1".to_owned(),
                width: 1,
                height: 1,
                rgba_b64: "not!valid!b64!".to_owned(),
            };
            let follow_up = WorkerEvent::TrackSubscribed {
                participant_identity: "user-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_mic_2".to_owned(),
                source: "microphone".to_owned(),
            };

            for ev in [
                &valid_ev,
                &invalid_zero_dim,
                &invalid_oversized_dim,
                &invalid_len_mismatch,
                &invalid_b64,
                &follow_up,
            ] {
                let line = format!("{}\n", serde_json::to_string(ev).unwrap());
                w_w.write_all(line.as_bytes()).await.unwrap();
            }
            w_w.flush().await.unwrap();

            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap(),
                Some(VoiceRoomEvent::VideoFrameReady)
            );
            assert_eq!(
                VoiceRoom::take_pending_video_frame(&room),
                Some(("TR_cam_1".to_owned(), 2, 1, valid_rgba))
            );
            assert_eq!(VoiceRoom::take_pending_video_frame(&room), None);
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap(),
                Some(VoiceRoomEvent::TracksChanged)
            );
        });
    }

    #[test]
    fn duplex_track_published_and_unpublished_manage_published_and_subscribed_tracks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(4096);
            let (mut w_w, w_r) = tokio::io::duplex(4096);

            let connected = format!("{}\n", serde_json::to_string(&WorkerEvent::Connected).unwrap());
            w_w.write_all(connected.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            let room = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: "secret-jwt".to_owned(),
                    connection_id: "conn-pub".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .expect("handshake must succeed");

            let mut ev_rx = VoiceRoom::take_event_rx(&room).expect("event_rx must be present");

            let screen_pub = WorkerEvent::TrackPublished {
                participant_identity: "user-2:conn-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_screen_2".to_owned(),
                source: "screen_share".to_owned(),
            };
            let line = format!("{}\n", serde_json::to_string(&screen_pub).unwrap());
            w_w.write_all(line.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap(),
                Some(VoiceRoomEvent::TracksChanged)
            );
            let expected_key = TrackSubscriptionKey {
                participant_identity: "user-2:conn-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_screen_2".to_owned(),
                source: VoiceTrackSource::ScreenShare,
            };
            assert_eq!(VoiceRoom::published_tracks(&room), vec![expected_key.clone()]);
            assert!(VoiceRoom::subscribed_tracks(&room).is_empty());

            // Subscribing adds to subscribed_tracks while keeping it in published_tracks.
            let screen_sub = WorkerEvent::TrackSubscribed {
                participant_identity: "user-2:conn-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_screen_2".to_owned(),
                source: "screen_share".to_owned(),
            };
            let line = format!("{}\n", serde_json::to_string(&screen_sub).unwrap());
            w_w.write_all(line.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap(),
                Some(VoiceRoomEvent::TracksChanged)
            );
            assert_eq!(VoiceRoom::published_tracks(&room), vec![expected_key.clone()]);
            assert_eq!(VoiceRoom::subscribed_tracks(&room), vec![expected_key]);

            // Unpublishing removes from both published_tracks and subscribed_tracks.
            let screen_unpub = WorkerEvent::TrackUnpublished {
                participant_identity: "user-2:conn-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_screen_2".to_owned(),
                source: "screen_share".to_owned(),
            };
            let line = format!("{}\n", serde_json::to_string(&screen_unpub).unwrap());
            w_w.write_all(line.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap(),
                Some(VoiceRoomEvent::TracksChanged)
            );
            assert!(VoiceRoom::published_tracks(&room).is_empty());
            assert!(VoiceRoom::subscribed_tracks(&room).is_empty());
        });
    }

    #[test]
    fn read_line_limited_rejects_oversized_lines() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let data = b"ok\ntoo_long_line\n";
            let mut reader = BufReader::new(&data[..]);
            assert_eq!(
                read_line_limited(&mut reader, 4).await.unwrap().as_deref(),
                Some("ok")
            );
            let err = read_line_limited(&mut reader, 4)
                .await
                .expect_err("oversized line must fail");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        });
    }

    #[test]
    fn connect_over_streams_times_out_when_write_stalls() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(16);
            let (_w_w, w_r) = tokio::io::duplex(16);
            let err = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: "secret-jwt-payload-larger-than-duplex-buffer".to_owned(),
                    connection_id: "conn-stall".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_millis(20),
                None,
            )
            .await
            .err()
            .expect("stalled write must time out");
            assert!(err.contains("timed out"));
        });
    }

    #[test]
    fn duplex_video_frame_coalesces_multiple_frames_to_latest() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (c_w, _c_r) = tokio::io::duplex(65536);
            let (mut w_w, w_r) = tokio::io::duplex(65536);

            let connected = format!("{}\n", serde_json::to_string(&WorkerEvent::Connected).unwrap());
            w_w.write_all(connected.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            let room = connect_over_streams(
                c_w,
                w_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: "secret-jwt".to_owned(),
                    connection_id: "conn-coalesce".to_owned(),
                    initial_mic_muted: true,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .expect("handshake must succeed");

            let mut ev_rx = VoiceRoom::take_event_rx(&room).expect("event_rx must be present");

            let frame1 = vec![1, 1, 1, 255];
            let frame2 = vec![2, 2, 2, 255];
            let frame3 = vec![3, 3, 3, 255];
            for rgba in [&frame1, &frame2, &frame3] {
                let ev = WorkerEvent::VideoFrame {
                    track_sid: "TR_cam_1".to_owned(),
                    width: 1,
                    height: 1,
                    rgba_b64: base64::engine::general_purpose::STANDARD.encode(rgba),
                };
                let line = format!("{}\n", serde_json::to_string(&ev).unwrap());
                w_w.write_all(line.as_bytes()).await.unwrap();
            }
            let follow_up = WorkerEvent::TrackSubscribed {
                participant_identity: "user-2".to_owned(),
                participant_sid: "PA_2".to_owned(),
                track_sid: "TR_mic_2".to_owned(),
                source: "microphone".to_owned(),
            };
            let follow_line = format!("{}\n", serde_json::to_string(&follow_up).unwrap());
            w_w.write_all(follow_line.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            // Wait until TracksChanged has been processed so all 3 frames were already read.
            let mut saw_ready = false;
            let mut saw_tracks = false;
            for _ in 0..4 {
                match tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap()
                {
                    Some(VoiceRoomEvent::VideoFrameReady) => {
                        assert!(!saw_ready, "VideoFrameReady must be coalesced to 1 notification");
                        saw_ready = true;
                    }
                    Some(VoiceRoomEvent::TracksChanged) => {
                        saw_tracks = true;
                        break;
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
            assert!(saw_ready && saw_tracks);
            assert_eq!(
                VoiceRoom::take_pending_video_frame(&room),
                Some(("TR_cam_1".to_owned(), 1, 1, frame3))
            );
            assert_eq!(VoiceRoom::take_pending_video_frame(&room), None);

            // A subsequent frame after consumption emits a fresh VideoFrameReady.
            let frame4 = vec![4, 4, 4, 255];
            let ev4 = WorkerEvent::VideoFrame {
                track_sid: "TR_cam_1".to_owned(),
                width: 1,
                height: 1,
                rgba_b64: base64::engine::general_purpose::STANDARD.encode(&frame4),
            };
            let line4 = format!("{}\n", serde_json::to_string(&ev4).unwrap());
            w_w.write_all(line4.as_bytes()).await.unwrap();
            w_w.flush().await.unwrap();

            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                    .await
                    .unwrap(),
                Some(VoiceRoomEvent::VideoFrameReady)
            );
            assert_eq!(
                VoiceRoom::take_pending_video_frame(&room),
                Some(("TR_cam_1".to_owned(), 1, 1, frame4))
            );
        });
    }

    #[test]
    fn mute_updates_are_delivered_even_when_command_queue_is_saturated() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (client_to_worker_w, client_to_worker_r) = tokio::io::duplex(64 * 1024);
            let (worker_to_client_w, worker_to_client_r) = tokio::io::duplex(4096);

            let worker_task = tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(client_to_worker_r).lines();
                let _connect = reader.next_line().await.unwrap().unwrap();
                let mut writer = worker_to_client_w;
                let connected = format!("{}\n", serde_json::to_string(&WorkerEvent::Connected).unwrap());
                writer.write_all(connected.as_bytes()).await.unwrap();
                writer.flush().await.unwrap();

                let mut received = Vec::new();
                while let Some(line) = reader.next_line().await.unwrap() {
                    let cmd: WorkerCommand = serde_json::from_str(&line).unwrap();
                    let is_close = matches!(cmd, WorkerCommand::Close);
                    received.push(cmd);
                    if is_close {
                        break;
                    }
                }
                received
            });

            let room = connect_over_streams(
                client_to_worker_w,
                worker_to_client_r,
                WorkerCommand::Connect {
                    endpoint: "wss://rtc.example.test".to_owned(),
                    token: "secret-jwt".to_owned(),
                    connection_id: "conn-sat".to_owned(),
                    initial_mic_muted: false,
                    initial_playback_muted: false,
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .expect("handshake must succeed");

            // Saturate the bounded command channel completely.
            for i in 0..(COMMAND_CHANNEL_CAPACITY + 8) {
                room.set_watched_video_track(Some(format!("TR_{i}")));
            }
            // Mute updates must still be delivered with the latest state even though `tx` is full.
            room.set_microphone_muted(false);
            room.set_microphone_muted(true);
            room.set_playback_muted(true);

            // Allow the writer to drain queued commands so Close can be enqueued.
            tokio::task::yield_now().await;
            room.close();

            let received = worker_task.await.unwrap();
            assert!(received.contains(&WorkerCommand::SetMicMuted { muted: true }));
            assert!(received.contains(&WorkerCommand::SetPlaybackMuted { muted: true }));
        });
    }
}
