// SPDX-License-Identifier: AGPL-3.0-or-later

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
mod bridge;
mod config;
mod ipc;
mod shortcuts;
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
    TrayMenu(MenuId),
    TrayIconClick,
    GlobalShortcut(u32),
}

fn get_cli_launch_instance() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for (i, arg) in args.iter().enumerate().skip(1) {
        if arg == "--fluxer-instance" {
            if let Some(next) = args.get(i + 1) {
                if !next.starts_with("--") {
                    return Some(next.clone());
                }
            }
        } else if let Some(stripped) = arg.strip_prefix("--fluxer-instance=") {
            return Some(stripped.to_string());
        } else if !arg.starts_with("--") && (arg.starts_with("http://") || arg.starts_with("https://") || arg.contains('.')) {
            return Some(arg.clone());
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop: EventLoop<UserEvent> = EventLoopBuilder::<UserEvent>::with_user_event().build();

    // 1. Electron 창 생성 옵션 준수: 1280x800 기본, 800x600 최소, Frameless, 초기 비가시
    // 창 아이콘은 .exe 리소스(icon.ico)를 Windows OS가 네이티브로 자동 상속하여 검은 배경 없이 완벽하게 투명화됩니다.
    let window = WindowBuilder::new()
        .with_title("Fluxer")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(800.0, 600.0))
        .with_visible(false)
        .build(&event_loop)?;

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
            if let Ok(event) = tray_receiver.try_recv() {
                if let tray_icon::TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } = event
                {
                    let _ = proxy_for_events.send_event(UserEvent::TrayIconClick);
                }
            }

            // 전역 단축키 이벤트
            if let Ok(event) = hotkey_receiver.try_recv() {
                if event.state == global_hotkey::HotKeyState::Pressed {
                    let _ = proxy_for_events.send_event(UserEvent::GlobalShortcut(event.id));
                }
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

    let asset_handler = AssetHandler::new();

    let win_for_drag = Arc::clone(&window);
    let win_for_ipc = Arc::clone(&window);
    let proxy_for_ipc = proxy.clone();

    let cli_launch_instance = get_cli_launch_instance();
    let cli_for_ipc = cli_launch_instance.clone();

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
                    std::thread::spawn(move || {
                        if let Some(res) = ipc::handle_ipc_request(ipc_req, &win, cli.as_deref()) {
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
    // 4. 기본값: 공식 웹 클라이언트 (https://web.fluxer.app) -> 인스턴스 선택기 표출
    let target_url = if let Ok(env_url) = std::env::var("FLUXER_WEB_URL") {
        env_url
    } else if let Some(cli_url) = cli_launch_instance.as_deref() {
        if cli_url.starts_with("http://") || cli_url.starts_with("https://") {
            cli_url.to_string()
        } else {
            format!("https://{cli_url}")
        }
    } else if let Some(last_url) = config::load_last_instance() {
        last_url
    } else {
        "https://web.fluxer.app".to_string()
    };

    println!("[Fluxer Shell] Bootstrapping WebView with target URL: {target_url}");

    let builder = builder
        .with_devtools(true)
        .with_url(&target_url);

    let webview = builder.build(&window)?;

    let mut is_revealed = false;

    // 7. 이벤트 루프 구동
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(UserEvent::IpcOneWay(req)) => match req.cmd.as_str() {
                "windowMinimize" => {
                    window.set_minimized(true);
                }
                "windowMaximize" => {
                    let maximized = !window.is_maximized();
                    window.set_maximized(maximized);
                    let script = format!(
                        "window.__isMaximized = {maximized}; window.dispatchEvent(new CustomEvent('fluxer:maximize-change', {{ detail: {{ isMaximized: {maximized} }} }}));"
                    );
                    let _ = webview.evaluate_script(&script);
                }
                "windowClose" => {
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
                        println!("[Fluxer Shell] Navigating to committed instance: {base_url}");
                        let _ = webview.load_url(&base_url);
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
                        println!("[Fluxer Shell] Navigating to selected instance: {base_url}");
                        let _ = webview.load_url(&base_url);
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
                                "window.dispatchEvent(new CustomEvent('fluxer:open-settings'));"
                            );
                        }
                        TrayMenuAction::SetStatus(status) => {
                            let script = format!(
                                "window.dispatchEvent(new CustomEvent('fluxer:set-status', {{ detail: {{ status: '{status}' }} }}));"
                            );
                            let _ = webview.evaluate_script(&script);
                        }
                        TrayMenuAction::CheckForUpdates => {
                            let _ = webview.evaluate_script(
                                "window.dispatchEvent(new CustomEvent('fluxer:check-updates'));"
                            );
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
                _ => (),
            },
            _ => (),
        }
    });
}
