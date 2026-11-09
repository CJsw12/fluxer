// SPDX-License-Identifier: AGPL-3.0-or-later

#![recursion_limit = "512"]

use std::sync::Arc;
use std::time::Duration;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

mod assets;
mod audio;
mod bridge;
mod config;
pub mod contracts;
mod gateway;
mod ipc;
mod overlay;
mod shortcuts;
mod storage;
mod stream_viewer;
mod tray;

use assets::AssetHandler;
use ipc::{IpcRequest, IpcResponse};
use shortcuts::ShortcutManager;
use tray::{TrayManager, TrayMenuAction};
use tray_icon::menu::{MenuEvent, MenuId};

#[derive(Debug)]
enum UserEvent {
    IpcOneWay(IpcRequest),
    IpcReply(IpcResponse),
    GatewayEvent(gateway::NativeGatewayTransportEvent),
    TrayMenu(MenuId),
    TrayIconClick,
    GlobalShortcut(u32),
}

fn get_cli_launch_instance() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for (i, arg) in args.iter().enumerate().skip(1) {
        if arg == "--fluxer-instance" {
            if let Some(next) = args.get(i + 1).filter(|s| !s.starts_with("--")) {
                return Some(next.clone());
            }
        } else if let Some(stripped) = arg.strip_prefix("--fluxer-instance=") {
            return Some(stripped.to_string());
        } else if !arg.starts_with("--") && (arg.starts_with("http://") || arg.starts_with("https://") || arg.contains('.')) {
            return Some(arg.clone());
        }
    }
    None
}

fn load_app_icon() -> Option<tao::window::Icon> {
    const ICON_BYTES: &[u8] = include_bytes!("../resources/tray_32x32.png");
    if let Ok(img) = image::load_from_memory(ICON_BYTES) {
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        return tao::window::Icon::from_rgba(rgba.into_raw(), width, height).ok();
    }
    None
}

