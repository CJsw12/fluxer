use std::time::Duration;

use fluxer_client_core::Dispatch;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::ui::reply::{ReplyLine, reply_line};

use super::*;

#[tokio::test]
async fn rest_page_from_before_ready_does_not_repopulate_the_window() {
    const WAIT: Duration = Duration::from_secs(5);
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (requested, in_flight) = oneshot::channel();
    let (ready_applied, release_response) = oneshot::channel();
    let server = tokio::spawn(async move {
        for pause in [Some((requested, release_response)), None] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut headers = BufReader::new(&mut stream);
            let mut line = String::new();
            headers.read_line(&mut line).await.unwrap();
            assert!(line.starts_with("GET /api/v1/channels/11/messages?"));
            loop {
                line.clear();
                headers.read_line(&mut line).await.unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            if let Some((requested, release)) = pause {
                requested.send(()).unwrap();
                release.await.unwrap();
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]")
                .await.unwrap();
        }
    });
    let endpoints = Endpoints {
        api_code_version: 1,
        api: format!("http://{addr}/api"),
        gateway: format!("ws://{addr}/gateway"),
        media: None,
        static_cdn: None,
        upload_relay: None,
        invite: None,
        webapp: None,
        discovery_url: format!("http://{addr}/.well-known/fluxer"),
        document: json!({}),
    };
    let mut config = RestConfig::new(&endpoints, ClientIdentity::default());
    config.policy = TransportPolicy {
        allow_insecure_loopback: true,
    };
    let rest = RestClient::new(config).unwrap();
    rest.set_token(Some(SecretToken::new("fixture-token"))).await;
    let (backend, mut events) = Backend::new(Handle::current(), Arc::new(rest), None);
    let snapshot = Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "fixture-session",
            "user": { "id": "100", "username": "me" },
            "private_channels": [{ "id": "11", "type": 1 }]
        }),
    };
    let channel = Snowflake(11);
    backend.state().apply(&snapshot);
    let load = backend.load_page(channel, LoadMode::Replace, None);
    timeout(WAIT, in_flight).await.unwrap().unwrap();

    // A READY with the same session ID also invalidates the current window.
    backend.state().apply(&snapshot);
    ready_applied.send(()).unwrap();
    timeout(WAIT, load).await.unwrap().unwrap();
    assert!(backend.state().messages.window(channel).is_none());
    assert!(matches!(events.try_recv(), Err(mpsc::error::TryRecvError::Empty)));

    let reload = backend.load_page(channel, LoadMode::Replace, None);
    timeout(WAIT, reload).await.unwrap().unwrap();
    timeout(WAIT, server).await.unwrap().unwrap();
    assert!(backend.state().messages.window(channel).is_some_and(|window| window.ready));
    assert_eq!(events.try_recv().unwrap(), backend.history_loaded(channel, None));
}

const WAIT: Duration = Duration::from_secs(5);
const CHANNEL: Snowflake = Snowflake(11);
const TARGET: Snowflake = Snowflake(50);
const REPLY: Snowflake = Snowflake(200);

fn load_unresolved_reply(backend: &Backend) {
    let mut state = backend.state();
    state.apply(&Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "fixture-session",
            "user": { "id": "100", "username": "me" },
            "private_channels": [{ "id": CHANNEL.to_string(), "type": 1 }]
        }),
    });
    let reply = serde_json::from_value(json!({
        "id": REPLY.to_string(),
        "channel_id": CHANNEL.to_string(),
        "type": 19,
        "author": { "id": "100", "username": "me" },
        "content": "reply",
        "timestamp": "2026-11-18T10:01:00Z",
        "message_reference": { "channel_id": CHANNEL.to_string(), "message_id": TARGET.to_string(), "type": 0 }
    }))
    .unwrap();
    state.messages.load(CHANNEL, vec![reply], LoadMode::Replace);
}

async fn reply_fixture() -> (Backend, mpsc::UnboundedReceiver<UiEvent>, TcpListener) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoints = Endpoints {
        api_code_version: 1,
        api: format!("http://{addr}/api"),
        gateway: format!("ws://{addr}/gateway"),
        media: None,
        static_cdn: None,
        upload_relay: None,
        invite: None,
        webapp: None,
        discovery_url: format!("http://{addr}/.well-known/fluxer"),
        document: json!({}),
    };
    let mut config = RestConfig::new(&endpoints, ClientIdentity::default());
    config.policy = TransportPolicy { allow_insecure_loopback: true };
    let rest = RestClient::new(config).unwrap();
    rest.set_token(Some(SecretToken::new("fixture-token"))).await;
    let (backend, events) = Backend::new(Handle::current(), Arc::new(rest), None);
    load_unresolved_reply(&backend);
    (backend, events, listener)
}

async fn receive_reply_request(listener: &TcpListener) -> tokio::net::TcpStream {
    timeout(WAIT, async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut headers = BufReader::new(&mut stream);
        let mut line = String::new();
        headers.read_line(&mut line).await.unwrap();
        assert!(line.starts_with(&format!("GET /api/v1/channels/{CHANNEL}/messages/{TARGET} ")));
        loop {
            line.clear();
            assert_ne!(headers.read_line(&mut line).await.unwrap(), 0);
            if line == "\r\n" {
                break;
            }
        }
        stream
    })
    .await
    .unwrap()
}

