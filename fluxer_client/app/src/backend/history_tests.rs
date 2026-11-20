use std::time::Duration;

use fluxer_client_core::Dispatch;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::timeout;

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
