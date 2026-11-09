// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

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

#[cfg(target_os = "windows")]
static ACTIVE_CAPTURES: LazyLock<RwLock<HashMap<String, Arc<fluxer_win_process_loopback::ProcessLoopback>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[cfg(not(target_os = "windows"))]
static ACTIVE_CAPTURES: LazyLock<RwLock<HashMap<String, ()>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

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

    pub fn start_capture(&self, options: &Value) -> Value {
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
                            let name_bytes: Vec<u8> =
                                entry.szExeFile[..name_len].iter().map(|&c| c as u8).collect();
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
            && let Some(raw_pid) = stripped.split(':').next().and_then(|s| s.parse::<u32>().ok())
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
            && let Some(pid) = stripped.split(':').next().and_then(|s| s.parse::<u32>().ok())
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
pub fn start_capture(options: &Value) -> Value {
    start_capture_with_sink(options, None)
}

/// 오디오 캡처 세션을 sink와 함께 시작합니다.
pub fn start_capture_with_sink(options: &Value, sink_opt: Option<&AudioEventSink>) -> Value {
    let id_num = NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed);
    let capture_id = format!("audio-capture-{id_num}");

    let target_pid = options.get("targetPid").and_then(Value::as_u64).map(|v| v as u32);
    let include_tree = options.get("includeProcessTree").and_then(Value::as_bool).unwrap_or(true);
    let win_capture_scope = options
        .get("winCaptureScope")
        .and_then(Value::as_str)
        .unwrap_or(if target_pid.is_some() { "process" } else { "session-mixer" });
    let endpoints = options.get("winEndpoints").and_then(Value::as_array).map(|arr| {
        arr.iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>()
    });

    #[cfg(target_os = "windows")]
    {
        let is_system = win_capture_scope == "system" || win_capture_scope == "session-mixer";
        let target_pid_for_capture = if is_system {
            std::process::id()
        } else {
            target_pid.unwrap_or_else(std::process::id)
        };
        let include_process_tree = if is_system { false } else { include_tree };

        let sink = sink_opt.cloned().or_else(|| GLOBAL_AUDIO_SINK.read().unwrap().clone());
        let on_frame: fluxer_win_process_loopback::RawFrameSink = {
            let sink_clone = sink.clone();
            let cid = capture_id.clone();
            Arc::new(move |samples: &[f32], sample_rate: u32, channels: u32, timestamp_us: i64| {
                if let Some(ref s) = sink_clone {
                    let byte_slice = unsafe {
                        std::slice::from_raw_parts(
                            samples.as_ptr() as *const u8,
                            std::mem::size_of_val(samples),
                        )
                    };
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(byte_slice);
                    s(AudioEvent::Frame {
                        capture_id: cid.clone(),
                        sample_rate,
                        channels,
                        timestamp_us,
                        samples_base64: b64,
                    });
                }
            })
        };

        let on_error: fluxer_win_process_loopback::RawErrorSink = {
            let sink_clone = sink.clone();
            let cid = capture_id.clone();
            Arc::new(move |detail: String| {
                if let Some(ref s) = sink_clone {
                    s(AudioEvent::End {
                        capture_id: cid.clone(),
                        reason: "addon-error".to_string(),
                        detail: Some(detail),
                    });
                }
            })
        };

        let on_closed: fluxer_win_process_loopback::RawVoidSink = {
            let sink_clone = sink.clone();
            let cid = capture_id.clone();
            Arc::new(move || {
                if let Some(ref s) = sink_clone {
                    s(AudioEvent::End {
                        capture_id: cid.clone(),
                        reason: "stopped".to_string(),
                        detail: None,
                    });
                }
            })
        };

        match fluxer_win_process_loopback::ProcessLoopback::new_with_sink(
            target_pid_for_capture,
            include_process_tree,
            win_capture_scope,
            endpoints,
            on_frame,
            on_error,
            on_closed,
            None,
        ) {
            Ok(loopback) => {
                if let Err(e) = loopback.start() {
                    eprintln!("[Fluxer Audio] Failed to start WASAPI loopback: {e}");
                }
                ACTIVE_CAPTURES.write().unwrap().insert(capture_id.clone(), Arc::new(loopback));
            }
            Err(e) => {
                eprintln!("[Fluxer Audio] Failed to initialize WASAPI loopback: {e}");
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        ACTIVE_CAPTURES.write().unwrap().insert(capture_id.clone(), ());
    }

    json!({
        "captureId": capture_id,
        "sampleRate": 48000,
        "channels": 2
    })
}

/// 활성 오디오 캡처 세션을 중지합니다.
pub fn stop_capture(capture_id: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Some(loopback) = ACTIVE_CAPTURES.write().unwrap().remove(capture_id) {
            loopback.stop();
            loopback.dispose();
            return true;
        }
        false
    }

    #[cfg(not(target_os = "windows"))]
    {
        ACTIVE_CAPTURES.write().unwrap().remove(capture_id).is_some()
    }
}

/// 활성 Windows 오디오 세션의 엔드포인트를 동적으로 변경합니다.
pub fn set_win_endpoints(capture_id: &str, endpoints: Vec<String>) -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Some(loopback) = ACTIVE_CAPTURES.read().unwrap().get(capture_id) {
            return loopback.set_endpoints(endpoints).unwrap_or(false);
        }
        false
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (capture_id, endpoints);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_availability() {
        let avail = get_availability();
        assert_eq!(avail["available"], true);
        assert_eq!(avail["backend"], "windows-wasapi-loopback");
        assert_eq!(avail["capabilities"]["process"], true);
        assert_eq!(avail["capabilities"]["system"], true);
    }

    #[test]
    fn test_resolve_root_pid() {
        assert_eq!(resolve_root_pid("window:4567:0"), Some(4567));
        assert_eq!(resolve_root_pid("window:9999"), Some(9999));
        assert_eq!(resolve_root_pid("1234"), Some(1234));
        assert_eq!(resolve_root_pid("screen:0:0"), None);
        assert_eq!(resolve_root_pid(""), None);
    }

    #[test]
    fn test_capture_lifecycle() {
        let res = start_capture(&json!({ "winCaptureScope": "session-mixer" }));
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
        assert_eq!(avail["available"], true);

        let res = manager.start_capture(&json!({ "winCaptureScope": "session-mixer" }));
        let cid = res["captureId"].as_str().unwrap();
        assert!(manager.stop_capture(cid));
    }
}
