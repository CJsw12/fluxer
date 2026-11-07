// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tao::window::Window;
use crate::config;

static MINIMIZE_TO_TRAY: AtomicBool = AtomicBool::new(false);
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: Option<u64>,
    pub cmd: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: u64,
    pub result: Value,
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn success(id: u64, result: Value) -> Self {
        Self {
            id,
            result,
            error: None,
        }
    }

    pub fn error(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            result: Value::Null,
            error: Some(error.into()),
        }
    }
}

/// 들어온 IPC 요청을 해석하고 처리 결과를 반환합니다.
pub fn handle_ipc_request(
    req: IpcRequest,
    window: &Arc<Window>,
    cli_instance: Option<&str>,
) -> Option<IpcResponse> {
    let id = req.id?;

    let response = match req.cmd.as_str() {
        "getDesktopInfo" => IpcResponse::success(
            id,
            json!({
                "platform": "win32",
                "arch": std::env::consts::ARCH,
                "version": "1.0.0-rust",
                "release": "10.0.22631",
                "nativeNotifications": true,
                "waylandSession": false,
                "rustShell": true
            }),
        ),

        "getGpuInfo" => IpcResponse::success(
            id,
            json!({
                "devices": [{
                    "active": true,
                    "vendorId": 0x10de,
                    "deviceId": 0x1234,
                    "deviceString": "Direct3D 11 / WebView2 Hardware Accelerated",
                    "source": "dxgi"
                }]
            }),
        ),

        "getAppMetrics" => {
            let pid = std::process::id();
            IpcResponse::success(
                id,
                json!({
                    "cpuInfo": {
                        "model": "Host Processor",
                        "speed": 3200,
                        "cores": 8,
                        "physicalCores": 4
                    },
                    "processes": [{
                        "pid": pid,
                        "type": "RustShell",
                        "cpu": { "percentCPUUsage": 0.2 },
                        "memory": {
                            "workingSetSize": 28_000_000,
                            "peakWorkingSetSize": 45_000_000
                        }
                    }],
                    "totalMemoryMB": 16384,
                    "freeMemoryMB": 8192
                }),
            )
        }

        "systemIdleTimeMs" => IpcResponse::success(id, json!(0)),

        "desktopWindowBehaviorGet" => IpcResponse::success(
            id,
            json!({
                "showTrayIcon": true,
                "coloredTrayIcon": false,
                "darkTaskbarBadge": false,
                "minimizeToTray": MINIMIZE_TO_TRAY.load(Ordering::Relaxed),
                "closeToTray": CLOSE_TO_TRAY.load(Ordering::Relaxed),
                "useNativeTitleBar": false,
                "activeUseNativeTitleBar": false,
                "rememberWindowState": true,
                "allowTransparency": false,
                "activeAllowTransparency": false,
                "smoothScrolling": true,
                "activeSmoothScrolling": true,
                "middleClickAutoscroll": true,
                "activeMiddleClickAutoscroll": true
            }),
        ),

        "desktopWindowBehaviorSet" => {
            if let Some(minimize_to_tray) = req.payload.get("minimizeToTray").and_then(Value::as_bool) {
                MINIMIZE_TO_TRAY.store(minimize_to_tray, Ordering::Relaxed);
            }
            if let Some(close_to_tray) = req.payload.get("closeToTray").and_then(Value::as_bool) {
                CLOSE_TO_TRAY.store(close_to_tray, Ordering::Relaxed);
            }
            IpcResponse::success(
                id,
                json!({
                    "showTrayIcon": true,
                    "minimizeToTray": MINIMIZE_TO_TRAY.load(Ordering::Relaxed),
                    "closeToTray": CLOSE_TO_TRAY.load(Ordering::Relaxed),
                    "useNativeTitleBar": false,
                    "activeUseNativeTitleBar": false,
                    "allowTransparency": false,
                    "activeAllowTransparency": false,
                    "rememberWindowState": true
                }),
            )
        }

        "desktopWindowBehaviorPendingRestart" => IpcResponse::success(id, json!(false)),

        "openExternal" => {
            let url_opt = req
                .payload
                .get("url")
                .and_then(Value::as_str)
                .or_else(|| req.payload.as_str());

            if let Some(url) = url_opt {
                match open::that(url) {
                    Ok(_) => IpcResponse::success(id, Value::Null),
                    Err(err) => IpcResponse::error(id, format!("Failed to open external url: {err}")),
                }
            } else {
                IpcResponse::error(id, "Missing 'url' parameter")
            }
        }

        "clipboardWriteText" => {
            let text_opt = req
                .payload
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| req.payload.as_str());

            if let Some(text) = text_opt {
                match arboard::Clipboard::new() {
                    Ok(mut clipboard) => match clipboard.set_text(text) {
                        Ok(_) => IpcResponse::success(id, Value::Null),
                        Err(err) => IpcResponse::error(id, format!("Failed to write clipboard: {err}")),
                    },
                    Err(err) => IpcResponse::error(id, format!("Failed to access clipboard: {err}")),
                }
            } else {
                IpcResponse::error(id, "Missing 'text' parameter")
            }
        }

        "clipboardReadText" => match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                let text = clipboard.get_text().unwrap_or_default();
                IpcResponse::success(id, json!(text))
            }
            Err(err) => IpcResponse::error(id, format!("Failed to access clipboard: {err}")),
        },

        "checkMediaAccess" => IpcResponse::success(id, json!("granted")),
        "requestMediaAccess" => IpcResponse::success(id, json!(true)),

        "autostartIsEnabled" => IpcResponse::success(id, json!(false)),
        "autostartEnable" => IpcResponse::success(id, Value::Null),
        "autostartDisable" => IpcResponse::success(id, Value::Null),

        "passkeyIsSupported" => IpcResponse::success(id, json!(false)),

        "desktopUpdateState" => IpcResponse::success(id, json!({ "status": "idle" })),
        "desktopModulesEnsure" => IpcResponse::success(id, json!({ "status": "ready" })),

        "desktopRuntimeInitialInput" => {
            // CLI 인수로 명시적인 인스턴스가 지정된 경우에만 해당 값을 반환하고,
            // 인수가 없는 일반 실행 시에는 null을 반환하여 원본 공식 Fluxer 데스크톱 클라이언트와 100% 동일하게
            // "Choose your instance" 인스턴스 선택 화면(타일 목록 및 뒤로가기 버튼)을 안전하게 표출합니다.
            let initial = cli_instance.map(Value::from).unwrap_or(Value::Null);
            IpcResponse::success(id, initial)
        }

        "desktopRuntimeResolve" => {
            let input_raw = req
                .payload
                .get("input")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();

            if input_raw.is_empty() {
                IpcResponse::error(id, "Instance URL is required")
            } else {
                let domain = input_raw
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .trim_end_matches('/');

                let client = match reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(8))
                    .danger_accept_invalid_certs(true)
                    .build()
                {
                    Ok(c) => c,
                    Err(err) => return Some(IpcResponse::error(id, format!("Failed to create HTTP client: {err}"))),
                };

                let candidates = [
                    format!("https://{domain}/.well-known/fluxer"),
                    format!("https://{domain}/api/.well-known/fluxer"),
                    format!("http://{domain}/.well-known/fluxer"),
                    format!("http://{domain}/api/.well-known/fluxer"),
                ];

                let mut discovery_doc: Option<(Value, String)> = None;
                for url in &candidates {
                    if let Ok(resp) = client.get(url).send() {
                        if resp.status().is_success() {
                            if let Ok(text) = resp.text() {
                                if let Ok(json) = serde_json::from_str::<Value>(&text) {
                                    let origin = if url.starts_with("https://") {
                                        format!("https://{domain}")
                                    } else {
                                        format!("http://{domain}")
                                    };
                                    discovery_doc = Some((json, origin));
                                    break;
                                }
                            }
                        }
                    }
                }

                match discovery_doc {
                    Some((doc, origin)) => {
                        let declared_api = doc
                            .get("endpoints")
                            .and_then(|e| e.get("api"))
                            .and_then(Value::as_str)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("{origin}/api"));

                        let instance_key = declared_api.clone();
                        let api_endpoint = declared_api.clone();
                        let remote_api_endpoint = declared_api;

                        config::save_last_instance(&origin);

                        IpcResponse::success(
                            id,
                            json!({
                                "instanceKey": instance_key,
                                "apiEndpoint": api_endpoint,
                                "remoteApiEndpoint": remote_api_endpoint,
                                "document": doc
                            }),
                        )
                    }
                    None => IpcResponse::error(
                        id,
                        format!("Could not connect to Fluxer instance at '{domain}'. Check the address and that the instance is online."),
                    ),
                }
            }
        }

        "knownInstancesGetAll" => {
            let list = config::load_known_instances();
            IpcResponse::success(id, Value::Array(list))
        }

        "knownInstancesUpsert" => {
            config::upsert_known_instance(req.payload);
            IpcResponse::success(id, Value::Null)
        }

        "knownInstancesDelete" => {
            if let Some(key) = req.payload.get("instanceKey").and_then(Value::as_str) {
                config::delete_known_instance(key);
            }
            IpcResponse::success(id, Value::Null)
        }

        "fetchProxy" => {
            let url = req.payload.get("url").and_then(Value::as_str).unwrap_or_default();
            let method = req.payload.get("method").and_then(Value::as_str).unwrap_or("GET");
            let headers = req.payload.get("headers").and_then(Value::as_object);
            let body = req.payload.get("body").and_then(Value::as_str);

            let client = match reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(15))
                .danger_accept_invalid_certs(true)
                .build()
            {
                Ok(c) => c,
                Err(err) => return Some(IpcResponse::error(id, format!("HTTP client error: {err}"))),
            };

            let mut request_builder = match method.to_uppercase().as_str() {
                "POST" => client.post(url),
                "PUT" => client.put(url),
                "DELETE" => client.delete(url),
                "HEAD" => client.head(url),
                "PATCH" => client.patch(url),
                _ => client.get(url),
            };

            if let Some(h) = headers {
                for (k, v) in h {
                    let k_lower = k.to_lowercase();
                    if k_lower == "host" || k_lower == "content-length" {
                        continue;
                    }
                    if let Some(v_str) = v.as_str() {
                        request_builder = request_builder.header(k, v_str);
                    }
                }
            }

            if let Some(b) = body {
                request_builder = request_builder.body(b.to_string());
            }

            match request_builder.send() {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let status_text = resp.status().canonical_reason().unwrap_or("OK").to_string();
                    let mut resp_headers = serde_json::Map::new();
                    for (k, v) in resp.headers() {
                        if let Ok(v_str) = v.to_str() {
                            resp_headers.insert(k.as_str().to_string(), Value::String(v_str.to_string()));
                        }
                    }
                    let resp_body = resp.text().unwrap_or_default();

                    IpcResponse::success(
                        id,
                        json!({
                            "status": status,
                            "statusText": status_text,
                            "headers": resp_headers,
                            "body": resp_body
                        }),
                    )
                }
                Err(err) => IpcResponse::error(id, format!("Network request failed: {err}")),
            }
        }

        "windowIsMaximized" => {
            let is_max = window.is_maximized();
            IpcResponse::success(id, json!(is_max))
        }

        unknown => {
            eprintln!("[IPC Warning] Unhandled invoke channel: '{unknown}', returning safe null success");
            IpcResponse::success(id, Value::Null)
        }
    };

    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_envelope_parsing() {
        let json = r#"{"id":42,"cmd":"getDesktopInfo","payload":{}}"#;
        let req: IpcRequest = serde_json::from_str(json).expect("deserialize IpcRequest");
        assert_eq!(req.id, Some(42));
        assert_eq!(req.cmd, "getDesktopInfo");
    }

    #[test]
    fn test_ipc_response_serialization() {
        let res = IpcResponse::success(1, json!({"status": "ok"}));
        let serialized = serde_json::to_string(&res).expect("serialize response");
        assert!(serialized.contains(r#""id":1"#));
        assert!(serialized.contains(r#""status":"ok""#));
        assert!(serialized.contains(r#""error":null"#));
    }
}
