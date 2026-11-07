// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use base64::prelude::*;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024; // 8MB
const MAX_CONNECTION_ID_LEN: usize = 192;
const MAX_URL_LEN: usize = 2048;

/// JavaScript 브리지로 전달되는 네이티브 게이트웨이 트랜스포트 이벤트 규격입니다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeGatewayTransportEvent {
    pub connection_id: String,
    pub kind: String, // "open", "message", "binary", "close", "error"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>, // Base64 인코딩 문자열
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_clean: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl NativeGatewayTransportEvent {
    pub fn open(connection_id: impl Into<String>) -> Self {
        Self {
            connection_id: connection_id.into(),
            kind: "open".to_string(),
            data: None,
            binary: None,
            code: None,
            reason: None,
            was_clean: None,
            message: None,
        }
    }

    pub fn message(connection_id: impl Into<String>, text: String) -> Self {
        Self {
            connection_id: connection_id.into(),
            kind: "message".to_string(),
            data: Some(text),
            binary: None,
            code: None,
            reason: None,
            was_clean: None,
            message: None,
        }
    }

    pub fn binary(connection_id: impl Into<String>, base64_payload: String) -> Self {
        Self {
            connection_id: connection_id.into(),
            kind: "binary".to_string(),
            data: None,
            binary: Some(base64_payload),
            code: None,
            reason: None,
            was_clean: None,
            message: None,
        }
    }

    pub fn close(
        connection_id: impl Into<String>,
        code: u16,
        reason: impl Into<String>,
        was_clean: bool,
    ) -> Self {
        Self {
            connection_id: connection_id.into(),
            kind: "close".to_string(),
            data: None,
            binary: None,
            code: Some(code),
            reason: Some(reason.into()),
            was_clean: Some(was_clean),
            message: None,
        }
    }

    pub fn error(connection_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            connection_id: connection_id.into(),
            kind: "error".to_string(),
            data: None,
            binary: None,
            code: None,
            reason: None,
            was_clean: None,
            message: Some(message.into()),
        }
    }
}

/// 백그라운드 WebSocket 태스크로 전달되는 제어 명령입니다.
#[derive(Debug)]
pub enum GatewayCommand {
    SendText(String),
    SendBinary(Vec<u8>),
    Close { code: u16, reason: String },
    Dispose,
}

pub type EventSender = Arc<dyn Fn(NativeGatewayTransportEvent) + Send + Sync + 'static>;

/// Rust 네이티브 백그라운드 WebSocket 게이트웨이 매니저입니다.
pub struct GatewayManager {
    connections: Arc<RwLock<HashMap<String, mpsc::Sender<GatewayCommand>>>>,
    runtime: tokio::runtime::Runtime,
    event_sender: EventSender,
}

impl GatewayManager {
    pub fn new(event_sender: EventSender) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("fluxer-gateway-worker")
            .enable_all()
            .build()?;

