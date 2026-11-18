//! Gateway tests against an in-process WebSocket server.
//!
//! Timer-driven cases run on a paused tokio clock over in-memory duplex
//! streams (the clock auto-advances only while every task is idle, which a
//! real socket would race). One smoke test goes through `spawn` and a real
//! TCP listener on 127.0.0.1:0.

use std::io::Write as _;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use url::Url;

use super::codec::{ZstdDeflater, ZstdInflater};
use super::connection::gateway_url;
use super::*;

const TOKEN: &str = "test-token-Zx81-secret";
const WAIT: Duration = Duration::from_secs(120);

fn config(compress: Compression) -> GatewayConfig {
    let mut extra = serde_json::Map::new();
    extra.insert("locale".into(), json!("en-US"));
    GatewayConfig {
        url: Url::parse("ws://gateway.test/?keep=1&v=9").expect("url"),
        token: TOKEN.to_owned(),
        properties: IdentifyProperties {
            os: "Windows".into(),
            browser: "Fluxer Rust".into(),
            device: "desktop".into(),
            extra,
        },
        compress,
        flags: FLAG_DEBOUNCE_MESSAGE_REACTIONS | FLAG_CHANNEL_THREADS | FLAG_GUILD_VISITS,
        initial_guild_id: Some("42".into()),
        presence: Some(json!({ "status": "online", "afk": false, "mobile": false })),
    }
}

/// Server side of one accepted connection, optionally zstd-stream.
struct ServerConn<S> {
    ws: WebSocketStream<S>,
    zstd: Option<ServerZstd>,
}

/// Server codec. Server -> client uses the zstd crate's streaming writer (not
/// the client's codec) so the client decoder is checked against an
/// independent encoder.
struct ServerZstd {
    encoder: zstd::stream::write::Encoder<'static, Vec<u8>>,
    decoder: ZstdInflater,
}

impl ServerZstd {
    fn new() -> Self {
        Self {
            encoder: zstd::stream::write::Encoder::new(Vec::new(), 3).expect("encoder"),
            decoder: ZstdInflater::new().expect("decoder"),
        }
    }

    fn compress(&mut self, data: &[u8]) -> Vec<u8> {
        self.encoder.write_all(data).expect("write");
        self.encoder.flush().expect("flush");
        std::mem::take(self.encoder.get_mut())
    }
}

#[derive(Debug, PartialEq)]
enum Got {
    Json(Value),
    Close(Option<u16>),
    Ended,
}

