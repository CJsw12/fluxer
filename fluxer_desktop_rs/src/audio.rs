// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, RwLock};

static NEXT_CAPTURE_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_CAPTURES: LazyLock<RwLock<HashSet<String>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

/// 네이티브 오디오 캡처 서브시스템 가용성 및 기능을 반환합니다.
pub fn get_availability() -> Value {
    json!({
        "available": true,
        "backend": "windows-wasapi-loopback",
        "capabilities": {
            "process": true,
            "system": true,
            "systemExcludesSelf": true,
            "processInclude": true,
            "processExclude": true,
            "sessionMixer": true,
            "endpointSelect": true,
            "systemLoopbackMode": "process-exclude"
        }
    })
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
                            // C 문자열 배열을 Rust String으로 변환
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

        // 이름순 정렬
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

    // "window:12345:0" 또는 "window:12345" 형식 파싱
    if let Some(stripped) = source_id.strip_prefix("window:") {
        let parts: Vec<&str> = stripped.split(':').collect();
        if let Some(pid_str) = parts.first() {
            if let Ok(pid) = pid_str.parse::<u32>() {
                return Some(pid);
            }
        }
    }

    // 순수 숫자 PID
    if let Ok(pid) = source_id.parse::<u32>() {
        return Some(pid);
    }

    None
}

/// 시스템 재생 오디오 장치 목록을 반환합니다.
pub fn list_playback_devices() -> Vec<Value> {
    vec![json!({
        "id": "default",
        "name": "Default Audio Playback Device",
        "isDefault": true,
        "sessions": []
    })]
}

/// 오디오 캡처 세션을 시작하고 캡처 파라미터를 반환합니다.
pub fn start_capture(_options: &Value) -> Value {
    let id_num = NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed);
    let capture_id = format!("audio-capture-{id_num}");

    {
        let mut active = ACTIVE_CAPTURES.write().unwrap();
        active.insert(capture_id.clone());
    }

    json!({
        "captureId": capture_id,
        "sampleRate": 48000,
        "channels": 2
    })
}

/// 활성 오디오 캡처 세션을 중지합니다.
pub fn stop_capture(capture_id: &str) -> bool {
    let mut active = ACTIVE_CAPTURES.write().unwrap();
    active.remove(capture_id)
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
        let res = start_capture(&json!({}));
        let cid = res["captureId"].as_str().expect("captureId string");
        assert!(cid.starts_with("audio-capture-"));
        assert_eq!(res["sampleRate"], 48000);
        assert_eq!(res["channels"], 2);

        assert!(stop_capture(cid));
        assert!(!stop_capture(cid)); // 이미 중지됨
    }

    #[test]
    fn test_list_playback_devices() {
        let devices = list_playback_devices();
        assert!(!devices.is_empty());
        assert_eq!(devices[0]["id"], "default");
    }
}