/// 현재 프로세스 및 모든 WebView2 자식/손자 프로세스(렌더러, GPU 프로세스 등)의 작업 세트를 전수 압축(Trimming)합니다.
#[cfg(target_os = "windows")]
pub fn trim_process_working_set() {
    unsafe {
        use std::collections::HashSet;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32, Process32First, Process32Next,
            TH32CS_SNAPPROCESS,
        };
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, GetCurrentProcessId, OpenProcess, SetProcessWorkingSetSize,
            PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA,
        };

        // 1. 메인 호스트 프로세스 자체 워킹셋 비우기
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);

        let my_pid = GetCurrentProcessId();
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return;
        }

        let mut child_pids: HashSet<u32> = HashSet::new();
        child_pids.insert(my_pid);

        // 2. 부모-자식 관계를 2차 순회하여 직계 자식 및 손자 프로세스(msedgewebview2.exe, Renderer 357MB, GPU 93MB)를 모두 수집
        let mut all_processes: Vec<(u32, u32)> = Vec::new();
        let mut entry: PROCESSENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

        if Process32First(snapshot, &mut entry) != 0 {
            loop {
                all_processes.push((entry.th32ProcessID, entry.th32ParentProcessID));
                if Process32Next(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);

        // 자식 및 손자 PID 확산 수집
        let mut changed = true;
        while changed {
            changed = false;
            for &(pid, ppid) in &all_processes {
                if child_pids.contains(&ppid) && !child_pids.contains(&pid) {
                    child_pids.insert(pid);
                    changed = true;
                }
            }
        }

        // 3. 수집된 모든 자식 프로세스의 작업 세트를 일괄 비우기 (357MB 렌더러 + 93MB GPU 프로세스 전수 압축)
        for pid in child_pids {
            if pid == my_pid {
                continue;
            }
            let handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION, 0, pid);
            if !handle.is_null() {
                let _ = SetProcessWorkingSetSize(handle, usize::MAX, usize::MAX);
                CloseHandle(handle);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn trim_process_working_set() {}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // WebView2의 과도한 메모리 점유(491MB)를 억제하면서도 V8 OOM 크래시를 원천 방지하는 정밀 플래그 주입
    // 1. --disable-extensions & --disk-cache-size=1: 불필요한 확장기능 및 디스크 캐시 극소화
    // 2. --optimize-for-size & --expose-gc: V8 코드 캐시 최소화 및 20초 주기 OS 작업 세트 동기 회수 파이프라인 가동
    // 3. --max-old-space-size=256: 기존 64MB 및 semi-space=1로 인한 V8 Fatal Out of Memory 크래시 원천 차단
    if std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_err() {
        unsafe {
            std::env::set_var(
                "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                "--renderer-process-limit=1 \
                 --in-process-gpu \
                 --disable-extensions \
                 --disk-cache-size=1 \
                 --disable-background-networking \
                 --disable-gpu-shader-disk-cache \
                 --disable-component-update \
                 --disable-features=AudioServiceOutOfProcess,RendererCodeIntegrity,PreloadMediaEngagementData,AutofillServerCommunication,CalculateNativeWinOcclusion \
                 --js-flags=\"--optimize-for-size --max-old-space-size=256 --expose-gc\"",
            );
        }
    }
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--overlay" || a == "--voice-overlay") {
        overlay::launch_voice_overlay();
        return Ok(());
    }
    if args.iter().any(|a| a == "--stream-viewer" || a == "--popout-stream") {
        stream_viewer::launch_stream_viewer();
        return Ok(());
    }

    let event_loop: EventLoop<UserEvent> = EventLoopBuilder::<UserEvent>::with_user_event().build();

    // 1. Electron 창 생성 옵션 준수: 1280x800 기본, 800x600 최소, Frameless, 초기 비가시
    // 창 아이콘을 Win32 HWND에 명시적으로 등록하여 작업 관리자 및 창 목록에 선명한 Fluxer 로고를 표출합니다.
    let mut window_builder = WindowBuilder::new()
        .with_title("Fluxer")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(800.0, 600.0))
        .with_visible(false);

    if let Some(icon) = load_app_icon() {
        window_builder = window_builder.with_window_icon(Some(icon));
    }

    let window = window_builder.build(&event_loop)?;

    let window = Arc::new(window);
    let proxy = event_loop.create_proxy();

    // 2. 시스템 트레이 초기화
    let tray_manager = match TrayManager::new() {
        Ok(manager) => Some(manager),
        Err(err) => {
            eprintln!("[Fluxer Tray] Failed to initialize tray: {err:?}");
            None
        }
    };

    // 3. 전역 단축키 초기화
    let shortcut_manager = match ShortcutManager::new() {
        Ok(manager) => Some(manager),
        Err(err) => {
            eprintln!("[Fluxer Shortcuts] Failed to register global shortcuts: {err:?}");
            None
        }
    };

    // 4. 백그라운드 이벤트 리스너 스레드 (트레이 및 전역 단축키 수신 파이프라인)
    let proxy_for_events = proxy.clone();
    std::thread::spawn(move || {
        let menu_receiver = MenuEvent::receiver();
        let tray_receiver = tray_icon::TrayIconEvent::receiver();
        let hotkey_receiver = global_hotkey::GlobalHotKeyEvent::receiver();

        loop {
            // 트레이 메뉴 이벤트
            if let Ok(event) = menu_receiver.try_recv() {
                let _ = proxy_for_events.send_event(UserEvent::TrayMenu(event.id));
            }

            // 트레이 아이콘 좌클릭 이벤트 (우클릭 컨텍스트 메뉴 깜박임 방지)
            if let Ok(tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            }) = tray_receiver.try_recv()
            {
                let _ = proxy_for_events.send_event(UserEvent::TrayIconClick);
            }

            // 전역 단축키 이벤트
            if let Ok(event) = hotkey_receiver.try_recv()
                && event.state == global_hotkey::HotKeyState::Pressed {
                    let _ = proxy_for_events.send_event(UserEvent::GlobalShortcut(event.id));
                }

            std::thread::sleep(Duration::from_millis(16)); // ~60Hz 폴링
        }
    });

    // 5. 안전 Reveal 타이머: UI 신호가 지연되더라도 최대 800ms 후 창 노출
    let proxy_for_timer = proxy.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        let _ = proxy_for_timer.send_event(UserEvent::IpcOneWay(IpcRequest {
            id: None,
            cmd: "firstContentPainted".into(),
            payload: serde_json::Value::Null,
        }));
    });

    // 6. 주기적 메모리 압축기: 기동 5초 후 초기 로딩 힙 즉각 회수, 이후 20초마다 모든 WebView2 자식/손자 프로세스 작업 세트를 전수 압축
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(5));
        trim_process_working_set();
        loop {
            std::thread::sleep(Duration::from_secs(20));
            trim_process_working_set();
        }
    });

    let asset_handler = AssetHandler::new();

    let win_for_drag = Arc::clone(&window);
    let win_for_ipc = Arc::clone(&window);
    let proxy_for_ipc = proxy.clone();

    let cli_launch_instance = get_cli_launch_instance();
    let cli_for_ipc = cli_launch_instance.clone();

    // 5. Rust 네이티브 백그라운드 WebSocket 게이트웨이 매니저 초기화
    let proxy_for_gateway = proxy.clone();
    let gateway_manager = Arc::new(gateway::GatewayManager::new(Arc::new(move |evt| {
        let _ = proxy_for_gateway.send_event(UserEvent::GatewayEvent(evt));
    }))?);
    let gateway_for_ipc = Arc::clone(&gateway_manager);

    // 6. Wry WebView2 빌드
    let builder = WebViewBuilder::new()
        .with_background_color((0x1a, 0x18, 0x1e, 255))
        .with_initialization_script(bridge::get_initialization_script())
        .with_custom_protocol("fluxer-app".into(), move |_id, req| {
            asset_handler.handle_request(&req)
        })
        .with_ipc_handler(move |req| {
            let body = req.body();
            if let Ok(ipc_req) = serde_json::from_str::<IpcRequest>(body) {
                // 단방향 Win32 네이티브 드래그: 렌더러 스레드 병목 없이 즉시 호출
                if ipc_req.cmd == "dragWindow" {
                    if let Err(err) = win_for_drag.drag_window() {
                        eprintln!("[Fluxer Shell] Failed to drag window: {err:?}");
                    }
                    return;
                }

                // ID가 있는 양방향 Invoke 요청 처리 (네트워크 프록시 등이 메인 스레드를 블로킹하지 않도록 워커 스레드에서 비동기 수행)
                if ipc_req.id.is_some() {
                    let win = Arc::clone(&win_for_ipc);
                    let cli = cli_for_ipc.clone();
                    let proxy = proxy_for_ipc.clone();
                    let gw = Arc::clone(&gateway_for_ipc);
                    std::thread::spawn(move || {
                        if let Some(res) = ipc::handle_ipc_request(ipc_req, &win, cli.as_deref(), Some(&gw)) {
                            let _ = proxy.send_event(UserEvent::IpcReply(res));
                        }
                    });
                    return;
                }

                // ID가 없는 단방향 메시지
                let _ = proxy_for_ipc.send_event(UserEvent::IpcOneWay(ipc_req));
            }
        });

    // 공식 데스크톱 웹 프론트엔드로 진입하여 인스턴스 선택 및 인증 파이프라인 구동
    // 1. FLUXER_WEB_URL 환경변수 우선
    // 2. CLI 실행 인자 (--fluxer-instance=... 또는 URL)
    // 3. 로컬에 영구 저장된 마지막 활성 인스턴스 (config::load_last_instance())
    // 4. 저장된 계정 목록으로부터 인스턴스 복원 (storage::get_last_instance_from_accounts())
    // 5. 알려진 인스턴스 기록으로부터 복원 (config::load_latest_known_instance_url())
    // 6. 기본값: 공식 웹 클라이언트 (https://web.fluxer.app) -> 인스턴스 선택기 표출
    let target_url = if let Ok(env_url) = std::env::var("FLUXER_WEB_URL") {
        env_url
    } else if let Some(cli_url) = cli_launch_instance.as_deref() {
        if cli_url.starts_with("http://") || cli_url.starts_with("https://") {
            cli_url.to_string()
        } else {
            format!("https://{cli_url}")
        }
    } else if let Some(last_url) = config::load_last_instance() {
        let base = last_url.trim_end_matches('/');
        format!("{base}/channels/@me")
    } else if let Some(acc_url) = storage::get_last_instance_from_accounts() {
        config::save_last_instance(&acc_url);
        let base = acc_url.trim_end_matches('/');
        format!("{base}/channels/@me")
    } else if let Some(known_url) = config::load_latest_known_instance_url() {
        if !known_url.contains("web.fluxer.app") {
            config::save_last_instance(&known_url);
            let base = known_url.trim_end_matches('/');
            format!("{base}/channels/@me")
        } else {
            "https://web.fluxer.app".to_string()
        }
    } else {
        "https://web.fluxer.app".to_string()
    };

    println!("[Fluxer Shell] Bootstrapping WebView with target URL: {target_url}");

    let builder = builder
        .with_devtools(true)
        .with_url(&target_url)
        .with_new_window_req_handler(move |url, _disposition| {
            // 빈 URL, about:blank, data: 또는 fluxer 내부 팝아웃 새 창 허용
            if url.is_empty() || url == "about:blank" || url.starts_with("data:") || url.contains("fluxer") {
                wry::NewWindowResponse::Allow
            } else {
                let _ = open::that_detached(&url);
                wry::NewWindowResponse::Deny
            }
        });

    let webview = builder.build(&window)?;

    let mut is_revealed = false;

    // 7. 이벤트 루프 구동
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(UserEvent::IpcOneWay(req)) => match req.cmd.as_str() {
                "windowMinimize" => {
                    println!("[Fluxer Shell] windowMinimize requested by caption control.");
                    window.set_minimized(true);
                }
                "windowMaximize" => {
                    let current = window.is_maximized();
                    let next_max = !current;
                    window.set_maximized(next_max);
                    println!("[Fluxer Shell] windowMaximize toggled (actual current: {current}, next: {next_max}).");
                    let script = format!(
                        "window.__isMaximized = {next_max}; window.dispatchEvent(new CustomEvent('fluxer:maximize-change', {{ detail: {{ isMaximized: {next_max} }} }}));"
                    );
                    let _ = webview.evaluate_script(&script);
                }
                "windowSetFullscreen" => {
                    let is_fullscreen = req.payload.get("fullscreen").and_then(|v| v.as_bool()).unwrap_or(false);
                    if is_fullscreen {
                        window.set_fullscreen(Some(tao::window::Fullscreen::Borderless(None)));
                    } else {
                        window.set_fullscreen(None);
                    }
                    println!("[Fluxer Shell] windowSetFullscreen executed: {is_fullscreen}");
                    let script = format!(
                        "window.__isNativeFullscreen = {is_fullscreen}; window.dispatchEvent(new CustomEvent('fluxer:fullscreen-change', {{ detail: {{ isFullscreen: {is_fullscreen} }} }}));"
                    );
                    let _ = webview.evaluate_script(&script);
                }
                "windowClose" => {
                    println!("[Fluxer Shell] windowClose requested by caption control.");
                    *control_flow = ControlFlow::Exit;
                }
                "firstContentPainted" => {
                    if !is_revealed {
                        is_revealed = true;
                        window.set_visible(true);
                        println!("[Fluxer Shell] Main window revealed with zero white-flash.");
                    }
                }
                "commitInstance" => {
                    if let Some(instance_key) = req.payload.get("instanceKey").and_then(|v| v.as_str()) {
                        let full_url = if !instance_key.starts_with("http://") && !instance_key.starts_with("https://") {
                            format!("https://{instance_key}")
                        } else {
                            instance_key.to_string()
                        };
                        let base_url = full_url.trim_end_matches("/api").trim_end_matches('/').to_string();
                        config::save_last_instance(&base_url);
                        let target = format!("{base_url}/channels/@me");
                        println!("[Fluxer Shell] Navigating to committed instance: {target}");
                        let _ = webview.load_url(&target);
                    }
                }
                "navigateToInstance" => {
                    if let Some(url) = req.payload.get("url").and_then(|v| v.as_str()) {
                        let full_url = if !url.starts_with("http://") && !url.starts_with("https://") {
                            format!("https://{url}")
                        } else {
                            url.to_string()
                        };
                        let base_url = full_url.trim_end_matches("/api").trim_end_matches('/').to_string();
                        config::save_last_instance(&base_url);
                        let target = format!("{base_url}/channels/@me");
                        println!("[Fluxer Shell] Navigating to selected instance: {target}");
                        let _ = webview.load_url(&target);
                    }
                }
                "deactivateInstance" => {
                    config::clear_last_instance();
                    println!("[Fluxer Shell] Deactivated instance, returning to default picker.");
                    let _ = webview.load_url("https://web.fluxer.app");
                }
                "consoleLog" => {
                    let level = req.payload.get("level").and_then(|v| v.as_str()).unwrap_or("log");
                    let message = req.payload.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    match level {
                        "error" => eprintln!("[Web Error] {message}"),
                        "warn" => eprintln!("[Web Warn] {message}"),
                        _ => println!("[Web Log] {message}"),
                    }
                }
                "reportLastRoute" => {
                    // 프론트엔드 라우트 변경 통지 (정상 수용)
                }
                "reportInstanceProductName" => {
                    // 프론트엔드 제품명 통지 (정상 수용)
                }
                "rendererGcComplete" => {
                    // 렌더러 V8 GC 완료 통지에 동기화하여 Windows OS 작업 세트 즉각 강제 회수
                    trim_process_working_set();
                }
                other => {
                    eprintln!("[IPC Warning] Unhandled one-way channel: '{other}'");
                }
            },
            Event::UserEvent(UserEvent::IpcReply(res)) => {
                let err_str = match res.error {
                    Some(err) => format!("'{}'", err.replace('\'', "\\'")),
                    None => "null".to_string(),
                };
                let script = format!(
                    "window.__fluxerIpcReply({}, {}, {});",
                    res.id, res.result, err_str
                );
                let _ = webview.evaluate_script(&script);
            }
            Event::UserEvent(UserEvent::GatewayEvent(evt)) => {
                // 웹뷰 프론트엔드로 네이티브 게이트웨이 이벤트 포워딩
                if let Ok(json) = serde_json::to_string(&evt) {
                    let script = format!(
                        "if (window.__fluxerNativeGatewayEmit) {{ window.__fluxerNativeGatewayEmit({json}); }}"
                    );
                    let _ = webview.evaluate_script(&script);
                }
            }
            Event::UserEvent(UserEvent::TrayMenu(menu_id)) => {
                if let Some(tray) = &tray_manager {
                    match tray.handle_menu_event(&menu_id, &window) {
                        TrayMenuAction::Quit => {
                            *control_flow = ControlFlow::Exit;
                        }
                        TrayMenuAction::Restart => {
                            if let Ok(exe) = std::env::current_exe() {
                                let _ = std::process::Command::new(exe).spawn();
                            }
                            *control_flow = ControlFlow::Exit;
                        }
                        TrayMenuAction::OpenSettings => {
                            let _ = webview.evaluate_script(
                                "if (window.__fluxerEmitOpenSettings) { window.__fluxerEmitOpenSettings(); } else { window.dispatchEvent(new CustomEvent('fluxer:open-settings')); }"
                            );
                        }
                        TrayMenuAction::SetStatus(status) => {
                            let script = format!(
                                "if (window.__fluxerEmitTrayAction) {{ window.__fluxerEmitTrayAction({{ action: 'set-status', status: '{status}' }}); }} else {{ window.dispatchEvent(new CustomEvent('fluxer:set-status', {{ detail: {{ status: '{status}' }} }})); }}"
                            );
                            let _ = webview.evaluate_script(&script);
                        }
                        TrayMenuAction::CheckForUpdates => {
                            let _ = webview.evaluate_script(
                                "if (window.__fluxerEmitTrayAction) { window.__fluxerEmitTrayAction({ action: 'check-for-updates' }); } else { window.dispatchEvent(new CustomEvent('fluxer:check-updates')); }"
                            );
                        }
                        TrayMenuAction::ToggleOverlay => {
                            println!("[Fluxer Shell] Tray action triggered: Launch GPUI voice overlay");
                            overlay::launch_voice_overlay_process();
                        }
                        TrayMenuAction::ToggleWindow => (),
                        TrayMenuAction::None => (),
                    }
                }
            }
            Event::UserEvent(UserEvent::TrayIconClick) => {
                if let Some(tray) = &tray_manager {
                    tray.toggle_window(&window);
                }
            }
            Event::UserEvent(UserEvent::GlobalShortcut(hotkey_id)) => {
                if let Some(sc) = &shortcut_manager {
                    if hotkey_id == sc.focus_hotkey.id() {
                        window.set_visible(true);
                        window.set_focus();
                        println!("[Fluxer Shell] Global shortcut triggered: Focus window");
                    } else if hotkey_id == sc.mute_hotkey.id() {
                        let _ = webview.evaluate_script(
                            "window.dispatchEvent(new CustomEvent('fluxer:global-keybind', { detail: { action: 'toggle-mute' } }));"
                        );
                        println!("[Fluxer Shell] Global shortcut triggered: Toggle mute");
                    } else if hotkey_id == sc.overlay_hotkey.id() {
                        println!("[Fluxer Shell] Global shortcut triggered: Launch GPUI voice overlay (Ctrl+Shift+O)");
                        overlay::launch_voice_overlay_process();
                    }
                }
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Resized(_) => {
                    let maximized = window.is_maximized();
                    let script = format!(
                        "if (window.__isMaximized !== {maximized}) {{ window.__isMaximized = {maximized}; window.dispatchEvent(new CustomEvent('fluxer:maximize-change', {{ detail: {{ isMaximized: {maximized} }} }})); }}"
                    );
                    let _ = webview.evaluate_script(&script);
                }
                WindowEvent::Focused(false) => {
                    trim_process_working_set();
                }
                _ => (),
            },
            _ => (),
        }
    });
}