impl<S> ServerConn<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    async fn send(&mut self, value: Value) {
        let text = value.to_string();
        let message = match &mut self.zstd {
            Some(zstd) => Message::binary(zstd.compress(text.as_bytes())),
            None => Message::text(text),
        };
        self.ws.send(message).await.expect("server send");
    }

    async fn send_raw(&mut self, message: Message) {
        self.ws.send(message).await.expect("server send raw");
    }

    async fn hello(&mut self, interval_ms: u64) {
        self.send(json!({ "op": 10, "d": { "heartbeat_interval": interval_ms } }))
            .await;
    }

    async fn dispatch(&mut self, t: &str, s: u64, d: Value) {
        self.send(json!({ "op": 0, "t": t, "s": s, "d": d })).await;
    }

    async fn close(&mut self, code: u16) {
        let frame = CloseFrame {
            code: CloseCode::from(code),
            reason: "test".into(),
        };
        self.ws.close(Some(frame)).await.expect("server close");
    }

    async fn recv(&mut self) -> Got {
        loop {
            let message = match timeout(WAIT, self.ws.next()).await.expect("server recv timeout") {
                None | Some(Err(_)) => return Got::Ended,
                Some(Ok(message)) => message,
            };
            match message {
                Message::Text(text) => {
                    assert!(self.zstd.is_none(), "text frame in zstd mode");
                    return Got::Json(serde_json::from_str(text.as_str()).expect("json"));
                }
                Message::Binary(data) => {
                    let zstd = self.zstd.as_mut().expect("binary frame without zstd");
                    if let Some(bytes) = zstd.decoder.decompress(&data).expect("inflate") {
                        return Got::Json(serde_json::from_slice(&bytes).expect("json"));
                    }
                }
                Message::Close(frame) => return Got::Close(frame.map(|f| u16::from(f.code))),
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }

    async fn recv_json(&mut self) -> Value {
        match self.recv().await {
            Got::Json(value) => value,
            other => panic!("expected json, got {other:?}"),
        }
    }

    async fn recv_op(&mut self, op: u64) -> Value {
        let value = self.recv_json().await;
        assert_eq!(value["op"], json!(op), "unexpected frame {value}");
        value
    }
}

/// In-memory connector: every connect attempt hands the server half to the test.
struct Fixture {
    handle: GatewayHandle,
    events: mpsc::Receiver<GatewayEvent>,
    accepts: mpsc::UnboundedReceiver<(Url, DuplexStream)>,
}

impl Fixture {
    fn start(config: GatewayConfig) -> Self {
        let (tx, accepts) = mpsc::unbounded_channel();
        let connect = move |url: Url| {
            let tx = tx.clone();
            async move {
                let (client, server) = tokio::io::duplex(1 << 20);
                tx.send((url.clone(), server))
                    .map_err(|_| anyhow::anyhow!("test fixture gone"))?;
                let (ws, _) = tokio_tungstenite::client_async(url.as_str(), client).await?;
                Ok(ws)
            }
        };
        let (handle, events) = spawn_with(config, connect).expect("spawn");
        Self {
            handle,
            events,
            accepts,
        }
    }

    async fn accept(&mut self) -> (Url, ServerConn<DuplexStream>) {
        let (url, stream) = timeout(WAIT, self.accepts.recv())
            .await
            .expect("accept timeout")
            .expect("connector dropped");
        let ws = tokio_tungstenite::accept_async(stream).await.expect("handshake");
        let zstd = url
            .query_pairs()
            .any(|(k, v)| k == "compress" && v == "zstd-stream")
            .then(ServerZstd::new);
        (url, ServerConn { ws, zstd })
    }

    async fn event(&mut self) -> GatewayEvent {
        timeout(WAIT, self.events.recv())
            .await
            .expect("event timeout")
            .expect("event channel closed")
    }

    /// HELLO -> IDENTIFY -> READY(session `sess-1`, s=1).
    async fn connect_ready(&mut self) -> ServerConn<DuplexStream> {
        let (_, mut server) = self.accept().await;
        server.hello(41_250).await;
        server.recv_op(2).await;
        server
            .dispatch("READY", 1, json!({ "session_id": "sess-1", "user": {}, "guilds": [] }))
            .await;
        assert!(matches!(self.event().await, GatewayEvent::Dispatch(d) if d.t == "READY"));
        assert_eq!(
            self.event().await,
            GatewayEvent::Ready {
                session_id: "sess-1".into()
            }
        );
        server
    }
}

async fn next(events: &mut mpsc::Receiver<GatewayEvent>) -> GatewayEvent {
    timeout(WAIT, events.recv())
        .await
        .expect("event timeout")
        .expect("event channel closed")
}

#[tokio::test]
async fn tcp_hello_identify_dispatch_lazy_request_and_close() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let mut cfg = config(Compression::None);
    cfg.url = Url::parse(&format!("ws://127.0.0.1:{port}/")).expect("url");
    let (handle, mut events) = spawn(cfg).expect("spawn");

    let (stream, _) = timeout(WAIT, listener.accept()).await.expect("accept").expect("tcp");
    let (uri_tx, uri_rx) = tokio::sync::oneshot::channel();
    let ws = tokio_tungstenite::accept_hdr_async(
        stream,
        move |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            let _ = uri_tx.send(request.uri().to_string());
            Ok(response)
        },
    )
    .await
    .expect("handshake");
    assert_eq!(uri_rx.await.expect("uri"), "/?v=1&encoding=json");
    let mut server = ServerConn { ws, zstd: None };

    server.hello(41_250).await;
    let identify = server.recv_op(2).await;
    let d = &identify["d"];
    assert_eq!(d["token"], json!(TOKEN));
    assert_eq!(d["properties"]["os"], json!("Windows"));
    assert_eq!(d["properties"]["browser"], json!("Fluxer Rust"));
    assert_eq!(d["properties"]["device"], json!("desktop"));
    assert_eq!(d["properties"]["locale"], json!("en-US"));
    assert_eq!(d["flags"], json!(14));
    assert_eq!(d["initial_guild_id"], json!("42"));
    assert_eq!(d["presence"]["status"], json!("online"));

    // Commands queued before READY are held until the session is up.
    handle
        .lazy_request(json!({ "42": { "active": true, "sync": true } }))
        .expect("lazy");

    server
        .dispatch("READY", 1, json!({ "session_id": "sess-tcp" }))
        .await;
    server
        .dispatch("MESSAGE_CREATE", 2, json!({ "id": "100", "content": "hi" }))
        .await;

    assert_eq!(
        next(&mut events).await,
        GatewayEvent::Dispatch(crate::Dispatch {
            t: "READY".into(),
            s: 1,
            d: json!({ "session_id": "sess-tcp" }),
        })
    );
    assert_eq!(
        next(&mut events).await,
        GatewayEvent::Ready {
            session_id: "sess-tcp".into()
        }
    );
    assert_eq!(
        next(&mut events).await,
        GatewayEvent::Dispatch(crate::Dispatch {
            t: "MESSAGE_CREATE".into(),
            s: 2,
            d: json!({ "id": "100", "content": "hi" }),
        })
    );

    let lazy = server.recv_op(14).await;
    assert_eq!(
        lazy["d"],
        json!({ "subscriptions": { "42": { "active": true, "sync": true } } })
    );

    handle.close();
    assert_eq!(server.recv().await, Got::Close(Some(1000)));
    assert!(timeout(WAIT, events.recv()).await.expect("end").is_none());
}