        Ok(Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            runtime,
            event_sender,
        })
    }

    /// 신규 WebSocket 게이트웨이 연결을 생성하고 백그라운드 비동기 태스크를 가동합니다.
    pub fn create(&self, connection_id: String, url: String) -> Result<()> {
        if connection_id.is_empty() || connection_id.len() > MAX_CONNECTION_ID_LEN {
            bail!("Invalid gateway connectionId length");
        }
        if url.is_empty() || url.len() > MAX_URL_LEN {
            bail!("Invalid gateway url length");
        }
        if !url.starts_with("ws://") && !url.starts_with("wss://") {
            bail!("Gateway url must start with ws:// or wss://");
        }

        let (cmd_tx, cmd_rx) = mpsc::channel(64);

        {
            let mut conns = self.connections.write().unwrap();
            if conns.contains_key(&connection_id) {
                bail!("Duplicate gateway connectionId: {}", connection_id);
            }
            conns.insert(connection_id.clone(), cmd_tx);
        }

        let cid = connection_id.clone();
        let event_sender = Arc::clone(&self.event_sender);
        let connections = Arc::clone(&self.connections);

        self.runtime.spawn(async move {
            run_socket_task(cid, url, cmd_rx, event_sender, connections).await;
        });

        Ok(())
    }

    /// WebSocket으로 텍스트 프레임을 전송합니다.
    pub fn send_text(&self, connection_id: &str, payload: String) -> Result<()> {
        if payload.len() > MAX_FRAME_SIZE {
            bail!("Gateway payload exceeds maximum allowed size of 8MB");
        }

        let sender = {
            let conns = self.connections.read().unwrap();
            conns.get(connection_id).cloned()
        };

        match sender {
            Some(tx) => {
                tx.try_send(GatewayCommand::SendText(payload))
                    .map_err(|e| anyhow!("Failed to send text frame: {e}"))?;
                Ok(())
            }
            None => bail!("Gateway connection not found: {connection_id}"),
        }
    }

    /// WebSocket으로 바이너리 프레임을 전송합니다.
    pub fn send_binary(&self, connection_id: &str, payload: Vec<u8>) -> Result<()> {
        if payload.len() > MAX_FRAME_SIZE {
            bail!("Gateway payload exceeds maximum allowed size of 8MB");
        }

        let sender = {
            let conns = self.connections.read().unwrap();
            conns.get(connection_id).cloned()
        };

        match sender {
            Some(tx) => {
                tx.try_send(GatewayCommand::SendBinary(payload))
                    .map_err(|e| anyhow!("Failed to send binary frame: {e}"))?;
                Ok(())
            }
            None => bail!("Gateway connection not found: {connection_id}"),
        }
    }

    /// WebSocket에 Close 프레임을 발송하고 정상 종료 절차를 시작합니다.
    pub fn close(&self, connection_id: &str, code: u16, reason: String) -> Result<()> {
        let sender = {
            let conns = self.connections.read().unwrap();
            conns.get(connection_id).cloned()
        };

        match sender {
            Some(tx) => {
                let _ = tx.try_send(GatewayCommand::Close { code, reason });
                Ok(())
            }
            None => bail!("Gateway connection not found: {connection_id}"),
        }
    }

    /// 해당 연결을 즉시 강제 해제하고 내부 리소스를 정리합니다.
    pub fn dispose(&self, connection_id: &str) -> Result<()> {
        let sender = {
            let mut conns = self.connections.write().unwrap();
            conns.remove(connection_id)
        };

        if let Some(tx) = sender {
            let _ = tx.try_send(GatewayCommand::Dispose);
        }

        Ok(())
    }

    /// 현재 관리 중인 연결의 개수를 조회합니다.
    #[allow(dead_code)]
    pub fn connection_count(&self) -> usize {
        self.connections.read().unwrap().len()
    }

    /// 특정 연결의 존재 여부를 조회합니다.
    #[allow(dead_code)]
    pub fn has_connection(&self, connection_id: &str) -> bool {
        self.connections.read().unwrap().contains_key(connection_id)
    }

    /// 모든 활성 연결을 정리하고 종료합니다.
    pub fn cleanup_all(&self) {
        let conns: Vec<mpsc::Sender<GatewayCommand>> = {
            let mut map = self.connections.write().unwrap();
            map.drain().map(|(_, tx)| tx).collect()
        };

        for tx in conns {
            let _ = tx.try_send(GatewayCommand::Dispose);
        }
    }
}

impl Drop for GatewayManager {
    fn drop(&mut self) {
        self.cleanup_all();
    }
}

