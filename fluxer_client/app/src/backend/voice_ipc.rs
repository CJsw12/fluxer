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

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use super::voice::{
    TrackSubscriptionKey, UnconfiguredVoiceTransport, VoiceCredentials, VoiceRoom, VoiceRoomEvent,
    VoiceTrackSource, VoiceTransport,
};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const COMMAND_CHANNEL_CAPACITY: usize = 32;
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

/// Commands sent from the GPUI app to the voice worker over private stdin.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum WorkerCommand {
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
    Close,
}

impl fmt::Debug for WorkerCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect { connection_id, .. } => f
                .debug_struct("Connect")
                .field("endpoint", &"<redacted>")
                .field("token", &"<redacted>")
                .field("connection_id", connection_id)
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

pub(crate) fn parse_track_source(raw: &str) -> VoiceTrackSource {
    match raw {
        "microphone" => VoiceTrackSource::Microphone,
        "camera" => VoiceTrackSource::Camera,
        "screen_share" => VoiceTrackSource::ScreenShare,
        "screen_share_audio" => VoiceTrackSource::ScreenShareAudio,
        _ => VoiceTrackSource::Unknown,
    }
}

impl WorkerEvent {
    pub(crate) fn into_track_key(self) -> Option<(bool, TrackSubscriptionKey)> {
        match self {
            Self::TrackSubscribed {
                participant_identity,
                participant_sid,
                track_sid,
                source,
            } => Some((
                true,
                TrackSubscriptionKey {
                    participant_identity,
                    participant_sid,
                    track_sid,
                    source: parse_track_source(&source),
                },
            )),
            Self::TrackUnsubscribed {
                participant_identity,
                participant_sid,
                track_sid,
                source,
            } => Some((
                false,
                TrackSubscriptionKey {
                    participant_identity,
                    participant_sid,
                    track_sid,
                    source: parse_track_source(&source),
                },
            )),
            Self::Connected | Self::Error { .. } | Self::Disconnected { .. } => None,
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

/// Active voice room backed by a private stdio IPC channel to the worker process.
pub(crate) struct ProcessVoiceRoom {
    tx: mpsc::Sender<WorkerCommand>,
    closed: Arc<AtomicBool>,
    kill_tx: Mutex<Option<oneshot::Sender<()>>>,
    subscribed_tracks: Arc<Mutex<BTreeSet<TrackSubscriptionKey>>>,
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
}

impl VoiceRoom for ProcessVoiceRoom {
    fn set_microphone_muted(&self, muted: bool) {
        self.send_command(WorkerCommand::SetMicMuted { muted });
    }

    fn set_playback_muted(&self, muted: bool) {
        self.send_command(WorkerCommand::SetPlaybackMuted { muted });
    }

    fn set_screen_share(&self, active: bool, with_audio: bool) {
        self.send_command(WorkerCommand::SetScreenShare { active, with_audio });
    }

    fn subscribed_tracks(&self) -> Vec<TrackSubscriptionKey> {
        ProcessVoiceRoom::subscribed_tracks(self)
    }

    fn take_event_rx(&self) -> Option<mpsc::UnboundedReceiver<VoiceRoomEvent>> {
        self.event_rx.lock().ok().and_then(|mut guard| guard.take())
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.tx.try_send(WorkerCommand::Close);
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

async fn run_writer<W>(mut writer: W, mut rx: mpsc::Receiver<WorkerCommand>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    while let Some(cmd) = rx.recv().await {
        let is_close = matches!(cmd, WorkerCommand::Close);
        if write_command_line(&mut writer, &cmd).await.is_err() {
            break;
        }
        if is_close {
            break;
        }
    }
    let _ = writer.shutdown().await;
}

async fn run_event_reader<R>(
    mut lines: tokio::io::Lines<BufReader<R>>,
    subscribed_tracks: Arc<Mutex<BTreeSet<TrackSubscriptionKey>>>,
    event_tx: mpsc::UnboundedSender<VoiceRoomEvent>,
    closed: Arc<AtomicBool>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut received_disconnect = false;
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(event) = serde_json::from_str::<WorkerEvent>(&line) else {
            continue;
        };
        match event {
            WorkerEvent::Disconnected { reason } => {
                received_disconnect = true;
                let _ = event_tx.send(VoiceRoomEvent::Disconnected(reason));
                break;
            }
            other => {
                if let Some((subscribed, key)) = other.into_track_key() {
                    let changed = if let Ok(mut guard) = subscribed_tracks.lock() {
                        if subscribed {
                            guard.insert(key)
                        } else {
                            guard.remove(&key)
                        }
                    } else {
                        false
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
    write_command_line(&mut writer, &connect_cmd).await?;

    let mut lines = BufReader::new(reader).lines();
    let line = tokio::time::timeout(connect_timeout, lines.next_line())
        .await
        .map_err(|_| "voice worker connect timed out".to_owned())?
        .map_err(|err| format!("failed to read voice worker stdout: {err}"))?
        .ok_or_else(|| "voice worker exited before connecting".to_owned())?;

    let event: WorkerEvent = serde_json::from_str(&line)
        .map_err(|err| format!("invalid voice worker event: {err}"))?;

    match event {
        WorkerEvent::Connected => {
            let (tx, rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
            let (event_tx, event_rx) = mpsc::unbounded_channel();
            let closed = Arc::new(AtomicBool::new(false));
            let subscribed_tracks = Arc::new(Mutex::new(BTreeSet::new()));
            tokio::spawn(run_writer(writer, rx));
            tokio::spawn(run_event_reader(
                lines,
                subscribed_tracks.clone(),
                event_tx,
                closed.clone(),
            ));
            Ok(ProcessVoiceRoom {
                tx,
                closed,
                kill_tx: Mutex::new(kill_tx),
                subscribed_tracks,
                event_rx: Mutex::new(Some(event_rx)),
            })
        }
        WorkerEvent::Error { message } => Err(message),
        WorkerEvent::Disconnected { reason } => {
            Err(format!("voice worker disconnected during connect: {reason}"))
        }
        WorkerEvent::TrackSubscribed { .. } | WorkerEvent::TrackUnsubscribed { .. } => {
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
                },
                Duration::from_secs(2),
                None,
            )
            .await
            .expect("handshake must succeed");

            room.set_microphone_muted(true);
            room.set_playback_muted(true);
            room.set_screen_share(true, true);
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
                    WorkerCommand::Close,
                ]
            );
        });
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
}
