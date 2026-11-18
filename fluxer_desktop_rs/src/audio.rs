// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum AudioEvent {
    Frame {
        capture_id: String,
        sample_rate: u32,
        channels: u32,
        timestamp_us: i64,
        samples_base64: String,
    },
    End {
        capture_id: String,
        reason: String,
        detail: Option<String>,
    },
}

pub type AudioEventSink = Arc<dyn Fn(AudioEvent) + Send + Sync>;

static NEXT_CAPTURE_ID: AtomicU64 = AtomicU64::new(1);
static GLOBAL_AUDIO_SINK: LazyLock<RwLock<Option<AudioEventSink>>> =
    LazyLock::new(|| RwLock::new(None));

static ACTIVE_CAPTURES: LazyLock<RwLock<HashMap<String, Arc<CaptureSession>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

enum CaptureSignal {
    Started,
    Error(String),
    Closed,
    Stop,
    SetEndpoints(Vec<String>, mpsc::Sender<bool>),
}

struct CaptureSession {
    id: String,
    signals: mpsc::Sender<CaptureSignal>,
    finished: AtomicBool,
    delivery: Mutex<()>,
    sink: Option<AudioEventSink>,
}

impl CaptureSession {
    fn finish(&self, reason: &str, detail: Option<String>) -> bool {
        let _delivery = self.delivery.lock().unwrap_or_else(|e| e.into_inner());
        if self.finished.swap(true, Ordering::AcqRel) {
            return false;
        }
        ACTIVE_CAPTURES
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
        if let Some(sink) = &self.sink {
            sink(AudioEvent::End {
                capture_id: self.id.clone(),
                reason: reason.to_string(),
                detail,
            });
        }
        true
    }

    fn emit_frame(&self, frame: AudioEvent) {
        // The application sink only enqueues events to the UI event loop. Serialize
        // that enqueue with End, including frames encoded before finish began.
        let _delivery = self.delivery.lock().unwrap_or_else(|e| e.into_inner());
        if !self.finished.load(Ordering::Acquire)
            && let Some(sink) = &self.sink
        {
            sink(frame);
        }
    }
}

// Only the supervisor owns the backend. Native callbacks send signals and never
// dispose it, because native disposal joins the callback's worker thread.
trait CaptureBackend: Send {
    fn start(&self) -> Result<(), String>;
    fn dispose(&self);
    fn set_endpoints(&self, endpoints: Vec<String>) -> bool;
}

#[cfg(windows)]
impl CaptureBackend for fluxer_win_process_loopback::ProcessLoopback {
    fn start(&self) -> Result<(), String> {
        self.start()
            .map_err(|e| format!("Failed to start WASAPI loopback: {e}"))
    }

    fn dispose(&self) {
        self.dispose();
    }

    fn set_endpoints(&self, endpoints: Vec<String>) -> bool {
        self.set_endpoints(endpoints).unwrap_or(false)
    }
}

fn launch_capture(
    sink: Option<AudioEventSink>,
    timeout: Duration,
    create: impl FnOnce(Arc<CaptureSession>) -> Result<Box<dyn CaptureBackend>, String> + Send + 'static,
) -> Result<Value, String> {
    let capture_id = format!(
        "audio-capture-{}",
        NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed)
    );
    let (signals, receiver) = mpsc::channel();
    let (startup, ready) = mpsc::channel();
    let session = Arc::new(CaptureSession {
        id: capture_id.clone(),
        signals,
        finished: AtomicBool::new(false),
        delivery: Mutex::new(()),
        sink,
    });
    ACTIVE_CAPTURES
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(capture_id.clone(), Arc::clone(&session));
    let supervisor_session = Arc::clone(&session);
    if let Err(error) = std::thread::Builder::new()
        .name("fluxer-audio-session".into())
        .spawn(move || {
            supervise_capture(supervisor_session, receiver, startup, timeout, create);
        })
    {
        let message = format!("Failed to spawn audio session supervisor: {error}");
        session.finish("addon-error", Some(message.clone()));
        return Err(message);
    }
    match ready.recv_timeout(timeout) {
        Ok(Ok(())) if !session.finished.load(Ordering::Acquire) => Ok(json!({
            "captureId": capture_id, "sampleRate": 48000, "channels": 2
        })),
        result => {
            let message = match result {
                Ok(Err(error)) => error,
                Ok(Ok(())) => "Audio capture closed during startup".into(),
                Err(mpsc::RecvTimeoutError::Timeout) => "Audio capture startup timed out".into(),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    "Audio capture startup supervisor closed".into()
                }
            };
            session.finish("addon-error", Some(message.clone()));
            let _ = session.signals.send(CaptureSignal::Stop);
            Err(message)
        }
    }
}

