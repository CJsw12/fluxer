// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// 애플리케이션의 설정 및 마지막 접속 인스턴스 저장소 디렉터리를 가져옵니다.
/// Windows 표준 `%APPDATA%\fluxer_desktop_rs` 경로를 사용하여 Git 저장소 외부에 안전하게 격리합니다.
pub fn get_config_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = PathBuf::from(appdata).join("fluxer_desktop_rs");
        let _ = fs::create_dir_all(&dir);
        dir
    } else {
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(".fluxer_data");
        let _ = fs::create_dir_all(&dir);
        dir
    }
}

/// 마지막으로 사용한 인스턴스 URL 파일 경로를 반환합니다.
fn get_instance_file_path() -> PathBuf {
    get_config_dir().join("last_instance.txt")
}

/// 저장된 알려진 인스턴스 목록(JSON) 파일 경로를 반환합니다.
fn get_known_instances_file_path() -> PathBuf {
    get_config_dir().join("known_instances.json")
}

/// 저장된 최근 인스턴스 URL을 읽어옵니다.
#[allow(dead_code)]
pub fn load_last_instance() -> Option<String> {
    let path = get_instance_file_path();
    if let Ok(content) = fs::read_to_string(path) {
        let trimmed = content.trim();
        if !trimmed.is_empty() && (trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// 인스턴스 URL을 로컬에 영구 저장합니다.
pub fn save_last_instance(url: &str) {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return;
    }
    let full_url = if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    };

    let path = get_instance_file_path();
    if let Err(err) = fs::write(&path, full_url.as_bytes()) {
        eprintln!("[Fluxer Config] Failed to save last instance URL: {err:?}");
    } else {
        println!("[Fluxer Config] Saved active instance URL: {full_url}");
    }
}

/// 저장된 인스턴스 설정을 삭제하여 인스턴스 선택 화면으로 리셋합니다.
#[allow(dead_code)]
pub fn clear_last_instance() {
    let path = get_instance_file_path();
    let _ = fs::remove_file(path);
    println!("[Fluxer Config] Cleared active instance configuration.");
}

/// 로컬 PC(`%APPDATA%\fluxer_desktop_rs\known_instances.json`)에 저장된 외부 인스턴스 목록을 불러옵니다.
pub fn load_known_instances() -> Vec<Value> {
    let path = get_known_instances_file_path();
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&content) {
            return arr;
        }
    }
    Vec::new()
}

/// 외부 인스턴스 항목을 로컬 PC(`%APPDATA%\fluxer_desktop_rs\known_instances.json`)에 추가 또는 갱신합니다.
pub fn upsert_known_instance(record: Value) {
    let Some(instance_key) = record.get("instanceKey").and_then(Value::as_str) else {
        return;
    };
    let mut list = load_known_instances();
    list.retain(|item| item.get("instanceKey").and_then(Value::as_str) != Some(instance_key));
    list.push(record);
    let path = get_known_instances_file_path();
    if let Ok(serialized) = serde_json::to_string_pretty(&list) {
        let _ = fs::write(path, serialized.as_bytes());
    }
}

/// 외부 인스턴스 항목을 로컬 PC(`%APPDATA%\fluxer_desktop_rs\known_instances.json`)에서 삭제합니다.
pub fn delete_known_instance(instance_key: &str) {
    let mut list = load_known_instances();
    list.retain(|item| item.get("instanceKey").and_then(Value::as_str) != Some(instance_key));
    let path = get_known_instances_file_path();
    if let Ok(serialized) = serde_json::to_string_pretty(&list) {
        let _ = fs::write(path, serialized.as_bytes());
    }
}

/// 알려진 인스턴스 목록(`known_instances.json`) 중 가장 최근에 사용된 인스턴스 웹앱 URL을 반환합니다.
pub fn load_latest_known_instance_url() -> Option<String> {
    let instances = load_known_instances();
    if instances.is_empty() {
        return None;
    }

    let mut latest_record: Option<&Value> = None;
    let mut max_used: i64 = -1;

    for item in &instances {
        let last_used = item.get("lastUsed").and_then(Value::as_i64).unwrap_or(0);
        if last_used >= max_used {
            max_used = last_used;
            latest_record = Some(item);
        }
    }

    let record = latest_record.or_else(|| instances.last())?;

    // 1. domain 필드 추출
    if let Some(domain) = record.get("domain").and_then(Value::as_str) {
        let trimmed = domain.trim();
        if !trimmed.is_empty() {
            let full_url = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                trimmed.to_string()
            } else {
                format!("https://{trimmed}")
            };
            return Some(full_url);
        }
    }

    // 2. instanceKey 필드 추출 (예: "https://fluxer.itsuj.duckdns.org/api" -> "https://fluxer.itsuj.duckdns.org")
    if let Some(instance_key) = record.get("instanceKey").and_then(Value::as_str) {
        let trimmed = instance_key.trim();
        let (scheme, rest) = if let Some(stripped) = trimmed.strip_prefix("https://") {
            ("https", stripped)
        } else if let Some(stripped) = trimmed.strip_prefix("http://") {
            ("http", stripped)
        } else {
            ("https", trimmed)
        };
        let host = rest.split('/').next().unwrap_or(rest);
        if !host.is_empty() {
            return Some(format!("{scheme}://{host}"));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_dir_creation() {
        let dir = get_config_dir();
        assert!(dir.exists());
    }

    #[test]
    fn test_load_latest_known_instance_url() {
        // known_instances.json이 존재하지 않거나 빈 배열일 때의 안전성 검증
        let _ = load_latest_known_instance_url();
    }
}