async fn respond(mut stream: tokio::net::TcpStream, status: &str, body: &str, task: tokio::task::JoinHandle<()>) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    timeout(WAIT, async {
        stream.write_all(response.as_bytes()).await.unwrap();
        drop(stream);
        task.await.unwrap();
    })
    .await
    .unwrap();
}

fn target_body() -> String {
    json!({
        "id": TARGET.to_string(),
        "channel_id": CHANNEL.to_string(),
        "author": { "id": "201", "username": "alice" },
        "content": "resolved body",
        "timestamp": "2026-11-18T10:00:00Z"
    })
    .to_string()
}

fn preview(backend: &Backend) -> ReplyLine {
    let state = backend.state();
    reply_line(&state, state.messages.message(CHANNEL, REPLY).unwrap(), None).unwrap()
}

const UNRESOLVED: ReplyLine = ReplyLine::NotLoaded { channel_id: CHANNEL, message_id: TARGET };

#[tokio::test]
async fn fetch_reply_target_retries_transient_failure_and_shows_the_target() {
    let (backend, mut events, listener) = reply_fixture().await;
    assert_eq!(preview(&backend), UNRESOLVED);

    let failed = backend.fetch_reply_target(CHANNEL, TARGET).unwrap();
    let request = receive_reply_request(&listener).await;
    assert!(backend.fetch_reply_target(CHANNEL, TARGET).is_none());
    respond(request, "503 Service Unavailable", "{}", failed).await;
    assert_eq!(preview(&backend), UNRESOLVED);
    assert!(matches!(events.try_recv(), Err(mpsc::error::TryRecvError::Empty)));

    assert!(backend.fetch_reply_target(CHANNEL, TARGET).is_none(), "wait before retrying the failed lookup");
    tokio::time::sleep(REPLY_TARGET_RETRY_DELAY).await;
    let retry = backend.fetch_reply_target(CHANNEL, TARGET).unwrap();
    respond(receive_reply_request(&listener).await, "200 OK", &target_body(), retry).await;
    assert_eq!(events.try_recv().unwrap(), UiEvent::StateChanged);
    let ReplyLine::Loaded(target) = preview(&backend) else {
        panic!("reply target should be visible");
    };
    assert_eq!(target.message_id, TARGET);
    assert_eq!(target.author.as_ref(), "alice");
    assert_eq!(target.excerpt.as_ref(), "resolved body");
    assert!(backend.state().messages.message(CHANNEL, TARGET).is_none());
}

#[tokio::test]
async fn fetch_reply_target_shows_404_as_deleted() {
    let (backend, mut events, listener) = reply_fixture().await;
    let task = backend.fetch_reply_target(CHANNEL, TARGET).unwrap();
    respond(
        receive_reply_request(&listener).await,
        "404 Not Found",
        r#"{"code":"UNKNOWN_MESSAGE","message":"Unknown Message"}"#,
        task,
    )
    .await;
    assert_eq!(events.try_recv().unwrap(), UiEvent::StateChanged);
    assert_eq!(preview(&backend), ReplyLine::Deleted);
}

#[tokio::test]
async fn fetch_reply_target_does_not_restore_a_deleted_target() {
    let (backend, mut events, listener) = reply_fixture().await;
    let task = backend.fetch_reply_target(CHANNEL, TARGET).unwrap();
    let request = receive_reply_request(&listener).await;
    backend.state().apply(&Dispatch {
        t: "MESSAGE_DELETE".to_owned(),
        s: 2,
        d: json!({ "channel_id": CHANNEL.to_string(), "id": TARGET.to_string() }),
    });
    assert_eq!(preview(&backend), ReplyLine::Deleted);
    respond(request, "200 OK", &target_body(), task).await;
    assert_eq!(preview(&backend), ReplyLine::Deleted);
    assert!(matches!(events.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
}

#[tokio::test]
async fn fetch_reply_target_ignores_old_ready_and_keeps_new_request_in_flight() {
    let (backend, mut events, listener) = reply_fixture().await;
    let old_task = backend.fetch_reply_target(CHANNEL, TARGET).unwrap();
    let old_request = receive_reply_request(&listener).await;
    load_unresolved_reply(&backend);

    let new_task = backend.fetch_reply_target(CHANNEL, TARGET).unwrap();
    let new_request = receive_reply_request(&listener).await;
    respond(old_request, "200 OK", &target_body(), old_task).await;
    assert_eq!(preview(&backend), UNRESOLVED);
    assert!(matches!(events.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    assert!(backend.fetch_reply_target(CHANNEL, TARGET).is_none());

    respond(new_request, "200 OK", &target_body(), new_task).await;
    assert_eq!(events.try_recv().unwrap(), UiEvent::StateChanged);
    assert!(matches!(preview(&backend), ReplyLine::Loaded(_)));
}