/// 단일 WebSocket 연결의 비동기 수신/송신 파이프라인 태스크입니다.
async fn run_socket_task(
    connection_id: String,
    url: String,
    mut cmd_rx: mpsc::Receiver<GatewayCommand>,
    event_sender: EventSender,
    connections: Arc<RwLock<HashMap<String, mpsc::Sender<GatewayCommand>>>>,
) {
    let connect_result = tokio::time::timeout(Duration::from_secs(10), connect_async(&url)).await;

    let ws_stream = match connect_result {
        Ok(Ok((stream, _response))) => stream,
        Ok(Err(err)) => {
            event_sender(NativeGatewayTransportEvent::error(
                &connection_id,
                format!("WebSocket connection failed: {err}"),
            ));
            event_sender(NativeGatewayTransportEvent::close(
                &connection_id,
                1006,
                "Failed to establish WebSocket connection",
                false,
            ));
            connections.write().unwrap().remove(&connection_id);
            return;
        }
        Err(_) => {
            event_sender(NativeGatewayTransportEvent::error(
                &connection_id,
                "WebSocket connection timeout after 10s",
            ));
            event_sender(NativeGatewayTransportEvent::close(
                &connection_id,
                1006,
                "Connection timeout",
                false,
            ));
            connections.write().unwrap().remove(&connection_id);
            return;
        }
    };

    // 1. 소켓 연결 성공 통지
    event_sender(NativeGatewayTransportEvent::open(&connection_id));

    let (mut write, mut read) = ws_stream.split();

    // 2. 메시지 수신 및 송신 제어 이벤트 루프
    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(GatewayCommand::SendText(text)) => {
                        if let Err(e) = write.send(Message::Text(text.into())).await {
                            event_sender(NativeGatewayTransportEvent::error(
                                &connection_id,
                                format!("Failed to send text frame: {e}"),
                            ));
                            break;
                        }
                    }
                    Some(GatewayCommand::SendBinary(payload)) => {
                        if let Err(e) = write.send(Message::Binary(payload.into())).await {
                            event_sender(NativeGatewayTransportEvent::error(
                                &connection_id,
                                format!("Failed to send binary frame: {e}"),
                            ));
                            break;
                        }
                    }
                    Some(GatewayCommand::Close { code, reason }) => {
                        let close_frame = CloseFrame {
                            code: CloseCode::from(code),
                            reason: reason.into(),
                        };
                        let _ = write.send(Message::Close(Some(close_frame))).await;
                        break;
                    }
                    Some(GatewayCommand::Dispose) | None => {
                        let _ = write.send(Message::Close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: "Disposed by desktop client".into(),
                        }))).await;
                        break;
                    }
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        event_sender(NativeGatewayTransportEvent::message(
                            &connection_id,
                            text.to_string(),
                        ));
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        let b64 = BASE64_STANDARD.encode(&bytes);
                        event_sender(NativeGatewayTransportEvent::binary(
                            &connection_id,
                            b64,
                        ));
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let (code, reason) = frame
                            .map(|f| (u16::from(f.code), f.reason.to_string()))
                            .unwrap_or((1000, String::new()));
                        event_sender(NativeGatewayTransportEvent::close(
                            &connection_id,
                            code,
                            reason,
                            true,
                        ));
                        break;
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => {
                        // Pong은 Tungstenite 내부에서 자동으로 처리됩니다.
                    }
                    Some(Err(err)) => {
                        event_sender(NativeGatewayTransportEvent::error(
                            &connection_id,
                            format!("WebSocket stream error: {err}"),
                        ));
                        event_sender(NativeGatewayTransportEvent::close(
                            &connection_id,
                            1006,
                            "Abnormal closure",
                            false,
                        ));
                        break;
                    }
                    None => {
                        // 스트림 종료
                        event_sender(NativeGatewayTransportEvent::close(
                            &connection_id,
                            1000,
                            "Stream reached EOF",
                            true,
                        ));
                        break;
                    }
                }
            }
        }
    }

    // 소켓 종료 후 매니저 맵에서 제거
    connections.write().unwrap().remove(&connection_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn test_event_serialization() {
        let open_event = NativeGatewayTransportEvent::open("conn-1");
        let json = serde_json::to_string(&open_event).expect("serialize open");
        assert!(json.contains("\"kind\":\"open\""));
        assert!(json.contains("\"connectionId\":\"conn-1\""));

        let msg_event = NativeGatewayTransportEvent::message("conn-1", "hello fluxer".to_string());
        let json = serde_json::to_string(&msg_event).expect("serialize message");
        assert!(json.contains("\"kind\":\"message\""));
        assert!(json.contains("\"data\":\"hello fluxer\""));

        let bin_event = NativeGatewayTransportEvent::binary("conn-1", "aGVsbG8=".to_string());
        let json = serde_json::to_string(&bin_event).expect("serialize binary");
        assert!(json.contains("\"kind\":\"binary\""));
        assert!(json.contains("\"binary\":\"aGVsbG8=\""));

        let close_event = NativeGatewayTransportEvent::close("conn-1", 1000, "normal", true);
        let json = serde_json::to_string(&close_event).expect("serialize close");
        assert!(json.contains("\"kind\":\"close\""));
        assert!(json.contains("\"code\":1000"));
        assert!(json.contains("\"wasClean\":true"));
    }

    #[test]
    fn test_manager_lifecycle_and_validation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let event_sender: EventSender = Arc::new(move |evt| {
            events_clone.lock().unwrap().push(evt);
        });

        let manager = GatewayManager::new(event_sender).expect("manager init");
        assert_eq!(manager.connection_count(), 0);

        // 잘못된 URL 검증
        let err = manager.create("conn-1".to_string(), "http://invalid.url".to_string());
        assert!(err.is_err());

        // 빈 connectionId 검증
        let err = manager.create("".to_string(), "wss://gateway.fluxer.app".to_string());
        assert!(err.is_err());

        // 정상 연결 등록
        let res = manager.create("conn-1".to_string(), "wss://127.0.0.1:9".to_string());
        assert!(res.is_ok());
        assert!(manager.has_connection("conn-1"));

        // 중복 connectionId 등록 거부
        let err = manager.create("conn-1".to_string(), "wss://127.0.0.1:9".to_string());
        assert!(err.is_err());

        // 프레임 발송 실패 (미존재 연결)
        let err = manager.send_text("unknown", "test".to_string());
        assert!(err.is_err());

        // dispose 호출
        let res = manager.dispose("conn-1");
        assert!(res.is_ok());
        assert!(!manager.has_connection("conn-1"));
    }
}