fn supervise_capture(
    session: Arc<CaptureSession>,
    receiver: mpsc::Receiver<CaptureSignal>,
    startup: mpsc::Sender<Result<(), String>>,
    timeout: Duration,
    create: impl FnOnce(Arc<CaptureSession>) -> Result<Box<dyn CaptureBackend>, String>,
) {
    let deadline = Instant::now() + timeout;
    let backend = match create(Arc::clone(&session)) {
        Ok(backend) => backend,
        Err(error) => {
            session.finish("addon-error", Some(error.clone()));
            let _ = startup.send(Err(error));
            return;
        }
    };
    let mut startup = Some(startup);
    let result = if session.finished.load(Ordering::Acquire) {
        Err("Audio capture cancelled during startup".into())
    } else {
        backend.start()
    };
    let result = result.and_then(|()| {
        loop {
            let signal = if startup.is_some() {
                receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            } else {
                receiver
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            };
            match signal {
                Ok(CaptureSignal::Started) => {
                    if session.finished.load(Ordering::Acquire) {
                        break Err("Audio capture cancelled during startup".into());
                    }
                    if let Some(startup) = startup.take() {
                        let _ = startup.send(Ok(()));
                    }
                }
                Ok(CaptureSignal::Error(error)) => break Err(error),
                Ok(CaptureSignal::Closed) => {
                    if startup.is_some() {
                        break Err("ProcessLoopback closed before audio capture started".into());
                    }
                    session.finish("target-exited", None);
                    break Ok(());
                }
                Ok(CaptureSignal::Stop) => {
                    session.finish("stopped", None);
                    break Err("Audio capture stopped before audio capture started".into());
                }
                Ok(CaptureSignal::SetEndpoints(endpoints, reply)) => {
                    let changed = !session.finished.load(Ordering::Acquire)
                        && backend.set_endpoints(endpoints);
                    let _ = reply.send(changed);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    break Err("Audio capture startup timed out".into());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err("Audio capture worker closed".into());
                }
            }
        }
    });
    if let Err(error) = &result {
        session.finish("addon-error", Some(error.clone()));
    }
    if let Some(startup) = startup {
        let _ =
            startup.send(result.and_then(|()| Err("Audio capture closed before startup".into())));
    }
    // No registry guard is held across native stop/join or callback delivery.
    backend.dispose();
}

pub struct AudioManager {
    sink: AudioEventSink,
}

impl AudioManager {
    pub fn new(sink: AudioEventSink) -> Self {
        *GLOBAL_AUDIO_SINK.write().unwrap() = Some(Arc::clone(&sink));
        Self { sink }
    }

    pub fn get_availability(&self) -> Value {
        get_availability()
    }

    pub fn list_applications(&self) -> Vec<Value> {
        list_applications()
    }

    pub fn resolve_root_pid(&self, source_id: &str) -> Option<u32> {
        resolve_root_pid(source_id)
    }

    pub fn list_playback_devices(&self) -> Vec<Value> {
        list_playback_devices()
    }

    pub fn start_capture(&self, options: &Value) -> Result<Value, String> {
        start_capture_with_sink(options, Some(&self.sink))
    }

    pub fn stop_capture(&self, capture_id: &str) -> bool {
        stop_capture(capture_id)
    }

    pub fn set_win_endpoints(&self, capture_id: &str, endpoints: Vec<String>) -> bool {
        set_win_endpoints(capture_id, endpoints)
    }
}