#[tokio::test(start_paused = true)]
async fn heartbeat_interval_seq_and_server_request() {
    let mut fx = Fixture::start(config(Compression::None));
    let (_, mut server) = fx.accept().await;
    server.hello(10_000).await;
    let hello_at = Instant::now();
    server.recv_op(2).await;

    // First heartbeat after 0.8 * interval + jitter (jitter <= min(1500, 5%) = 500).
    let hb = server.recv_op(1).await;
    let elapsed = hello_at.elapsed();
    assert!(
        (Duration::from_millis(8_000)..=Duration::from_millis(8_500)).contains(&elapsed),
        "first heartbeat after {elapsed:?}"
    );
    assert_eq!(hb["d"], Value::Null);
    server.send(json!({ "op": 11 })).await;

    server.dispatch("READY", 5, json!({ "session_id": "s" })).await;
    // A lower or missing seq never moves the counter backwards.
    server.dispatch("TYPING_START", 3, json!({})).await;
    server.send(json!({ "op": 0, "t": "X", "d": {} })).await;
    for _ in 0..2 {
        fx.event().await;
    }
    assert!(matches!(fx.event().await, GatewayEvent::Dispatch(d) if d.t == "TYPING_START" && d.s == 3));
    assert!(matches!(fx.event().await, GatewayEvent::Dispatch(d) if d.t == "X" && d.s == 5));

    let acked_at = Instant::now();
    let hb = server.recv_op(1).await;
    let elapsed = acked_at.elapsed();
    assert!(elapsed <= Duration::from_millis(8_500), "second heartbeat after {elapsed:?}");
    assert_eq!(hb["d"], json!(5));
    server.send(json!({ "op": 11 })).await;

    // Server-requested heartbeat is answered immediately.
    server.send(json!({ "op": 1, "d": null })).await;
    let asked_at = Instant::now();
    let hb = server.recv_op(1).await;
    assert!(asked_at.elapsed() < Duration::from_millis(100));
    assert_eq!(hb["d"], json!(5));
    server.send(json!({ "op": 11 })).await;

    // With ACKs flowing the connection stays up across several intervals.
    for _ in 0..3 {
        server.recv_op(1).await;
        server.send(json!({ "op": 11 })).await;
    }
    assert!(fx.events.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn missing_ack_reconnects_and_resumes() {
    let mut fx = Fixture::start(config(Compression::None));
    let mut server = fx.connect_ready().await;
    server.dispatch("MESSAGE_CREATE", 2, json!({})).await;
    fx.event().await;

    server.recv_op(1).await;
    let sent_at = Instant::now();
    // No ACK: the client gives up after 15 s and closes with 4000.
    assert_eq!(server.recv().await, Got::Close(Some(4000)));
    let elapsed = sent_at.elapsed();
    assert!(
        (Duration::from_secs(15)..Duration::from_secs(16)).contains(&elapsed),
        "ACK timeout after {elapsed:?}"
    );
    assert_eq!(
        fx.event().await,
        GatewayEvent::Reconnecting {
            code: None,
            attempt: 1
        }
    );

    let closed_at = Instant::now();
    let (_, mut server) = fx.accept().await;
    // Immediate reconnect: rand(0..=2000) ms, no exponential backoff.
    assert!(closed_at.elapsed() <= Duration::from_millis(2_000));
    server.hello(41_250).await;
    let resume = server.recv_op(6).await;
    assert_eq!(
        resume["d"],
        json!({ "token": TOKEN, "session_id": "sess-1", "seq": 2 })
    );

    server.dispatch("MESSAGE_CREATE", 3, json!({ "replayed": true })).await;
    server.dispatch("RESUMED", 4, json!(null)).await;
    assert!(matches!(fx.event().await, GatewayEvent::Dispatch(d) if d.s == 3));
    assert!(matches!(fx.event().await, GatewayEvent::Dispatch(d) if d.t == "RESUMED"));
    assert_eq!(fx.event().await, GatewayEvent::Resumed);
    // Presence is re-sent after RESUMED.
    let presence = server.recv_op(3).await;
    assert_eq!(presence["d"]["status"], json!("online"));
}

#[tokio::test(start_paused = true)]
async fn close_4004_is_fatal() {
    let mut fx = Fixture::start(config(Compression::None));
    let (_, mut server) = fx.accept().await;
    server.hello(41_250).await;
    server.recv_op(2).await;
    server.close(4004).await;

    assert_eq!(
        fx.event().await,
        GatewayEvent::Closed {
            code: 4004,
            reason: "test".into()
        }
    );
    assert!(timeout(WAIT, fx.events.recv()).await.expect("end").is_none());
    // The task ended without another connect attempt.
    assert!(timeout(WAIT, fx.accepts.recv()).await.expect("end").is_none());
}

#[tokio::test(start_paused = true)]
async fn close_4007_and_4009_discard_session_and_reidentify() {
    for code in [4007, 4009] {
        let mut fx = Fixture::start(config(Compression::None));
        let mut server = fx.connect_ready().await;
        server.close(code).await;
        assert_eq!(
            fx.event().await,
            GatewayEvent::Reconnecting {
                code: Some(code),
                attempt: 1
            }
        );
        let (_, mut server) = fx.accept().await;
        server.hello(41_250).await;
        let identify = server.recv_op(2).await;
        assert_eq!(identify["d"]["token"], json!(TOKEN), "close {code}");
    }
}

#[tokio::test(start_paused = true)]
async fn generic_close_resumes_with_backoff() {
    let mut fx = Fixture::start(config(Compression::None));
    let mut server = fx.connect_ready().await;
    // Let the connection age past the 1 s "recent attempt" window.
    tokio::time::sleep(Duration::from_secs(5)).await;
    server.close(4000).await;
    assert_eq!(
        fx.event().await,
        GatewayEvent::Reconnecting {
            code: Some(4000),
            attempt: 1
        }
    );
    let closed_at = Instant::now();
    let (_, mut server) = fx.accept().await;
    let waited = closed_at.elapsed();
    // n = 1: 500 + rand * (min(1000, cap) - 500).
    assert!(
        (Duration::from_millis(500)..=Duration::from_millis(1_000)).contains(&waited),
        "backoff {waited:?}"
    );
    server.hello(41_250).await;
    let resume = server.recv_op(6).await;
    assert_eq!(resume["d"]["session_id"], json!("sess-1"));
    assert_eq!(resume["d"]["seq"], json!(1));

    // A second failure before RESUMED keeps counting attempts.
    server.close(4001).await;
    assert_eq!(
        fx.event().await,
        GatewayEvent::Reconnecting {
            code: Some(4001),
            attempt: 2
        }
    );
    let (_, mut server) = fx.accept().await;
    server.hello(41_250).await;
    server.recv_op(6).await;
}

#[tokio::test(start_paused = true)]
async fn invalid_session_after_resume_identifies_on_same_connection() {
    let mut fx = Fixture::start(config(Compression::None));
    let mut server = fx.connect_ready().await;
    server.close(4000).await;
    fx.event().await;

    let (_, mut server) = fx.accept().await;
    server.hello(41_250).await;
    server.recv_op(6).await;
    server.send(json!({ "op": 9, "d": false })).await;
    let sent_at = Instant::now();
    assert_eq!(
        fx.event().await,
        GatewayEvent::InvalidSession { resumable: false }
    );
    let identify = server.recv_op(2).await;
    assert!(sent_at.elapsed() <= Duration::from_millis(1_000));
    assert_eq!(identify["d"]["token"], json!(TOKEN));
    server.dispatch("READY", 1, json!({ "session_id": "sess-2" })).await;
    fx.event().await;
    assert_eq!(
        fx.event().await,
        GatewayEvent::Ready {
            session_id: "sess-2".into()
        }
    );
}

#[tokio::test(start_paused = true)]
async fn invalid_session_while_connected_reconnects_and_identifies() {
    let mut fx = Fixture::start(config(Compression::None));
    let mut server = fx.connect_ready().await;
    server.send(json!({ "op": 9, "d": false })).await;
    let sent_at = Instant::now();
    assert_eq!(
        fx.event().await,
        GatewayEvent::InvalidSession { resumable: false }
    );
    assert_eq!(server.recv().await, Got::Close(Some(4000)));
    let elapsed = sent_at.elapsed();
    assert!(
        (Duration::from_millis(2_500)..=Duration::from_millis(3_500)).contains(&elapsed),
        "invalid session reconnect after {elapsed:?}"
    );
    assert!(matches!(
        fx.event().await,
        GatewayEvent::Reconnecting { code: None, .. }
    ));
    let (_, mut server) = fx.accept().await;
    server.hello(41_250).await;
    server.recv_op(2).await;
}

#[tokio::test(start_paused = true)]
async fn server_reconnect_op_resumes() {
    let mut fx = Fixture::start(config(Compression::None));
    let mut server = fx.connect_ready().await;
    server.send(json!({ "op": 7 })).await;
    assert_eq!(server.recv().await, Got::Close(Some(4000)));
    fx.event().await;
    let (_, mut server) = fx.accept().await;
    server.hello(41_250).await;
    server.recv_op(6).await;
}

#[tokio::test(start_paused = true)]
async fn zstd_stream_both_directions() {
    let mut fx = Fixture::start(config(Compression::ZstdStream));
    let (url, mut server) = fx.accept().await;
    assert_eq!(
        url.query(),
        Some("keep=1&v=1&encoding=json&compress=zstd-stream&stream=1")
    );
    server.hello(41_250).await;
    // `recv` rejects text frames in zstd mode and inflates with one context.
    let identify = server.recv_op(2).await;
    assert_eq!(identify["d"]["token"], json!(TOKEN));

    server.dispatch("READY", 1, json!({ "session_id": "z" })).await;
    let big = "x".repeat(50_000);
    server
        .dispatch("MESSAGE_CREATE", 2, json!({ "content": big }))
        .await;
    server.dispatch("MESSAGE_CREATE", 3, json!({ "content": "small" })).await;
    fx.event().await;
    fx.event().await;
    match fx.event().await {
        GatewayEvent::Dispatch(d) => {
            assert_eq!(d.s, 2);
            assert_eq!(d.d["content"].as_str().map(str::len), Some(50_000));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(fx.event().await, GatewayEvent::Dispatch(d) if d.s == 3));

    for guild in ["1", "2"] {
        fx.handle
            .lazy_request(json!({ guild: { "active": true } }))
            .expect("lazy");
        let lazy = server.recv_op(14).await;
        assert_eq!(lazy["d"]["subscriptions"][guild]["active"], json!(true));
    }
}

#[tokio::test(start_paused = true)]
async fn zstd_decode_failure_falls_back_to_uncompressed() {
    let mut fx = Fixture::start(config(Compression::ZstdStream));
    let (_, mut server) = fx.accept().await;
    server
        .send_raw(Message::binary(b"definitely not zstd".to_vec()))
        .await;
    assert_eq!(server.recv().await, Got::Close(Some(4000)));
    fx.event().await;
    let (url, mut server) = fx.accept().await;
    assert_eq!(url.query(), Some("keep=1&v=1&encoding=json"));
    server.hello(41_250).await;
    server.recv_op(2).await;
}

#[tokio::test(start_paused = true)]
async fn close_4002_with_zstd_falls_back_without_it_is_fatal() {
    let mut fx = Fixture::start(config(Compression::ZstdStream));
    let (_, mut server) = fx.accept().await;
    server.close(4002).await;
    fx.event().await;
    let (url, mut server) = fx.accept().await;
    assert!(!url.as_str().contains("compress"));
    server.close(4002).await;
    assert!(matches!(
        fx.event().await,
        GatewayEvent::Closed { code: 4002, .. }
    ));
}

#[tokio::test(start_paused = true)]
async fn oversized_payload_is_rejected_locally() {
    let fx = Fixture::start(config(Compression::None));
    let err = fx
        .handle
        .lazy_request(json!({ "1": { "members": ["9".repeat(MAX_CLIENT_PAYLOAD_BYTES)] } }))
        .expect_err("too large");
    assert!(matches!(err, GatewayError::PayloadTooLarge { .. }));
    assert!(encode_frame(op::LAZY_REQUEST, &json!({ "a": "b" })).is_ok());
}

#[tokio::test(start_paused = true)]
async fn token_never_in_debug_output() {
    let cfg = config(Compression::ZstdStream);
    let debug = format!("{cfg:?} {cfg:#?}");
    assert!(!debug.contains(TOKEN), "{debug}");
    assert!(debug.contains("<redacted>"));

    let fx = Fixture::start(cfg);
    let debug = format!("{:?}", fx.handle);
    assert!(!debug.contains(TOKEN));
    let err = GatewayError::PayloadTooLarge { size: 20_000 };
    assert!(!format!("{err} {err:?}").contains(TOKEN));
}

#[test]
fn zstd_stream_keeps_one_context_per_direction() {
    let mut deflater = ZstdDeflater::new().expect("deflater");
    let first = deflater.compress(br#"{"op":1,"d":null}"#).expect("first");
    let second = deflater.compress(br#"{"op":1,"d":null}"#).expect("second");

    let mut inflater = ZstdInflater::new().expect("inflater");
    assert_eq!(
        inflater.decompress(&first).expect("first").as_deref(),
        Some(&br#"{"op":1,"d":null}"#[..])
    );
    assert_eq!(
        inflater.decompress(&second).expect("second").as_deref(),
        Some(&br#"{"op":1,"d":null}"#[..])
    );
    // The second chunk continues the first frame; a fresh context cannot read it.
    let mut fresh = ZstdInflater::new().expect("fresh");
    assert!(fresh.decompress(&second).is_err());
}

#[test]
fn zstd_decoder_accepts_window_log_21() {
    let mut encoder = ZstdDeflater::with_window_log(21).expect("encoder");
    let payload = vec![b'a'; 300_000];
    let compressed = encoder.compress(&payload).expect("compress");
    let mut inflater = ZstdInflater::new().expect("inflater");
    assert_eq!(inflater.decompress(&compressed).expect("inflate"), Some(payload));
}

#[test]
fn gateway_url_sets_protocol_params() {
    let base = Url::parse("wss://gw.example/path?v=2&compress=x&region=eu").expect("url");
    assert_eq!(
        gateway_url(&base, Compression::None).as_str(),
        "wss://gw.example/path?region=eu&v=1&encoding=json"
    );
    assert_eq!(
        gateway_url(&base, Compression::ZstdStream).as_str(),
        "wss://gw.example/path?region=eu&v=1&encoding=json&compress=zstd-stream&stream=1"
    );
}

#[test]
fn heartbeat_delay_matches_formula() {
    for _ in 0..200 {
        let delay = connection::heartbeat_delay(41_250).as_millis();
        assert!((33_000..=34_500).contains(&delay), "{delay}");
        let small = connection::heartbeat_delay(500).as_millis();
        assert!((1_000..=1_025).contains(&small), "{small}");
    }
    // Absurd intervals must not overflow.
    let _ = connection::heartbeat_delay(u64::MAX);
}

#[test]
fn spawn_rejects_non_ws_url() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let mut cfg = config(Compression::None);
    cfg.url = Url::parse("https://gw.example/").expect("url");
    rt.block_on(async {
        assert!(matches!(spawn(cfg), Err(GatewayError::InvalidUrl(_))));
    });
    assert!(matches!(
        spawn(config(Compression::None)),
        Err(GatewayError::NoRuntime)
    ));
}

/// A server that refuses at once must still back off to the 10 s ceiling:
/// only failures within 1 s of the last counted backoff skip counting.
#[test]
fn backoff_grows_when_connects_fail_immediately() {
    use super::connection::Backoff;
    let mut backoff = Backoff::default();
    let mut now = Instant::now();
    let mut ceilings = Vec::new();
    for _ in 0..6 {
        let (low, high) = backoff.next_bounds(now);
        assert_eq!(low, 500);
        ceilings.push(high);
        // The refused connect fails right after the (max) wait.
        now += Duration::from_millis(high + 5);
    }
    assert_eq!(ceilings, [1_000, 2_000, 4_000, 8_000, 10_000, 10_000]);
    // A failure within 1 s of the last backoff waits 1-3 s, uncounted.
    assert_eq!(backoff.next_bounds(now + Duration::from_millis(10)), (500, 10_000));
    assert_eq!(backoff.next_bounds(now + Duration::from_millis(20)), (1_000, 3_000));
    backoff.reset();
    assert_eq!(backoff.next_bounds(now + Duration::from_secs(5)), (500, 1_000), "READY resets");
}
