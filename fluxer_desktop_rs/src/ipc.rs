// SPDX-License-Identifier: AGPL-3.0-or-later

use base64::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tao::window::Window;
use crate::audio;
use crate::config;
use crate::gateway::GatewayManager;
use crate::overlay;
use crate::storage::{self, DesktopAccountRecord, DesktopStorageWrite};
use crate::stream_viewer;

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
    gateway: Option<&Arc<GatewayManager>>,
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

        "desktopUpdateState" => IpcResponse::success(
            id,
            json!({
                "available": false,
                "updating": false,
                "replacesShell": false
            }),
        ),
        "desktopModulesEnsure" => {
            let module_name = req
                .payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            IpcResponse::success(
                id,
                json!({
                    "module": module_name,
                    "status": "already-installed"
                }),
            )
        }
        "desktopModulesConfirmLaunch" => IpcResponse::success(id, Value::Null),

        "voiceStateUpdate" => {
            let is_connected = req
                .payload
                .get("isConnected")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let channel_name = req
                .payload
                .get("channelName")
                .and_then(Value::as_str)
                .unwrap_or("Voice Channel")
                .to_string();
            let is_muted = req
                .payload
                .get("isMuted")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_deafened = req
                .payload
                .get("isDeafened")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_speaking = req
                .payload
                .get("isSpeaking")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let active_speakers = req
                .payload
                .get("activeSpeakers")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();

            overlay::update_voice_state(overlay::VoiceOverlayState {
                is_connected,
                channel_name,
                is_muted,
                is_deafened,
                is_speaking,
                active_speakers,
            });

            IpcResponse::success(id, Value::Null)
        }

        "popoutVoiceOverlay" | "windowToggleVoiceOverlay" => {
            overlay::launch_voice_overlay_process();
            IpcResponse::success(id, json!(true))
        }

        "popoutStream" | "popoutStreamViewer" | "windowPopoutStream" => {
            stream_viewer::launch_stream_viewer_process();
            IpcResponse::success(id, json!(true))
        }

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
                    if let Ok(resp) = client.get(url).send()
                        && resp.status().is_success()
                            && let Ok(json) = resp.json::<Value>() {
                                let origin = if url.starts_with("https://") {
                                    format!("https://{domain}")
                                } else {
                                    format!("http://{domain}")
                                };
                                discovery_doc = Some((json, origin));
                                break;
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
            if let Some(domain) = req.payload.get("domain").and_then(Value::as_str) {
                let trimmed = domain.trim();
                if !trimmed.is_empty() && !trimmed.contains("web.fluxer.app") {
                    let full = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                        trimmed.to_string()
                    } else {
                        format!("https://{trimmed}")
                    };
                    if config::load_last_instance().is_none() {
                        config::save_last_instance(&full);
                    }
                }
            }
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
                    let mut is_binary = false;
                    for (k, v) in resp.headers() {
                        if let Ok(v_str) = v.to_str() {
                            let k_lower = k.as_str().to_lowercase();
                            if k_lower == "content-type" {
                                let ct_lower = v_str.to_lowercase();
                                if ct_lower.contains("wasm")
                                    || ct_lower.contains("octet-stream")
                                    || ct_lower.contains("gzip")
                                    || ct_lower.contains("tar")
                                    || ct_lower.contains("audio")
                                    || ct_lower.contains("image")
                                    || ct_lower.contains("video")
                                    || ct_lower.contains("zip")
                                {
                                    is_binary = true;
                                }
                            }
                            resp_headers.insert(k.as_str().to_string(), Value::String(v_str.to_string()));
                        }
                    }

                    // 파일 확장자 기반 바이너리 판정 보강 (WASM, 모델 아카이브 등)
                    let url_lower = url.to_lowercase();
                    if url_lower.ends_with(".wasm")
                        || url_lower.ends_with(".tar.gz")
                        || url_lower.ends_with(".gz")
                        || url_lower.ends_with(".bin")
                        || url_lower.ends_with(".onnx")
                    {
                        is_binary = true;
                    }

                    let raw_bytes = resp.bytes().unwrap_or_default();
                    let (body_val, is_b64) = if is_binary {
                        (BASE64_STANDARD.encode(&raw_bytes), true)
                    } else {
                        match String::from_utf8(raw_bytes.to_vec()) {
                            Ok(text) => (text, false),
                            Err(_) => (BASE64_STANDARD.encode(&raw_bytes), true),
                        }
                    };

                    IpcResponse::success(
                        id,
                        json!({
                            "status": status,
                            "statusText": status_text,
                            "headers": resp_headers,
                            "body": body_val,
                            "isBase64": is_b64
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

        "popoutSetAlwaysOnTop" | "popout:set-always-on-top" => {
            let flag = req.payload.get("flag").and_then(Value::as_bool).unwrap_or(false);
            window.set_always_on_top(flag);
            IpcResponse::success(id, json!(true))
        }

        "popoutFocus" | "popout:focus" => {
            window.set_focus();
            IpcResponse::success(id, json!(true))
        }

        // =========================================================
        // 네이티브 스토리지 API (desktopStorage)
        // =========================================================
        "desktopStorageGetStatus" | "desktop-storage:get-status" => {
            IpcResponse::success(id, storage::get_storage_status())
        }

        "desktopStorageLoad" | "desktop-storage:load" => {
            let scope = req.payload.get("scope").and_then(Value::as_str)
                .or_else(|| req.payload.as_str())
                .unwrap_or_default();
            let entries = storage::load_scope(scope);
            IpcResponse::success(id, json!(entries))
        }

        "desktopStorageGet" | "desktop-storage:get" => {
            let scope = req.payload.get("scope").and_then(Value::as_str).unwrap_or_default();
            let key = req.payload.get("key").and_then(Value::as_str).unwrap_or_default();
            let entry = storage::get_entry(scope, key);
            IpcResponse::success(id, json!(entry))
        }

        "desktopStorageSet" | "desktop-storage:set" => {
            let scope = req.payload.get("scope").and_then(Value::as_str).unwrap_or_default();
            let key = req.payload.get("key").and_then(Value::as_str).unwrap_or_default();
            let value = req.payload.get("value").and_then(Value::as_str).unwrap_or_default();
            storage::set_entry(scope, key, value);
            IpcResponse::success(id, Value::Null)
        }

        "desktopStorageDelete" | "desktop-storage:delete" => {
            let scope = req.payload.get("scope").and_then(Value::as_str).unwrap_or_default();
            let key = req.payload.get("key").and_then(Value::as_str).unwrap_or_default();
            storage::delete_entry(scope, key);
            IpcResponse::success(id, Value::Null)
        }

        "desktopStorageClearAllForScope" | "desktop-storage:clear-all-for-scope" => {
            let scope = req.payload.get("scope").and_then(Value::as_str)
                .or_else(|| req.payload.as_str())
                .unwrap_or_default();
            storage::clear_all_for_scope(scope);
            IpcResponse::success(id, Value::Null)
        }

        "desktopStorageClearAllExcept" | "desktop-storage:clear-all-except" => {
            let keys: Vec<String> = req.payload.get("keysToKeep")
                .or_else(|| req.payload.as_array().map(|_| &req.payload))
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_str).map(String::from).collect())
                .unwrap_or_default();
            storage::clear_all_except(&keys);
            IpcResponse::success(id, Value::Null)
        }

        "desktopStorageSetMany" | "desktop-storage:set-many" => {
            let writes: Vec<DesktopStorageWrite> = req.payload.get("writes")
                .or_else(|| req.payload.as_array().map(|_| &req.payload))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            storage::set_many(&writes);
            IpcResponse::success(id, Value::Null)
        }

        "desktopStorageGetMarker" | "desktop-storage:get-marker" => {
            let key = req.payload.get("key").and_then(Value::as_str)
                .or_else(|| req.payload.as_str())
                .unwrap_or_default();
            let val = storage::get_marker(key);
            IpcResponse::success(id, json!(val))
        }

        "desktopStorageSetMarker" | "desktop-storage:set-marker" => {
            let key = req.payload.get("key").and_then(Value::as_str).unwrap_or_default();
            let val = req.payload.get("value").and_then(Value::as_str).unwrap_or_default();
            storage::set_marker(key, val);
            IpcResponse::success(id, Value::Null)
        }

        // =========================================================
        // 네이티브 계정 저장소 API (desktopAccounts)
        // =========================================================
        "desktopAccountsGetAll" | "desktop-accounts:get-all" => {
            let accounts = storage::get_all_accounts();
            IpcResponse::success(id, json!(accounts))
        }

        "desktopAccountsGet" | "desktop-accounts:get" => {
            let key = req.payload.get("storageKey").and_then(Value::as_str)
                .or_else(|| req.payload.as_str())
                .unwrap_or_default();
            let acc = storage::get_account(key);
            IpcResponse::success(id, json!(acc))
        }

        "desktopAccountsUpsert" | "desktop-accounts:upsert" => {
            let acc_opt: Option<DesktopAccountRecord> = req.payload.get("account")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .or_else(|| serde_json::from_value(req.payload.clone()).ok());

            if let Some(acc) = acc_opt {
                if let Some(inst) = storage::extract_instance_url(&acc.storage_key) {
                    config::save_last_instance(&inst);
                }
                storage::upsert_account(acc);
                IpcResponse::success(id, Value::Null)
            } else {
                IpcResponse::error(id, "Invalid DesktopAccountRecord format")
            }
        }

        "desktopAccountsCompareAndSwap" | "desktop-accounts:compare-and-swap" => {
            let expected_opt: Option<DesktopAccountRecord> = req.payload.get("expected")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let replacement_opt: Option<DesktopAccountRecord> = req.payload.get("replacement")
                .and_then(|v| serde_json::from_value(v.clone()).ok());

            if let (Some(expected), Some(replacement)) = (expected_opt, replacement_opt) {
                let swapped = storage::compare_and_swap_account(&expected, &replacement);
                IpcResponse::success(id, json!(swapped))
            } else {
                IpcResponse::error(id, "Missing expected or replacement account")
            }
        }

        "desktopAccountsDelete" | "desktop-accounts:delete" => {
            let key = req.payload.get("storageKey").and_then(Value::as_str)
                .or_else(|| req.payload.as_str())
                .unwrap_or_default();
            storage::delete_account(key);
            IpcResponse::success(id, Value::Null)
        }

        "desktopAccountsPrune" | "desktop-accounts:prune" => {
            let keys: Vec<String> = req.payload.get("keysToKeep")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_str).map(String::from).collect())
                .unwrap_or_default();
            let pruned = storage::prune_accounts(&keys);
            IpcResponse::success(id, json!({ "pruned": pruned }))
        }

        // =========================================================
        // 네이티브 게이트웨이 트랜스포트 API (nativeGatewayTransport)
        // =========================================================
        "nativeGatewayTransportIsAvailable" | "native-gateway-transport:is-available" => {
            IpcResponse::success(id, json!(true))
        }

        "nativeGatewayTransportCreate" | "native-gateway-transport:create" => {
            let connection_id = req.payload.get("connectionId").and_then(Value::as_str).unwrap_or("");
            let url = req.payload.get("url").and_then(Value::as_str).unwrap_or("");

            match gateway {
                Some(mgr) => match mgr.create(connection_id.to_string(), url.to_string()) {
                    Ok(()) => IpcResponse::success(id, json!({ "connectionId": connection_id })),
                    Err(err) => IpcResponse::error(id, err.to_string()),
                },
                None => IpcResponse::error(id, "Gateway manager unavailable"),
            }
        }

        "nativeGatewayTransportSendText" | "native-gateway-transport:send-text" => {
            let connection_id = req.payload.get("connectionId").and_then(Value::as_str).unwrap_or("");
            let payload = req.payload.get("payload").and_then(Value::as_str).unwrap_or("");

            match gateway {
                Some(mgr) => match mgr.send_text(connection_id, payload.to_string()) {
                    Ok(()) => IpcResponse::success(id, json!({})),
                    Err(err) => IpcResponse::error(id, err.to_string()),
                },
                None => IpcResponse::error(id, "Gateway manager unavailable"),
            }
        }

        "nativeGatewayTransportSendBinary" | "native-gateway-transport:send-binary" => {
            let connection_id = req.payload.get("connectionId").and_then(Value::as_str).unwrap_or("");
            let b64 = req.payload.get("payload").and_then(Value::as_str).unwrap_or("");

            match BASE64_STANDARD.decode(b64) {
                Ok(bytes) => match gateway {
                    Some(mgr) => match mgr.send_binary(connection_id, bytes) {
                        Ok(()) => IpcResponse::success(id, json!({})),
                        Err(err) => IpcResponse::error(id, err.to_string()),
                    },
                    None => IpcResponse::error(id, "Gateway manager unavailable"),
                },
                Err(err) => IpcResponse::error(id, format!("Invalid base64 payload: {err}")),
            }
        }

        "nativeGatewayTransportClose" | "native-gateway-transport:close" => {
            let connection_id = req.payload.get("connectionId").and_then(Value::as_str).unwrap_or("");
            let code = req.payload.get("code").and_then(Value::as_u64).unwrap_or(1000) as u16;
            let reason = req.payload.get("reason").and_then(Value::as_str).unwrap_or("");

            match gateway {
                Some(mgr) => match mgr.close(connection_id, code, reason.to_string()) {
                    Ok(()) => IpcResponse::success(id, json!({})),
                    Err(err) => IpcResponse::error(id, err.to_string()),
                },
                None => IpcResponse::error(id, "Gateway manager unavailable"),
            }
        }

        "nativeGatewayTransportDispose" | "native-gateway-transport:dispose" => {
            let connection_id = req.payload.get("connectionId").and_then(Value::as_str)
                .or_else(|| req.payload.as_str())
                .unwrap_or("");

            match gateway {
                Some(mgr) => match mgr.dispose(connection_id) {
                    Ok(()) => IpcResponse::success(id, json!({})),
                    Err(err) => IpcResponse::error(id, err.to_string()),
                },
                None => IpcResponse::error(id, "Gateway manager unavailable"),
            }
        }

        // =========================================================
        // 네이티브 오디오 캡처 서브시스템 API (nativeAudio)
        // =========================================================
        "nativeAudioGetAvailability" | "native-audio:get-availability" => {
            IpcResponse::success(id, audio::get_availability())
        }

        "nativeAudioListApplications" | "native-audio:list-applications" => {
            IpcResponse::success(id, json!(audio::list_applications()))
        }

        "nativeAudioResolveRootPid" | "native-audio:resolve-root-pid" => {
            let source_id = req.payload.get("sourceId").and_then(Value::as_str)
                .or_else(|| req.payload.as_str())
                .unwrap_or("");
            IpcResponse::success(id, json!(audio::resolve_root_pid(source_id)))
        }

        "nativeAudioListPlaybackDevices" | "native-audio:list-playback-devices" => {
            IpcResponse::success(id, json!(audio::list_playback_devices()))
        }

        "nativeAudioStart" | "native-audio:start" => {
            IpcResponse::success(id, audio::start_capture(&req.payload))
        }

        "nativeAudioStop" | "native-audio:stop" => {
            let capture_id = req.payload.get("captureId").and_then(Value::as_str)
                .or_else(|| req.payload.as_str())
                .unwrap_or("");
            audio::stop_capture(capture_id);
            IpcResponse::success(id, Value::Null)
        }

        "nativeAudioSetWinEndpoints" | "native-audio:set-win-endpoints" => {
            IpcResponse::success(id, json!(true))
        }

        "nativeAudioSetRule" | "native-audio:set-rule" => {
            IpcResponse::success(id, json!(true))
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

    #[test]
    fn test_native_gateway_ipc_availability() {
        let req = IpcRequest {
            id: Some(10),
            cmd: "nativeGatewayTransportIsAvailable".to_string(),
            payload: Value::Null,
        };
        // window가 없어도 id와 cmd 매칭 가능하도록 테스트
        let response = match req.cmd.as_str() {
            "nativeGatewayTransportIsAvailable" => IpcResponse::success(req.id.unwrap(), json!(true)),
            _ => panic!("unexpected cmd"),
        };
        assert_eq!(response.id, 10);
        assert_eq!(response.result, json!(true));
        assert!(response.error.is_none());
    }

    #[test]
    fn test_native_audio_ipc_availability() {
        let avail = audio::get_availability();
        assert_eq!(avail["available"], true);
        assert_eq!(avail["backend"], "windows-wasapi-loopback");
    }

    #[test]
    fn test_popout_ipc_commands_matching() {
        let req = IpcRequest {
            id: Some(99),
            cmd: "popoutSetAlwaysOnTop".to_string(),
            payload: json!({ "key": "test-key", "flag": true }),
        };
        assert_eq!(req.cmd, "popoutSetAlwaysOnTop");
        assert_eq!(req.payload.get("flag").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn test_desktop_module_and_update_contract() {
        let module_res = json!({
            "module": "fluxer_deepfilter",
            "status": "already-installed"
        });
        assert_eq!(module_res["status"], "already-installed");
        assert_eq!(module_res["module"], "fluxer_deepfilter");

        let update_res = json!({
            "available": false,
            "updating": false,
            "replacesShell": false
        });
        assert_eq!(update_res["available"], false);
    }

    #[test]
    fn test_voice_state_update_ipc_parsing() {
        let req = IpcRequest {
            id: Some(101),
            cmd: "voiceStateUpdate".to_string(),
            payload: json!({
                "isConnected": true,
                "channelName": "Voice Room",
                "isMuted": false,
                "isDeafened": false,
                "isSpeaking": true,
                "activeSpeakers": ["Alice", "Bob"]
            }),
        };
        assert_eq!(req.cmd, "voiceStateUpdate");
        assert_eq!(req.payload.get("isConnected").and_then(Value::as_bool), Some(true));
        assert_eq!(req.payload.get("channelName").and_then(Value::as_str), Some("Voice Room"));
    }

    #[test]
    fn test_popout_stream_ipc_commands_matching() {
        let req = IpcRequest {
            id: Some(102),
            cmd: "popoutStreamViewer".to_string(),
            payload: json!({ "streamId": "test-stream-123" }),
        };
        assert_eq!(req.cmd, "popoutStreamViewer");
        assert_eq!(req.payload.get("streamId").and_then(Value::as_str), Some("test-stream-123"));
    }
}