/// 네이티브 오디오 캡처 서브시스템 가용성 및 기능을 반환합니다.
pub fn get_availability() -> Value {
    #[cfg(target_os = "windows")]
    {
        let info = fluxer_win_process_loopback::get_backend_info();
        json!({
            "available": info.supported,
            "backend": "windows-wasapi-loopback",
            "reason": info.reason,
            "capabilities": {
                "process": info.process_supported,
                "system": info.system_supported,
                "systemExcludesSelf": info.system_excludes_self,
                "processInclude": info.process_include_supported,
                "processExclude": info.process_exclude_supported,
                "sessionMixer": info.session_mixer_supported,
                "endpointSelect": true,
                "systemLoopbackMode": info.system_loopback_mode
            }
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        json!({
            "available": false,
            "backend": "none",
            "capabilities": {
                "process": false,
                "system": false,
                "systemExcludesSelf": false,
                "processInclude": false,
                "processExclude": false,
                "sessionMixer": false,
                "endpointSelect": false,
                "systemLoopbackMode": "unavailable"
            }
        })
    }
}

/// 현재 실행 중인 프로세스 중 오디오 캡처 대상이 될 수 있는 애플리케이션 목록을 열거합니다.
pub fn list_applications() -> Vec<Value> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32, Process32First, Process32Next,
            TH32CS_SNAPPROCESS,
        };

        let mut apps = Vec::new();
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot != INVALID_HANDLE_VALUE {
                let mut entry: PROCESSENTRY32 = std::mem::zeroed();
                entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

                if Process32First(snapshot, &mut entry) != 0 {
                    loop {
                        let pid = entry.th32ProcessID;
                        if pid > 4 {
                            let name_len = entry
                                .szExeFile
                                .iter()
                                .position(|&c| c == 0)
                                .unwrap_or(entry.szExeFile.len());
                            let name_bytes: Vec<u8> = entry.szExeFile[..name_len]
                                .iter()
                                .map(|&c| c as u8)
                                .collect();
                            let name = String::from_utf8_lossy(&name_bytes).to_string();

                            // 대화상자/백그라운드 유틸리티 필터링
                            if !name.is_empty()
                                && !name.eq_ignore_ascii_case("svchost.exe")
                                && !name.eq_ignore_ascii_case("conhost.exe")
                            {
                                apps.push(json!({
                                    "pid": pid,
                                    "identifier": name,
                                    "name": name,
                                    "audible": true
                                }));
                            }
                        }

                        if Process32Next(snapshot, &mut entry) == 0 {
                            break;
                        }
                    }
                }
                windows_sys::Win32::Foundation::CloseHandle(snapshot);
            }
        }

        apps.sort_by(|a, b| {
            let na = a.get("name").and_then(Value::as_str).unwrap_or("");
            let nb = b.get("name").and_then(Value::as_str).unwrap_or("");
            na.cmp(nb)
        });

        apps
    }

    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// 화면 공유 소스 ID(예: "window:12345:0" 또는 "12345")로부터 대상 프로세스 ID를 추출합니다.
pub fn resolve_root_pid(source_id: &str) -> Option<u32> {
    if source_id.is_empty() {
        return None;
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(stripped) = source_id.strip_prefix("window:")
            && let Some(raw_pid) = stripped
                .split(':')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
        {
            let root = fluxer_win_process_loopback::resolve_audio_root_pid(raw_pid);
            return Some(if root > 0 { root } else { raw_pid });
        }

        if let Ok(raw_pid) = source_id.parse::<u32>() {
            let root = fluxer_win_process_loopback::resolve_audio_root_pid(raw_pid);
            return Some(if root > 0 { root } else { raw_pid });
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(stripped) = source_id.strip_prefix("window:")
            && let Some(pid) = stripped
                .split(':')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
        {
            return Some(pid);
        }

        if let Ok(pid) = source_id.parse::<u32>() {
            return Some(pid);
        }
    }

    None
}

/// 시스템 재생 오디오 장치 목록을 반환합니다.
pub fn list_playback_devices() -> Vec<Value> {
    #[cfg(target_os = "windows")]
    {
        match fluxer_win_process_loopback::list_playback_devices_sync() {
            Ok(devices) => devices
                .into_iter()
                .map(|d| {
                    json!({
                        "id": d.id,
                        "name": d.name,
                        "isDefault": d.is_default,
                        "sessions": d.sessions.into_iter().map(|s| json!({
                            "pid": s.pid,
                            "rootPid": s.root_pid,
                            "exe": s.exe,
                            "state": s.state,
                            "excluded": s.excluded
                        })).collect::<Vec<_>>()
                    })
                })
                .collect(),
            Err(e) => {
                eprintln!("[Fluxer Audio] Failed to enumerate playback devices: {e}");
                vec![json!({
                    "id": "default",
                    "name": "Default Audio Playback Device",
                    "isDefault": true,
                    "sessions": []
                })]
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![json!({
            "id": "default",
            "name": "Default Audio Playback Device",
            "isDefault": true,
            "sessions": []
        })]
    }
}

/// 오디오 캡처 세션을 시작하고 캡처 파라미터를 반환합니다.
pub fn start_capture(options: &Value) -> Result<Value, String> {
    start_capture_with_sink(options, None)
}

/// 오디오 캡처 세션을 sink와 함께 시작합니다.
pub fn start_capture_with_sink(
    options: &Value,
    sink_opt: Option<&AudioEventSink>,
) -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    {
        // Match NativeAudio.ts: absent/unknown scope means process capture and
        // requires a positive uint32 PID. Never substitute the host process.
        let scope = match options.get("winCaptureScope").and_then(Value::as_str) {
            Some("system") => "system",
            Some("session-mixer") => "session-mixer",
            _ => "process",
        };
        let is_system = scope != "process";
        let pid = if is_system {
            std::process::id()
        } else {
            options
                .get("targetPid")
                .and_then(Value::as_u64)
                .and_then(|pid| u32::try_from(pid).ok())
                .filter(|pid| *pid > 0)
                .ok_or_else(|| "Invalid native audio target PID".to_string())?
        };
        let include_tree = !is_system
            && options
                .get("includeProcessTree")
                .and_then(Value::as_bool)
                .unwrap_or(true);
        let endpoints = match options.get("winEndpoints") {
            None => None,
            Some(Value::Array(values)) if values.iter().all(Value::is_string) => Some(
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect(),
            ),
            _ => return Err("invalid endpoints: Invalid Windows playback device selection".into()),
        };
        let sink = sink_opt.cloned().or_else(|| {
            GLOBAL_AUDIO_SINK
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        });
        launch_capture(sink, STARTUP_TIMEOUT, move |session| {
            let on_frame: fluxer_win_process_loopback::RawFrameSink = {
                let session = Arc::clone(&session);
                Arc::new(move |samples, sample_rate, channels, timestamp_us| {
                    if session.finished.load(Ordering::Acquire) {
                        return;
                    }
                    if session.sink.is_some() {
                        // f32 samples are initialized and valid for this callback.
                        let bytes = unsafe {
                            std::slice::from_raw_parts(
                                samples.as_ptr().cast::<u8>(),
                                std::mem::size_of_val(samples),
                            )
                        };
                        use base64::Engine;
                        session.emit_frame(AudioEvent::Frame {
                            capture_id: session.id.clone(),
                            sample_rate,
                            channels,
                            timestamp_us,
                            samples_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                        });
                    }
                })
            };
            let on_error: fluxer_win_process_loopback::RawErrorSink = {
                let signals = session.signals.clone();
                Arc::new(move |error| {
                    let _ = signals.send(CaptureSignal::Error(error));
                })
            };
            let on_closed: fluxer_win_process_loopback::RawVoidSink = {
                let signals = session.signals.clone();
                Arc::new(move || {
                    let _ = signals.send(CaptureSignal::Closed);
                })
            };
            let on_started: fluxer_win_process_loopback::RawVoidSink = {
                let signals = session.signals.clone();
                Arc::new(move || {
                    let _ = signals.send(CaptureSignal::Started);
                })
            };
            fluxer_win_process_loopback::ProcessLoopback::new_with_sink(
                pid,
                include_tree,
                scope,
                endpoints,
                on_frame,
                on_error,
                on_closed,
                Some(on_started),
            )
            .map(|backend| Box::new(backend) as Box<dyn CaptureBackend>)
            .map_err(|error| format!("Failed to initialize WASAPI loopback: {error}"))
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (options, sink_opt);
        Err("Native audio capture is unavailable on this platform".into())
    }
}

/// 활성 오디오 캡처 세션을 중지합니다.
pub fn stop_capture(capture_id: &str) -> bool {
    let session = ACTIVE_CAPTURES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(capture_id)
        .cloned();
    if let Some(session) = session {
        // Suppress subsequent frames immediately; disposal runs on the supervisor.
        let stopped = session.finish("stopped", None);
        let _ = session.signals.send(CaptureSignal::Stop);
        stopped
    } else {
        false
    }
}

/// 활성 Windows 오디오 세션의 엔드포인트를 동적으로 변경합니다.
pub fn set_win_endpoints(capture_id: &str, endpoints: Vec<String>) -> bool {
    let session = ACTIVE_CAPTURES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(capture_id)
        .cloned();
    if let Some(session) = session {
        let (reply, receiver) = mpsc::channel();
        if session
            .signals
            .send(CaptureSignal::SetEndpoints(endpoints, reply))
            .is_ok()
        {
            return receiver.recv_timeout(STARTUP_TIMEOUT).unwrap_or(false);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, atomic::AtomicUsize};

    struct TestBackend {
        session: Arc<CaptureSession>,
        start_error: bool,
        async_error: bool,
        worker: Mutex<Option<std::thread::JoinHandle<()>>>,
        disposed: mpsc::Sender<()>,
    }

    impl CaptureBackend for TestBackend {
        fn start(&self) -> Result<(), String> {
            if self.start_error {
                return Err("synchronous start failure".into());
            }
            if self.async_error {
                let signals = self.session.signals.clone();
                *self.worker.lock().unwrap() = Some(std::thread::spawn(move || {
                    signals
                        .send(CaptureSignal::Error("asynchronous startup failure".into()))
                        .unwrap();
                    let _ = signals.send(CaptureSignal::Closed);
                }));
            }
            Ok(())
        }
        fn dispose(&self) {
            // Reproduce native disposal joining the callback worker.
            if let Some(worker) = self.worker.lock().unwrap().take() {
                assert_ne!(worker.thread().id(), std::thread::current().id());
                worker.join().unwrap();
            }
            assert!(
                !ACTIVE_CAPTURES
                    .write()
                    .unwrap()
                    .contains_key(&self.session.id)
            );
            self.disposed.send(()).unwrap();
        }
        fn set_endpoints(&self, _endpoints: Vec<String>) -> bool {
            true
        }
    }

    fn test_sink() -> (AudioEventSink, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let events = Arc::clone(&count);
        (
            Arc::new(move |event| {
                if let AudioEvent::End { .. } = event {
                    events.fetch_add(1, Ordering::SeqCst);
                }
            }),
            count,
        )
    }

    #[test]
    fn constructor_failure_removes_registration() {
        let (sink, ends) = test_sink();
        let (created, receiver) = mpsc::channel();
        let result = launch_capture(Some(sink), Duration::from_secs(1), move |session| {
            created.send(session).unwrap();
            Err("constructor failure".into())
        });
        let session = receiver.recv().unwrap();
        assert_eq!(result.unwrap_err(), "constructor failure");
        assert!(!ACTIVE_CAPTURES.read().unwrap().contains_key(&session.id));
        assert_eq!(ends.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn synchronous_and_asynchronous_start_failures_dispose_once() {
        for async_error in [false, true] {
            let (sink, ends) = test_sink();
            let (disposed, receiver) = mpsc::channel();
            let result = launch_capture(Some(sink), Duration::from_secs(1), move |session| {
                Ok(Box::new(TestBackend {
                    session,
                    start_error: !async_error,
                    async_error,
                    worker: Mutex::new(None),
                    disposed,
                }))
            });
            assert_eq!(
                result.unwrap_err(),
                if async_error {
                    "asynchronous startup failure"
                } else {
                    "synchronous start failure"
                }
            );
            receiver.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(ends.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn startup_waits_for_ready_and_stop_is_idempotent() {
        let (sink, ends) = test_sink();
        let (created, created_rx) = mpsc::channel();
        let (result, result_rx) = mpsc::channel();
        let (disposed, disposed_rx) = mpsc::channel();
        let caller = std::thread::spawn(move || {
            result
                .send(launch_capture(
                    Some(sink),
                    Duration::from_secs(2),
                    move |session| {
                        created.send(Arc::clone(&session)).unwrap();
                        Ok(Box::new(TestBackend {
                            session,
                            start_error: false,
                            async_error: false,
                            worker: Mutex::new(None),
                            disposed,
                        }))
                    },
                ))
                .unwrap();
        });
        let session = created_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            result_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        session.signals.send(CaptureSignal::Started).unwrap();
        let value = result_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(value["captureId"], session.id);
        assert!(set_win_endpoints(&session.id, vec!["default".into()]));
        assert!(stop_capture(&session.id));
        assert!(!stop_capture(&session.id));
        disposed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(ends.load(Ordering::SeqCst), 1);
        caller.join().unwrap();
    }

    #[test]
    fn close_before_ready_and_startup_timeout_reject_and_cleanup() {
        for close in [false, true] {
            let (sink, ends) = test_sink();
            let (disposed, receiver) = mpsc::channel();
            let error = launch_capture(Some(sink), Duration::from_millis(50), move |session| {
                if close {
                    session.signals.send(CaptureSignal::Closed).unwrap();
                }
                Ok(Box::new(TestBackend {
                    session,
                    start_error: false,
                    async_error: false,
                    worker: Mutex::new(None),
                    disposed,
                }))
            })
            .unwrap_err();
            assert!(
                error.contains(if close { "closed before" } else { "timed out" }),
                "{error}"
            );
            receiver.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(ends.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn invalid_process_pid_never_falls_back_to_host() {
        for options in [
            json!({}),
            json!({"targetPid": 0}),
            json!({"targetPid": -1}),
            json!({"targetPid": 4294967296u64}),
            json!({"targetPid": 1.5}),
            json!({"winCaptureScope": "unknown"}),
        ] {
            let error = start_capture(&options).unwrap_err();
            assert!(
                error.contains(if cfg!(windows) {
                    "target PID"
                } else {
                    "unavailable"
                }),
                "{error}"
            );
        }
    }

    #[test]
    fn frame_prepared_before_end_cannot_be_delivered_after_end() {
        let (signals, _receiver) = mpsc::channel();
        let (events, events_rx) = mpsc::channel();
        let session = Arc::new(CaptureSession {
            id: "frame-order-test".into(),
            signals,
            finished: AtomicBool::new(false),
            delivery: Mutex::new(()),
            sink: Some(Arc::new(move |event| {
                events.send(event).unwrap();
            })),
        });
        let (encoding, encoding_rx) = mpsc::channel();
        let (encoded, encoded_rx) = mpsc::channel();
        let callback_session = Arc::clone(&session);
        let callback = std::thread::spawn(move || {
            assert!(!callback_session.finished.load(Ordering::Acquire));
            encoding.send(()).unwrap();
            // Stop while the callback is preparing its PCM frame.
            encoded_rx.recv().unwrap();
            callback_session.emit_frame(AudioEvent::Frame {
                capture_id: callback_session.id.clone(),
                sample_rate: 48000,
                channels: 2,
                timestamp_us: 0,
                samples_base64: String::new(),
            });
        });
        encoding_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(session.finish("stopped", None));
        encoded.send(()).unwrap();
        callback.join().unwrap();
        assert!(matches!(events_rx.try_recv(), Ok(AudioEvent::End { .. })));
        assert!(matches!(
            events_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn error_after_ready_removes_session_and_emits_one_end() {
        let (sink, ends) = test_sink();
        let (created, created_rx) = mpsc::channel();
        let (disposed, disposed_rx) = mpsc::channel();
        let value = launch_capture(Some(sink), Duration::from_secs(1), move |session| {
            created.send(Arc::clone(&session)).unwrap();
            session.signals.send(CaptureSignal::Started).unwrap();
            Ok(Box::new(TestBackend {
                session,
                start_error: false,
                async_error: false,
                worker: Mutex::new(None),
                disposed,
            }))
        })
        .unwrap();
        let session = created_rx.recv().unwrap();
        session
            .signals
            .send(CaptureSignal::Error("device disconnected".into()))
            .unwrap();
        let _ = session.signals.send(CaptureSignal::Closed);
        disposed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!stop_capture(value["captureId"].as_str().unwrap()));
        assert_eq!(ends.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn timeout_during_constructor_never_starts_late_backend() {
        let (sink, ends) = test_sink();
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let (disposed, disposed_rx) = mpsc::channel();
        let (result, result_rx) = mpsc::channel();
        let caller = std::thread::spawn(move || {
            result
                .send(launch_capture(
                    Some(sink),
                    Duration::from_millis(50),
                    move |session| {
                        entered.send(Arc::clone(&session)).unwrap();
                        release_rx.recv().unwrap();
                        // Starting this backend would produce a distinct error.
                        Ok(Box::new(TestBackend {
                            session,
                            start_error: true,
                            async_error: false,
                            worker: Mutex::new(None),
                            disposed,
                        }))
                    },
                ))
                .unwrap();
        });
        let session = entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            result_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap_err()
                .contains("timed out")
        );
        assert!(!ACTIVE_CAPTURES.read().unwrap().contains_key(&session.id));
        release.send(()).unwrap();
        disposed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(ends.load(Ordering::SeqCst), 1);
        caller.join().unwrap();
    }

    #[test]
    fn test_audio_availability() {
        let avail = get_availability();
        assert_eq!(avail["available"], cfg!(windows));
        assert_eq!(
            avail["backend"],
            if cfg!(windows) {
                "windows-wasapi-loopback"
            } else {
                "none"
            }
        );
        assert_eq!(avail["capabilities"]["process"], cfg!(windows));
        assert_eq!(avail["capabilities"]["system"], cfg!(windows));
    }

    #[test]
    fn test_resolve_root_pid() {
        assert_eq!(resolve_root_pid("window:4567:0"), Some(4567));
        assert_eq!(resolve_root_pid("window:9999"), Some(9999));
        assert_eq!(resolve_root_pid("1234"), Some(1234));
        assert_eq!(resolve_root_pid("screen:0:0"), None);
        assert_eq!(resolve_root_pid(""), None);
    }

    #[cfg(windows)]
    #[test]
    fn capture_start_rejects_invalid_endpoints_and_self_capture() {
        let error = start_capture(&json!({
            "winCaptureScope": "session-mixer",
            "winEndpoints": []
        }))
        .expect_err("empty endpoint selection must fail");
        assert!(error.contains("invalid endpoints"), "{error}");

        let error = start_capture(&json!({
            "winCaptureScope": "process",
            "targetPid": std::process::id()
        }))
        .expect_err("capturing the host process must fail");
        assert!(error.contains("own process tree"), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn test_capture_lifecycle() {
        let res =
            start_capture(&json!({ "winCaptureScope": "session-mixer" })).expect("start capture");
        let cid = res["captureId"].as_str().expect("captureId string");
        assert!(cid.starts_with("audio-capture-"));
        assert_eq!(res["sampleRate"], 48000);
        assert_eq!(res["channels"], 2);

        assert!(stop_capture(cid));
        assert!(!stop_capture(cid));
    }

    #[test]
    fn test_list_playback_devices() {
        let devices = list_playback_devices();
        assert!(!devices.is_empty());
        assert!(devices[0].get("id").is_some());
        assert!(devices[0].get("name").is_some());
    }

    #[test]
    fn test_audio_manager_wrapper() {
        let event_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = Arc::clone(&event_count);
        let manager = AudioManager::new(Arc::new(move |_evt| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let avail = manager.get_availability();
        assert_eq!(avail["available"], cfg!(windows));
        #[cfg(not(windows))]
        {
            assert!(
                manager
                    .start_capture(&json!({}))
                    .unwrap_err()
                    .contains("unavailable")
            );
            assert!(!manager.stop_capture("missing"));
        }
        #[cfg(windows)]
        {
            let res = manager
                .start_capture(&json!({ "winCaptureScope": "session-mixer" }))
                .expect("start capture");
            let cid = res["captureId"].as_str().unwrap();
            assert!(manager.stop_capture(cid));
        }
    }
}
