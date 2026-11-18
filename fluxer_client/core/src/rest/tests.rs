//! REST/auth tests against a hand-rolled local HTTP/1.1 fixture.
//! Every token below is fake test data.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::client::{SERVER_GLOBAL_LIMIT_PER_SECOND, client_properties_header, parse_rate_limit};
use super::discovery::parse_document_for_test;
use super::*;

/// Fake token in the server's `^flx_[A-Za-z0-9]{36}$` shape.
const FAKE_TOKEN: &str = "flx_TESTONLYxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
const FAKE_PASSWORD: &str = "test-only-password-1";
const FAKE_TICKET: &str = "test-only-mfa-ticket";
const CHANNEL: &str = "123456789012345678";
const MESSAGE: &str = "223456789012345678";

// ---------------------------------------------------------------- fixture

#[derive(Debug, Clone)]
struct Req {
    method: String,
    /// Path plus query, as sent on the request line.
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

struct Resp {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// Send no Content-Length; the body ends when the connection closes.
    omit_length: bool,
}

impl Resp {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.to_string().into_bytes(),
            omit_length: false,
        }
    }

    fn empty(status: u16) -> Self {
        Self { status, headers: Vec::new(), body: Vec::new(), omit_length: false }
    }

    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

type Handler = Arc<dyn Fn(&Req, usize) -> Resp + Send + Sync>;

struct Fixture {
    origin: String,
    requests: Arc<Mutex<Vec<Req>>>,
}

impl Fixture {
    /// `handler(request, index)` where `index` counts requests from 0.
    async fn start(handler: impl Fn(&Req, usize) -> Resp + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests: Arc<Mutex<Vec<Req>>> = Arc::default();
        let handler: Handler = Arc::new(handler);
        let log = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { return };
                let handler = handler.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let Some(req) = read_request(&mut stream).await else { return };
                    let index = {
                        let mut log = log.lock().unwrap();
                        log.push(req.clone());
                        log.len() - 1
                    };
                    let resp = handler(&req, index);
                    let mut head = format!("HTTP/1.1 {} X\r\nConnection: close\r\n", resp.status);
                    if !resp.omit_length {
                        head.push_str(&format!("Content-Length: {}\r\n", resp.body.len()));
                    }
                    for (k, v) in &resp.headers {
                        head.push_str(&format!("{k}: {v}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(&resp.body).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self { origin: format!("http://{addr}"), requests }
    }

    fn requests(&self) -> Vec<Req> {
        self.requests.lock().unwrap().clone()
    }

    fn api_base(&self) -> String {
        format!("{}/api", self.origin)
    }

    fn client(&self) -> RestClient {
        RestClient::new(test_config(&self.api_base())).unwrap()
    }

    fn discovery_doc(&self) -> Value {
        json!({
            "api_code_version": 1,
            "endpoints": {
                "api": self.api_base(),
                "api_client": self.api_base(),
                "gateway": format!("ws://{}/gateway", self.origin.trim_start_matches("http://")),
                "media": format!("{}/media", self.origin),
                "static_cdn": format!("{}/static", self.origin),
                "invite": format!("{}/invite", self.origin),
                "webapp": self.origin.clone(),
            }
        })
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    let target = request_line.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Req { method, target, headers, body })
}

fn loopback_policy() -> TransportPolicy {
    TransportPolicy { allow_insecure_loopback: true }
}

fn test_config(api_base: &str) -> RestConfig {
    let endpoints = Endpoints {
        api_code_version: 1,
        api: api_base.to_owned(),
        gateway: "ws://127.0.0.1:1/gateway".to_owned(),
        media: None,
        static_cdn: None,
        upload_relay: None,
        invite: None,
        webapp: None,
        discovery_url: String::new(),
        document: Value::Null,
    };
    let mut config = RestConfig::new(&endpoints, ClientIdentity::default());
    config.policy = loopback_policy();
    config
}

async fn authed(fixture: &Fixture) -> RestClient {
    let client = fixture.client();
    client.set_token(Some(SecretToken::new(FAKE_TOKEN))).await;
    client
}

fn loopback_options() -> DiscoveryOptions {
    DiscoveryOptions { policy: loopback_policy(), release_channel: ReleaseChannel::Stable }
}

// --------------------------------------------------------------- discovery

#[tokio::test]
async fn discovery_happy_path_uses_api_client_and_json_accept() {
    let doc = Arc::new(Mutex::new(Value::Null));
    let doc_for_server = doc.clone();
    let fixture = Fixture::start(move |req, _| {
        if req.target == DISCOVERY_PATH {
            Resp::json(200, doc_for_server.lock().unwrap().clone())
        } else {
            Resp::empty(404)
        }
    })
    .await;
    *doc.lock().unwrap() = fixture.discovery_doc();

    let endpoints = discover_with(&fixture.origin, &loopback_options()).await.unwrap();
    assert_eq!(endpoints.api, fixture.api_base());
    assert_eq!(endpoints.api_code_version, 1);
    assert!(endpoints.gateway.starts_with("ws://127.0.0.1:"));
    assert_eq!(endpoints.discovery_url, format!("{}{DISCOVERY_PATH}", fixture.origin));

    let requests = fixture.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].header("accept"), Some("application/json"));
    assert_eq!(requests[0].header("authorization"), None);

    // RestConfig built from discovery targets <api_client>/v1.
    let config = RestConfig::new(&endpoints, ClientIdentity::default());
    assert_eq!(config.api_base, fixture.api_base());
    assert_eq!(config.api_code_version, 1);
}

#[tokio::test]
async fn discovery_falls_back_to_api_candidate() {
    let doc = Arc::new(Mutex::new(Value::Null));
    let doc_for_server = doc.clone();
    let fixture = Fixture::start(move |req, _| {
        if req.target == "/api/.well-known/fluxer" {
            Resp::json(200, doc_for_server.lock().unwrap().clone())
        } else {
            Resp::empty(404)
        }
    })
    .await;
    *doc.lock().unwrap() = fixture.discovery_doc();
    let endpoints = discover_with(&fixture.origin, &loopback_options()).await.unwrap();
    assert_eq!(endpoints.discovery_url, format!("{}/api/.well-known/fluxer", fixture.origin));
    let targets: Vec<String> = fixture.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(targets, vec![DISCOVERY_PATH.to_owned(), "/api/.well-known/fluxer".to_owned()]);
}

#[tokio::test]
async fn discovery_rejects_plain_http_without_loopback_opt_in() {
    let fixture = Fixture::start(|_, _| Resp::empty(500)).await;
    let error = discover_with(&fixture.origin, &DiscoveryOptions::default()).await.unwrap_err();
    assert!(matches!(error, DiscoveryError::InsecureUrl(_)), "{error:?}");
    assert!(fixture.requests().is_empty(), "no request may be sent over plain http");
}

#[tokio::test]
async fn discovery_rejects_plain_http_to_non_loopback_even_with_opt_in() {
    let error = discover_with("http://example.com", &loopback_options()).await.unwrap_err();
    assert!(matches!(error, DiscoveryError::InsecureUrl(_)), "{error:?}");
}

#[tokio::test]
async fn discovery_rejects_oversized_documents() {
    // Both with a declared Content-Length and streamed without one.
    for omit_length in [false, true] {
        let fixture = Fixture::start(move |_, _| {
            let mut resp = Resp::json(200, json!({}));
            resp.body = vec![b' '; DISCOVERY_MAX_BYTES + 16];
            resp.omit_length = omit_length;
            resp
        })
        .await;
        let error = discover_with(&fixture.origin, &loopback_options()).await.unwrap_err();
        let DiscoveryError::Unreachable(attempts) = &error else { panic!("{error:?}") };
        assert!(!attempts.is_empty());
        assert!(attempts.iter().all(|a| a.reason.contains("byte cap")), "{attempts:?}");
    }
}

#[tokio::test]
async fn discovery_rejects_non_json_content_type() {
    let doc = Arc::new(Mutex::new(Value::Null));
    let doc_for_server = doc.clone();
    let fixture = Fixture::start(move |_, _| {
        let mut resp = Resp::json(200, doc_for_server.lock().unwrap().clone());
        resp.headers = vec![("Content-Type".into(), "text/html".into())];
        resp
    })
    .await;
    *doc.lock().unwrap() = fixture.discovery_doc();
    let error = discover_with(&fixture.origin, &loopback_options()).await.unwrap_err();
    let DiscoveryError::Unreachable(attempts) = &error else { panic!("{error:?}") };
    assert!(attempts.iter().all(|a| a.reason.contains("JSON content type")), "{attempts:?}");
}

#[tokio::test]
async fn discovery_does_not_follow_redirects() {
    let fixture = Fixture::start(|_, _| Resp::empty(302).header("Location", "http://127.0.0.1:1/.well-known/fluxer")).await;
    let error = discover_with(&fixture.origin, &loopback_options()).await.unwrap_err();
    let DiscoveryError::Unreachable(attempts) = &error else { panic!("{error:?}") };
    assert!(attempts.iter().all(|a| a.reason.contains("HTTP 302")), "{attempts:?}");
}

#[tokio::test]
async fn discovery_rejects_endpoint_on_another_origin() {
    let doc = Arc::new(Mutex::new(Value::Null));
    let doc_for_server = doc.clone();
    let fixture = Fixture::start(move |_, _| Resp::json(200, doc_for_server.lock().unwrap().clone())).await;
    let mut document = fixture.discovery_doc();
    document["endpoints"]["api_client"] = json!("https://evil.example/api");
    *doc.lock().unwrap() = document;
    let error = discover_with(&fixture.origin, &loopback_options()).await.unwrap_err();
    let DiscoveryError::Unreachable(attempts) = &error else { panic!("{error:?}") };
    assert!(attempts.iter().all(|a| a.reason.contains("another origin")), "{attempts:?}");
}

#[test]
fn discovery_document_validation() {
    let served = "https://chat.example/.well-known/fluxer";
    let policy = TransportPolicy::default();
    let good = json!({
        "api_code_version": 1,
        "endpoints": {
            "api": "https://chat.example/api",
            "api_client": "https://chat.example/client-api/",
            "gateway": "wss://gateway.chat.example",
        }
    });
    let endpoints = parse_document_for_test(served, good.clone(), &policy).unwrap();
    assert_eq!(endpoints.api, "https://chat.example/client-api", "api_client wins, trailing slash stripped");

    let mut no_version = good.clone();
    no_version["api_code_version"] = json!(0);
    assert!(parse_document_for_test(served, no_version, &policy).unwrap_err().contains("api_code_version"));

    let mut bad_gateway = good.clone();
    bad_gateway["endpoints"]["gateway"] = json!("https://gateway.chat.example");
    assert!(parse_document_for_test(served, bad_gateway, &policy).unwrap_err().contains("gateway"));

    let mut insecure_gateway = good.clone();
    insecure_gateway["endpoints"]["gateway"] = json!("ws://gateway.chat.example");
    assert!(parse_document_for_test(served, insecure_gateway, &policy).unwrap_err().contains("insecure"));

    let mut newer = good;
    newer["codename"] = json!("osmium");
    assert!(parse_document_for_test(served, newer, &policy).unwrap_err().contains("osmium"));
}

#[test]
fn discovery_input_and_candidates() {
    assert_eq!(
        resolve_instance_input("fluxer.app", ReleaseChannel::Stable).unwrap(),
        "https://web.fluxer.app/api"
    );
    assert_eq!(
        resolve_instance_input("fluxer.app", ReleaseChannel::Canary).unwrap(),
        "https://web.canary.fluxer.app/api"
    );
    assert_eq!(resolve_instance_input("chat.example/", ReleaseChannel::Stable).unwrap(), "https://chat.example");
    assert!(resolve_instance_input("/relative", ReleaseChannel::Stable).is_err());
    assert!(resolve_instance_input("https://user:pw@chat.example", ReleaseChannel::Stable).is_err());
    assert_eq!(
        discovery_candidates("https://chat.example/custom"),
        vec![
            "https://chat.example/.well-known/fluxer",
            "https://chat.example/custom/.well-known/fluxer",
            "https://chat.example/api/.well-known/fluxer",
        ]
    );
    assert_eq!(DISCOVERY_TIMEOUT, Duration::from_secs(15));
    assert_eq!(DISCOVERY_MAX_BYTES, 1024 * 1024);
}

// ----------------------------------------------------------------- headers

#[tokio::test]
async fn requests_carry_raw_token_and_fluxer_headers() {
    let fixture = Fixture::start(|_, _| Resp::json(200, json!({ "id": "1" }))).await;
    let client = authed(&fixture).await;
    client.get_me().await.unwrap();

    let requests = fixture.requests();
    let req = &requests[0];
    assert_eq!(req.method, "GET");
    assert_eq!(req.target, "/api/v1/users/@me");
    assert_eq!(req.header("authorization"), Some(FAKE_TOKEN), "raw token, no Bearer prefix");
    assert_eq!(req.header("x-fluxer-features"), Some("view_channel_members_permission,channel_threads"));
    assert_eq!(req.header("x-fluxer-capabilities"), Some("guild_visits"));
    assert_eq!(req.header("x-fluxer-client-installation-id"), Some(client.installation_id()));
    assert_eq!(req.header("x-fluxer-client-properties"), Some(client_properties_header(&ClientIdentity::default()).as_str()));
    assert_eq!(req.header("accept"), Some("application/json"));
    assert!(req.header("user-agent").unwrap().starts_with("Fluxer Desktop/"));
    assert_eq!(req.header("x-captcha-token"), None);
}

#[tokio::test]
async fn auth_endpoints_never_send_the_token_and_authed_calls_require_one() {
    let fixture = Fixture::start(|_, _| {
        Resp::json(200, json!({ "token": FAKE_TOKEN, "user_id": "1", "user": {} }))
    })
    .await;
    let client = fixture.client();
    assert!(matches!(client.get_me().await, Err(RestError::NotAuthenticated)));
    assert!(fixture.requests().is_empty());

    client.set_token(Some(SecretToken::new("flx_OTHERxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"))).await;
    client.login("user@example.test", &Secret::new(FAKE_PASSWORD)).await.unwrap();
    assert_eq!(fixture.requests()[0].header("authorization"), None);
}

#[test]
fn client_config_validation() {
    let mut config = test_config("http://127.0.0.1:9/api");
    assert!(config.max_requests_per_second < SERVER_GLOBAL_LIMIT_PER_SECOND);
    config.max_requests_per_second = SERVER_GLOBAL_LIMIT_PER_SECOND;
    assert!(matches!(RestClient::new(config.clone()), Err(RestError::InvalidArgument(_))));
    config.max_requests_per_second = MAX_REQUESTS_PER_SECOND;
    assert!(RestClient::new(config.clone()).is_ok());

    config.policy = TransportPolicy::default();
    assert!(matches!(RestClient::new(config.clone()), Err(RestError::InsecureUrl(_))));
    config.api_base = "http://example.com/api".to_owned();
    config.policy = loopback_policy();
    assert!(matches!(RestClient::new(config.clone()), Err(RestError::InsecureUrl(_))));
    config.api_base = "https://example.com/api".to_owned();
    config.identity.installation_id = Some("not-hex".to_owned());
    assert!(matches!(RestClient::new(config), Err(RestError::InvalidArgument(_))));
}

// -------------------------------------------------------------- rate limits

#[test]
fn rate_limit_header_priority() {
    let headers = |pairs: &[(&'static str, &'static str)]| {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(*k, v.parse().unwrap());
        }
        map
    };
    let body = json!({ "retry_after": 7.0, "global": false });

    let info = parse_rate_limit(&headers(&[("x-ratelimit-reset-after", "0.5"), ("retry-after", "30")]), &body);
    assert_eq!(info.wait, Duration::from_millis(500), "Reset-After beats Retry-After");
    let info = parse_rate_limit(&headers(&[("retry-after", "3")]), &body);
    assert_eq!(info.wait, Duration::from_secs(3), "Retry-After beats body");
    let info = parse_rate_limit(&headers(&[]), &body);
    assert_eq!(info.wait, Duration::from_secs(7), "body retry_after as fallback");
    let info = parse_rate_limit(&headers(&[("x-ratelimit-reset-after", "0")]), &Value::Null);
    assert_eq!(info.wait, Duration::from_secs(1), "non-positive or missing -> 1 s");
    assert!(!info.global);
    let info = parse_rate_limit(&headers(&[("x-ratelimit-global", "true")]), &Value::Null);
    assert!(info.global);
}

#[tokio::test]
async fn rate_limited_request_waits_reset_after_then_retries() {
    let fixture = Fixture::start(|_, index| {
        if index == 0 {
            Resp::json(429, json!({ "code": "RATE_LIMITED", "message": "slow down", "retry_after": 30.0, "global": false }))
                .header("X-RateLimit-Reset-After", "0.3")
                .header("Retry-After", "30")
        } else {
            Resp::json(200, json!({ "id": "1" }))
        }
    })
    .await;
    let client = authed(&fixture).await;
    let started = std::time::Instant::now();
    client.get_me().await.unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(300), "waited {elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "used Retry-After instead of Reset-After: {elapsed:?}");
    assert_eq!(fixture.requests().len(), 2);
}

#[tokio::test]
async fn retry_aborts_when_the_token_was_cleared_during_the_wait() {
    let fixture = Fixture::start(|_, _| {
        Resp::json(429, json!({ "code": "RATE_LIMITED", "message": "", "retry_after": 1.0, "global": false }))
            .header("X-RateLimit-Reset-After", "0.5")
    })
    .await;
    let client = Arc::new(authed(&fixture).await);
    let clearer = client.clone();
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        clearer.set_token(None).await;
    });
    let result = client.get_me().await;
    task.await.unwrap();
    assert!(matches!(result, Err(RestError::NotAuthenticated)), "{result:?}");
    assert_eq!(fixture.requests().len(), 1, "no request may be resent with the old token");
}

#[tokio::test]
async fn long_rate_limit_fails_fast_and_blocks_the_bucket() {
    let fixture = Fixture::start(|_, _| {
        Resp::json(429, json!({ "code": "RATE_LIMITED", "message": "", "retry_after": 60.0, "global": false }))
            .header("X-RateLimit-Reset-After", "60")
    })
    .await;
    let client = authed(&fixture).await;
    let error = client.get_me().await.unwrap_err();
    let RestError::RateLimited { retry_after, global } = error else { panic!("{error:?}") };
    assert_eq!(retry_after, Duration::from_secs(60));
    assert!(!global);
    // Same bucket: answered locally without contacting the server.
    assert!(matches!(client.get_me().await, Err(RestError::RateLimited { .. })));
    assert_eq!(fixture.requests().len(), 1);
}

#[tokio::test]
async fn global_rate_limit_blocks_every_bucket() {
    let fixture = Fixture::start(|_, _| {
        Resp::json(429, json!({ "code": "RATE_LIMITED", "message": "", "retry_after": 60.0, "global": true }))
            .header("X-RateLimit-Global", "true")
            .header("Retry-After", "60")
    })
    .await;
    let client = authed(&fixture).await;
    let error = client.get_me().await.unwrap_err();
    assert!(matches!(error, RestError::RateLimited { global: true, .. }), "{error:?}");
    let error = client.get_messages(CHANNEL, &MessageQuery::default()).await.unwrap_err();
    assert!(matches!(error, RestError::RateLimited { global: true, .. }), "{error:?}");
    assert_eq!(fixture.requests().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn pacer_keeps_every_one_second_window_under_the_limit() {
    let mut config = test_config("https://chat.example/api");
    config.max_requests_per_second = MAX_REQUESTS_PER_SECOND;
    let limit = usize::try_from(config.max_requests_per_second).unwrap();
    let client = RestClient::new(config).unwrap();

    let origin = tokio::time::Instant::now();
    let mut starts = Vec::new();
    for i in 0..150 {
        client.wait_for_slot(&format!("GET /bucket/{}", i % 3)).await.unwrap();
        starts.push(tokio::time::Instant::now());
    }
    // In any half-open 1 s window there are at most `limit` (<= 40) starts.
    for window in starts.windows(limit + 1) {
        assert!(window[limit] - window[0] >= Duration::from_secs(1));
    }
    let total = starts[starts.len() - 1] - origin;
    assert!(total >= Duration::from_secs(3), "150 starts at 40/s need >= 3 s, took {total:?}");
    assert!(total < Duration::from_secs(4), "pacer is too slow: {total:?}");
}

#[tokio::test(start_paused = true)]
async fn pacer_lets_a_burst_through_under_the_limit() {
    let client = RestClient::new(test_config("https://chat.example/api")).unwrap();
    let origin = tokio::time::Instant::now();
    for _ in 0..DEFAULT_REQUESTS_PER_SECOND {
        client.wait_for_slot("GET /x").await.unwrap();
    }
    assert_eq!(tokio::time::Instant::now() - origin, Duration::ZERO);
    client.wait_for_slot("GET /x").await.unwrap();
    assert_eq!(tokio::time::Instant::now() - origin, Duration::from_secs(1));
}

// ------------------------------------------------------------------- login

#[tokio::test]
async fn login_success_returns_token_outcome() {
    let fixture = Fixture::start(|_, _| {
        Resp::json(200, json!({ "token": FAKE_TOKEN, "user_id": "42", "user": { "id": "42", "email": "user@example.test" } }))
    })
    .await;
    let client = fixture.client();
    let outcome = client.login("user@example.test", &Secret::new(FAKE_PASSWORD)).await.unwrap();
    let LoginOutcome::Token(success) = &outcome else { panic!("{outcome:?}") };
    assert_eq!(success.token.expose(), FAKE_TOKEN);
    assert_eq!(success.user_id, "42");

    let req = &fixture.requests()[0];
    assert_eq!((req.method.as_str(), req.target.as_str()), ("POST", "/api/v1/auth/login"));
    assert_eq!(req.header("content-type"), Some("application/json"));
    assert_eq!(req.json(), json!({ "email": "user@example.test", "password": FAKE_PASSWORD }));

    let debug = format!("{outcome:?}");
    assert!(!debug.contains(FAKE_TOKEN) && !debug.contains("user@example.test"), "{debug}");
    assert!(!client.has_token().await, "login does not store the token by itself");
}

#[tokio::test]
async fn login_mfa_then_totp() {
    let fixture = Fixture::start(|req, _| match req.target.as_str() {
        "/api/v1/auth/login" => Resp::json(
            200,
            json!({ "mfa": true, "ticket": FAKE_TICKET, "allowed_methods": ["totp", "webauthn"], "totp": true, "webauthn": true, "backup_codes": false }),
        ),
        "/api/v1/auth/login/mfa/totp" => Resp::json(200, json!({ "token": FAKE_TOKEN, "user_id": "42", "user": {} })),
        _ => Resp::empty(404),
    })
    .await;
    let client = fixture.client();
    let outcome = client.login("user@example.test", &Secret::new(FAKE_PASSWORD)).await.unwrap();
    let LoginOutcome::Mfa { ticket, methods } = &outcome else { panic!("{outcome:?}") };
    assert_eq!(ticket.expose(), FAKE_TICKET);
    assert_eq!(methods.allowed, vec!["totp", "webauthn"]);
    assert!(methods.totp && methods.webauthn && !methods.backup_codes);
    assert!(!format!("{outcome:?}").contains(FAKE_TICKET));

    let outcome = client.login_mfa_totp(ticket, &Secret::new(" 123456 ")).await.unwrap();
    assert!(matches!(outcome, LoginOutcome::Token(_)), "{outcome:?}");
    let req = &fixture.requests()[1];
    assert_eq!(req.target, "/api/v1/auth/login/mfa/totp");
    assert_eq!(req.json(), json!({ "code": "123456", "ticket": FAKE_TICKET }));
}

#[tokio::test]
async fn login_captcha_required_then_retry_with_token() {
    let fixture = Fixture::start(|req, _| {
        if req.header("x-captcha-token").is_some() {
            Resp::json(200, json!({ "token": FAKE_TOKEN, "user_id": "42", "user": {} }))
        } else {
            Resp::json(
                400,
                json!({
                    "code": "CAPTCHA_REQUIRED",
                    "message": "captcha required",
                    "captcha_provider": "altcha",
                    "altcha_challenge": { "parameters": { "algorithm": "PBKDF2/SHA-256", "cost": 1000 }, "signature": "sig" }
                }),
            )
        }
    })
    .await;
    let client = fixture.client();
    let password = Secret::new(FAKE_PASSWORD);
    let outcome = client.login("user@example.test", &password).await.unwrap();
    let LoginOutcome::CaptchaRequired { code, provider, challenge } = &outcome else { panic!("{outcome:?}") };
    assert_eq!(code, "CAPTCHA_REQUIRED");
    assert_eq!(provider.as_deref(), Some("altcha"));
    assert_eq!(challenge.as_ref().unwrap()["signature"], "sig");

    let solved = Secret::new("dGVzdC1vbmx5LWNhcHRjaGE=");
    let outcome = client.login_with_captcha("user@example.test", &password, &solved).await.unwrap();
    assert!(matches!(outcome, LoginOutcome::Token(_)));
    assert_eq!(fixture.requests()[1].header("x-captcha-token"), Some("dGVzdC1vbmx5LWNhcHRjaGE="));
}

#[tokio::test]
async fn login_ip_authorization_and_plain_errors() {
    let fixture = Fixture::start(|_, index| {
        if index == 0 {
            Resp::json(
                403,
                json!({ "code": "IP_AUTHORIZATION_REQUIRED", "message": "", "ticket": FAKE_TICKET, "email": "user@example.test", "resend_available_in": 30 }),
            )
        } else {
            Resp::json(400, json!({ "code": "INVALID_FORM_BODY", "message": "bad", "errors": [{ "path": "password", "code": "X", "message": "wrong" }] }))
        }
    })
    .await;
    let client = fixture.client();
    let outcome = client.login("user@example.test", &Secret::new(FAKE_PASSWORD)).await.unwrap();
    assert!(matches!(outcome, LoginOutcome::IpAuthorizationRequired { resend_available_in: Some(30) }), "{outcome:?}");
    let error = client.login("user@example.test", &Secret::new(FAKE_PASSWORD)).await.unwrap_err();
    assert_eq!(error.code(), Some("INVALID_FORM_BODY"));
    assert_eq!(error.status(), Some(400));
    let RestError::Api(api) = error else { unreachable!() };
    assert_eq!(api.errors[0].path, "password");
}

#[tokio::test]
async fn logout_clears_the_token() {
    let fixture = Fixture::start(|_, _| Resp::empty(204)).await;
    let client = authed(&fixture).await;
    client.logout().await.unwrap();
    assert!(!client.has_token().await);
    let req = &fixture.requests()[0];
    assert_eq!((req.method.as_str(), req.target.as_str()), ("POST", "/api/v1/auth/logout"));
    assert_eq!(req.header("authorization"), Some(FAKE_TOKEN));
}

// ---------------------------------------------------------------- messages

#[tokio::test]
async fn get_messages_builds_query() {
    let fixture = Fixture::start(|_, _| Resp::json(200, json!([]))).await;
    let client = authed(&fixture).await;
    let query = MessageQuery { before: Some(MESSAGE.to_owned()), limit: Some(50), ..MessageQuery::default() };
    let messages = client.get_messages(CHANNEL, &query).await.unwrap();
    assert_eq!(messages, json!([]));
    assert_eq!(
        fixture.requests()[0].target,
        format!("/api/v1/channels/{CHANNEL}/messages?before={MESSAGE}&limit=50")
    );
    let around = MessageQuery { around: Some(MESSAGE.to_owned()), after: Some(MESSAGE.to_owned()), ..MessageQuery::default() };
    client.get_messages(CHANNEL, &around).await.unwrap();
    assert_eq!(
        fixture.requests()[1].target,
        format!("/api/v1/channels/{CHANNEL}/messages?after={MESSAGE}&around={MESSAGE}")
    );

    let bad_limit = MessageQuery { limit: Some(101), ..MessageQuery::default() };
    assert!(matches!(client.get_messages(CHANNEL, &bad_limit).await, Err(RestError::InvalidArgument(_))));
    assert!(matches!(client.get_messages("../users", &MessageQuery::default()).await, Err(RestError::InvalidArgument(_))));
    assert_eq!(fixture.requests().len(), 2);
}

#[tokio::test]
async fn send_message_posts_content_and_nonce() {
    let fixture = Fixture::start(|req, _| Resp::json(200, json!({ "id": MESSAGE, "nonce": req.json()["nonce"] }))).await;
    let client = authed(&fixture).await;
    let sent = client.send_message(CHANNEL, "hello", "1300000000000000000").await.unwrap();
    assert_eq!(sent["nonce"], "1300000000000000000");
    let req = &fixture.requests()[0];
    assert_eq!((req.method.as_str(), req.target.as_str()), ("POST", format!("/api/v1/channels/{CHANNEL}/messages").as_str()));
    assert_eq!(req.json(), json!({ "content": "hello", "nonce": "1300000000000000000" }));
    assert!(matches!(client.send_message(CHANNEL, "x", "").await, Err(RestError::InvalidArgument(_))));
    assert!(matches!(client.send_message(CHANNEL, "x", &"9".repeat(33)).await, Err(RestError::InvalidArgument(_))));
}

#[tokio::test]
async fn ack_uses_read_states_and_falls_back_on_404() {
    let fixture = Fixture::start(|req, _| {
        if req.target == "/api/v1/read-states/ack" { Resp::empty(404) } else { Resp::empty(204) }
    })
    .await;
    let client = authed(&fixture).await;
    let ack = AckRequest { channel_id: CHANNEL.into(), message_id: MESSAGE.into(), mention_count: Some(0), manual: false };
    assert_eq!(client.ack_message(&ack).await.unwrap(), Value::Null);
    let requests = fixture.requests();
    assert_eq!(requests[0].target, "/api/v1/read-states/ack");
    assert_eq!(
        requests[0].json(),
        json!({ "read_states": [{ "channel_id": CHANNEL, "message_id": MESSAGE, "mention_count": 0 }] })
    );
    assert_eq!(requests[1].target, "/api/v1/read-states/ack-bulk");
    assert_eq!(requests[1].json(), json!({ "read_states": [{ "channel_id": CHANNEL, "message_id": MESSAGE }] }));
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn ack_fallback_splits_automatic_and_manual_like_read_states_ts() {
    let fixture = Fixture::start(|req, _| {
        if req.target == "/api/v1/read-states/ack" { Resp::empty(405) } else { Resp::empty(204) }
    })
    .await;
    let client = authed(&fixture).await;
    let other_channel = "323456789012345678";
    let acks = [
        AckRequest { channel_id: CHANNEL.into(), message_id: MESSAGE.into(), mention_count: Some(3), manual: false },
        AckRequest { channel_id: other_channel.into(), message_id: MESSAGE.into(), mention_count: None, manual: true },
    ];
    assert_eq!(client.ack_messages(&acks).await.unwrap(), Value::Null);
    let requests = fixture.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0].json(),
        json!({ "read_states": [
            { "channel_id": CHANNEL, "message_id": MESSAGE, "mention_count": 3 },
            { "channel_id": other_channel, "message_id": MESSAGE, "manual": true },
        ] })
    );
    assert_eq!(requests[1].target, "/api/v1/read-states/ack-bulk");
    assert_eq!(requests[1].json(), json!({ "read_states": [{ "channel_id": CHANNEL, "message_id": MESSAGE }] }));
    assert_eq!(requests[2].target, format!("/api/v1/channels/{other_channel}/messages/{MESSAGE}/ack"));
    assert_eq!(requests[2].json(), json!({ "manual": true, "mention_count": 0 }));

    assert!(matches!(client.ack_messages(&[]).await, Err(RestError::InvalidArgument(_))));
    let too_many = vec![acks[0].clone(); 101];
    assert!(matches!(client.ack_messages(&too_many).await, Err(RestError::InvalidArgument(_))));
    assert_eq!(fixture.requests().len(), 3);
}

// --------------------------------------------------------------- redirects

#[tokio::test]
async fn cross_origin_redirect_is_not_followed_and_token_does_not_leak() {
    let other = Fixture::start(|_, _| Resp::json(200, json!({ "id": "1" }))).await;
    let target = format!("{}/api/v1/users/@me", other.origin);
    let fixture = Fixture::start(move |_, _| Resp::empty(302).header("Location", &target)).await;
    let client = authed(&fixture).await;
    let error = client.get_me().await.unwrap_err();
    assert!(matches!(error, RestError::UnexpectedRedirect { status: 302 }), "{error:?}");
    assert_eq!(fixture.requests().len(), 1);
    assert!(other.requests().is_empty(), "the other origin must not be contacted");
}

#[tokio::test]
async fn same_origin_redirect_is_followed_with_token() {
    let fixture = Fixture::start(|req, _| {
        if req.target == "/api/v1/users/@me" {
            Resp::empty(307).header("Location", "/api/v1/users/@me/moved")
        } else {
            Resp::json(200, json!({ "id": "1" }))
        }
    })
    .await;
    let client = authed(&fixture).await;
    assert_eq!(client.get_me().await.unwrap(), json!({ "id": "1" }));
    let requests = fixture.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].header("authorization"), Some(FAKE_TOKEN));
}

// ----------------------------------------------------------------- secrets

#[test]
fn secrets_are_masked_in_debug_and_display() {
    let token = SecretToken::new(FAKE_TOKEN);
    let secret = Secret::new(FAKE_PASSWORD);
    for text in [format!("{token:?}"), format!("{token}"), format!("{secret:?}"), format!("{secret}")] {
        assert!(!text.contains(FAKE_TOKEN) && !text.contains(FAKE_PASSWORD), "{text}");
    }
    let redacted = redact_json(&json!({ "Token": FAKE_TOKEN, "nested": [{ "password": FAKE_PASSWORD, "id": "1" }] }));
    assert_eq!(redacted, json!({ "Token": "***", "nested": [{ "password": "***", "id": "1" }] }));
}

#[tokio::test]
async fn error_bodies_and_client_debug_do_not_leak_secrets() {
    let fixture = Fixture::start(|_, _| {
        Resp::json(
            401,
            json!({ "code": "UNAUTHORIZED", "message": "nope", "token": FAKE_TOKEN, "email": "user@example.test", "ticket": FAKE_TICKET }),
        )
    })
    .await;
    let client = authed(&fixture).await;
    let error = client.get_me().await.unwrap_err();
    assert_eq!(error.code(), Some("UNAUTHORIZED"));
    let RestError::Api(api) = &error else { panic!("{error:?}") };
    assert_eq!(api.body["token"], "***");
    for text in [format!("{error:?}"), error.to_string(), format!("{client:?}")] {
        for secret in [FAKE_TOKEN, FAKE_TICKET, "user@example.test"] {
            assert!(!text.contains(secret), "{text}");
        }
    }
}

// ------------------------------------------------------------------ altcha

const ALTCHA_SIGNATURE_SECRET: &str = "test-only-altcha-signature-secret";
const ALTCHA_KEY_SECRET: &str = "test-only-altcha-key-secret";

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

fn hmac_hex(secret: &str, data: &[u8]) -> String {
    use hmac::Mac;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(data);
    hex(&mac.finalize().into_bytes())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

/// `JSON.stringify(sortKeys(value))` (altcha-lib `canonicalJSON`).
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), canonical_json(&map[k])))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => format!("[{}]", items.iter().map(canonical_json).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

fn pbkdf2_key(algorithm: &str, password: &[u8], salt: &[u8], cost: u32, key_length: usize) -> Vec<u8> {
    let mut key = vec![0u8; key_length];
    match algorithm {
        "PBKDF2/SHA-256" => pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password, salt, cost, &mut key),
        "PBKDF2/SHA-384" => pbkdf2::pbkdf2_hmac::<sha2::Sha384>(password, salt, cost, &mut key),
        "PBKDF2/SHA-512" => pbkdf2::pbkdf2_hmac::<sha2::Sha512>(password, salt, cost, &mut key),
        other => panic!("{other}"),
    }
    key
}

fn altcha_password(nonce: &[u8], counter: u32) -> Vec<u8> {
    let mut password = nonce.to_vec();
    password.extend_from_slice(&counter.to_be_bytes());
    password
}

/// Mirror of altcha-lib 2.8.0 `createChallenge` in deterministic mode as
/// used by `AltchaProvider.createChallenge`: random nonce/salt, the key for
/// `counter`, `keyPrefix` = first `keyLength / 2` bytes, `keySignature` =
/// HMAC(derived key), `signature` = HMAC(canonical parameters).
fn make_altcha_challenge(algorithm: &str, cost: u32, counter: u32) -> Value {
    let nonce: [u8; 16] = rand::random();
    let salt: [u8; 16] = rand::random();
    let key_length = 32usize;
    let key = pbkdf2_key(algorithm, &altcha_password(&nonce, counter), &salt, cost, key_length);
    let parameters = json!({
        "algorithm": algorithm,
        "nonce": hex(&nonce),
        "salt": hex(&salt),
        "cost": cost,
        "keyLength": key_length,
        "keyPrefix": hex(&key[..key_length / 2]),
        "keySignature": hmac_hex(ALTCHA_KEY_SECRET, &key),
        "expiresAt": 4_000_000_000u64,
    });
    let signature = hmac_hex(ALTCHA_SIGNATURE_SECRET, canonical_json(&parameters).as_bytes());
    json!({ "parameters": parameters, "signature": signature })
}

/// Mirror of `AltchaProvider.verify` -> altcha-lib `verifySolution`
/// (signature check, key-signature path 4a and re-derivation path 4b).
fn verify_altcha_token(token: &str) -> Result<u32, String> {
    use base64::Engine;
    if token.len() > 4096 {
        return Err("token too long".into());
    }
    let raw = base64::engine::general_purpose::STANDARD.decode(token).map_err(|e| e.to_string())?;
    let payload: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let parameters = &payload["challenge"]["parameters"];
    let signature = payload["challenge"]["signature"].as_str().ok_or("no signature")?;
    if hmac_hex(ALTCHA_SIGNATURE_SECRET, canonical_json(parameters).as_bytes()) != signature {
        return Err("invalid signature".into());
    }
    let solution = &payload["solution"];
    let derived = solution["derivedKey"].as_str().ok_or("no derivedKey")?;
    let counter = solution["counter"].as_u64().and_then(|c| u32::try_from(c).ok()).ok_or("bad counter")?;
    if derived.len() % 2 != 0 || !derived.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("derivedKey is not lowercase hex".into());
    }
    if hmac_hex(ALTCHA_KEY_SECRET, &unhex(derived)) != parameters["keySignature"].as_str().unwrap_or_default() {
        return Err("key signature mismatch".into());
    }
    let algorithm = parameters["algorithm"].as_str().unwrap();
    let cost = u32::try_from(parameters["cost"].as_u64().unwrap()).unwrap();
    let key_length = usize::try_from(parameters["keyLength"].as_u64().unwrap()).unwrap();
    let nonce = unhex(parameters["nonce"].as_str().unwrap());
    let salt = unhex(parameters["salt"].as_str().unwrap());
    let key = hex(&pbkdf2_key(algorithm, &altcha_password(&nonce, counter), &salt, cost, key_length));
    if key != derived || !key.starts_with(parameters["keyPrefix"].as_str().unwrap()) {
        return Err("re-derived key mismatch".into());
    }
    Ok(counter)
}

#[test]
fn altcha_solver_produces_a_token_the_server_verifies() {
    for (algorithm, counter) in [("PBKDF2/SHA-256", 137u32), ("PBKDF2/SHA-512", 41)] {
        let challenge = make_altcha_challenge(algorithm, 20, counter);
        let token = solve_altcha_challenge(&challenge).unwrap();
        assert_eq!(verify_altcha_token(token.expose()), Ok(counter), "{algorithm}");
        assert!(!format!("{token:?}").contains(token.expose()));
    }
}

#[test]
fn altcha_solver_matches_odd_length_prefixes_as_hex() {
    // Unsigned, non-deterministic shape: a short odd prefix any key may hit.
    let challenge = json!({
        "parameters": {
            "algorithm": "PBKDF2/SHA-256", "nonce": "00112233", "salt": "44556677",
            "cost": 1, "keyLength": 32, "keyPrefix": "A",
        },
        "signature": "unsigned",
    });
    let token = solve_altcha_challenge(&challenge).unwrap();
    use base64::Engine;
    let payload: Value =
        serde_json::from_slice(&base64::engine::general_purpose::STANDARD.decode(token.expose()).unwrap()).unwrap();
    assert!(payload["solution"]["derivedKey"].as_str().unwrap().starts_with('a'));
    assert_eq!(payload["challenge"]["parameters"]["keyPrefix"], "A", "parameters are echoed verbatim");
    assert_eq!(payload["challenge"]["signature"], "unsigned");
}

#[test]
fn altcha_solver_is_bounded_and_rejects_bad_challenges() {
    let unreachable_counter = make_altcha_challenge("PBKDF2/SHA-256", 1, 5000);
    let started = std::time::Instant::now();
    let error = super::altcha::solve_bounded(&unreachable_counter, 100, Duration::from_secs(30)).unwrap_err();
    assert!(matches!(error, RestError::Captcha(_)), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(10));

    let started = std::time::Instant::now();
    let error = super::altcha::solve_bounded(&unreachable_counter, u32::MAX, Duration::ZERO).unwrap_err();
    assert!(matches!(error, RestError::Captcha(_)), "deadline: {error:?}");
    assert!(started.elapsed() < Duration::from_secs(5));

    let good = make_altcha_challenge("PBKDF2/SHA-256", 1, 1);
    for (key, value) in [
        ("algorithm", json!("SHA-256")),
        ("nonce", json!("xyz")),
        ("cost", json!(0)),
        ("cost", json!(1_000_000_000u64)),
        ("keyLength", json!(4096)),
        ("keyPrefix", json!("")),
        ("keyPrefix", json!("zz")),
    ] {
        let mut bad = good.clone();
        bad["parameters"][key] = value.clone();
        let error = solve_altcha_challenge(&bad).unwrap_err();
        assert!(matches!(error, RestError::Captcha(_)), "{key}={value}: {error:?}");
    }
    assert!(solve_altcha_challenge(&json!({ "parameters": {} })).is_err());
}

fn captcha_error_body(code: &str, challenge: Value) -> Value {
    json!({ "code": code, "message": "captcha", "captcha_provider": "altcha", "altcha_challenge": challenge })
}

#[tokio::test]
async fn login_solves_altcha_and_retries_transparently() {
    let fixture = Fixture::start(|req, _| match req.header("x-captcha-token") {
        None => Resp::json(400, captcha_error_body("CAPTCHA_REQUIRED", make_altcha_challenge("PBKDF2/SHA-256", 10, 77))),
        Some(token) => match verify_altcha_token(token) {
            Ok(_) => Resp::json(200, json!({ "token": FAKE_TOKEN, "user_id": "42", "user": {} })),
            Err(_) => Resp::json(400, captcha_error_body("INVALID_CAPTCHA", make_altcha_challenge("PBKDF2/SHA-256", 10, 77))),
        },
    })
    .await;
    let client = fixture.client();
    let outcome = client.login("user@example.test", &Secret::new(FAKE_PASSWORD)).await.unwrap();
    assert!(matches!(outcome, LoginOutcome::Token(_)), "{outcome:?}");
    let requests = fixture.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].json(), requests[1].json(), "the same body is resent");
    assert!(verify_altcha_token(requests[1].header("x-captcha-token").unwrap()).is_ok());
}

#[tokio::test]
async fn totp_also_gets_captcha_retries() {
    let fixture = Fixture::start(|req, _| match req.header("x-captcha-token") {
        None => Resp::json(400, captcha_error_body("CAPTCHA_REQUIRED", make_altcha_challenge("PBKDF2/SHA-256", 10, 5))),
        Some(_) => Resp::json(200, json!({ "token": FAKE_TOKEN, "user_id": "42", "user": {} })),
    })
    .await;
    let client = fixture.client();
    let outcome =
        client.login_mfa_totp(&Secret::new(FAKE_TICKET), &Secret::new("123456")).await.unwrap();
    assert!(matches!(outcome, LoginOutcome::Token(_)), "{outcome:?}");
    let requests = fixture.requests();
    assert_eq!(requests[1].target, "/api/v1/auth/login/mfa/totp");
    assert!(verify_altcha_token(requests[1].header("x-captcha-token").unwrap()).is_ok());
}

#[tokio::test]
async fn captcha_solves_stop_after_two_like_the_interceptor() {
    let fixture = Fixture::start(|_, _| {
        Resp::json(400, captcha_error_body("INVALID_CAPTCHA", make_altcha_challenge("PBKDF2/SHA-256", 1, 3)))
    })
    .await;
    let client = fixture.client();
    let outcome = client.login("user@example.test", &Secret::new(FAKE_PASSWORD)).await.unwrap();
    let LoginOutcome::CaptchaRequired { code, challenge, .. } = &outcome else { panic!("{outcome:?}") };
    assert_eq!(code, "INVALID_CAPTCHA");
    assert!(challenge.is_some());
    assert_eq!(fixture.requests().len(), 3, "first try plus two solved retries");
}

// ----------------------------------------------------------- body / limits

#[tokio::test]
async fn oversized_responses_are_cut_off_with_or_without_content_length() {
    for omit_length in [true, false] {
        let fixture = Fixture::start(move |_, _| {
            let mut resp = Resp::json(200, json!({}));
            resp.body = vec![b' '; MAX_RESPONSE_BYTES + 1024];
            resp.omit_length = omit_length;
            resp
        })
        .await;
        let client = authed(&fixture).await;
        let error = client.get_me().await.unwrap_err();
        assert!(
            matches!(error, RestError::ResponseTooLarge { limit } if limit == MAX_RESPONSE_BYTES),
            "omit_length={omit_length}: {error:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn in_flight_requests_are_capped() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let current = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (c, p) = (current.clone(), peak.clone());
    let fixture = Fixture::start(move |_, _| {
        let now = c.fetch_add(1, Ordering::SeqCst) + 1;
        p.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        c.fetch_sub(1, Ordering::SeqCst);
        Resp::json(200, json!({ "id": "1" }))
    })
    .await;
    let client = Arc::new(authed(&fixture).await);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let client = client.clone();
        tasks.spawn(async move { client.get_me().await });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap().unwrap();
    }
    let peak = peak.load(Ordering::SeqCst);
    assert!(peak <= MAX_IN_FLIGHT_REQUESTS, "peak {peak}");
    assert!(peak >= 2, "requests should still run in parallel, peak {peak}");
    assert_eq!(fixture.requests().len(), 20);
}

#[test]
fn rate_limit_retry_after_http_date() {
    let mut headers = HeaderMap::new();
    let future = std::time::SystemTime::now() + Duration::from_secs(120);
    headers.insert("retry-after", httpdate::fmt_http_date(future).parse().unwrap());
    let info = parse_rate_limit(&headers, &Value::Null);
    assert!(info.wait > Duration::from_secs(115) && info.wait <= Duration::from_secs(120), "{:?}", info.wait);

    let past = std::time::SystemTime::now() - Duration::from_secs(120);
    headers.insert("retry-after", httpdate::fmt_http_date(past).parse().unwrap());
    let info = parse_rate_limit(&headers, &json!({ "retry_after": 7.0 }));
    assert_eq!(info.wait, Duration::from_secs(7), "a past date falls through to the body");
}

#[test]
fn defaults_stay_under_the_server_bucket() {
    let config = test_config("https://chat.example/api");
    assert_eq!(config.max_requests_per_second, DEFAULT_REQUESTS_PER_SECOND);
    assert!(DEFAULT_REQUESTS_PER_SECOND < MAX_REQUESTS_PER_SECOND);
    assert!(MAX_REQUESTS_PER_SECOND + u32::try_from(MAX_IN_FLIGHT_REQUESTS).unwrap() <= SERVER_GLOBAL_LIMIT_PER_SECOND);
    let mut over = config;
    over.max_requests_per_second = MAX_REQUESTS_PER_SECOND + 1;
    assert!(matches!(RestClient::new(over), Err(RestError::InvalidArgument(_))));
}

// ------------------------------------------------------------------ logout

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logout_keeps_a_token_set_by_a_newer_login() {
    const NEW_TOKEN: &str = "flx_TESTONLYnewerxxxxxxxxxxxxxxxxxxxxxxx";
    let fixture = Fixture::start(|_, _| {
        std::thread::sleep(Duration::from_millis(400));
        Resp::empty(204)
    })
    .await;
    let client = Arc::new(authed(&fixture).await);
    let logout = tokio::spawn({
        let client = client.clone();
        async move { client.logout().await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    client.set_token(Some(SecretToken::new(NEW_TOKEN))).await;
    logout.await.unwrap().unwrap();
    let (token, _) = client.token_snapshot().await;
    assert_eq!(token.as_ref().map(SecretToken::expose), Some(NEW_TOKEN));
    assert_eq!(fixture.requests()[0].header("authorization"), Some(FAKE_TOKEN), "the old token was logged out");
}

#[tokio::test]
async fn logout_does_not_retry_429_and_still_clears() {
    let fixture = Fixture::start(|_, _| {
        Resp::json(429, json!({ "code": "RATE_LIMITED", "message": "", "retry_after": 0.1, "global": false }))
            .header("X-RateLimit-Reset-After", "0.1")
    })
    .await;
    let client = authed(&fixture).await;
    let error = client.logout().await.unwrap_err();
    assert!(matches!(error, RestError::RateLimited { .. }), "{error:?}");
    assert_eq!(fixture.requests().len(), 1);
    assert!(!client.has_token().await);
    assert!(matches!(client.logout().await, Err(RestError::NotAuthenticated)));
}

// ---------------------------------------------------------- url redaction

#[test]
fn userinfo_never_reaches_error_messages() {
    for input in ["https://alice:hunter2@chat.example/api", "alice:hunter2@chat.example", "ftp://alice:hunter2@x/y"] {
        let error = resolve_instance_input(input, ReleaseChannel::Stable).unwrap_err();
        let text = error.to_string();
        assert!(matches!(error, DiscoveryError::InvalidInput(_)), "{input}: {error:?}");
        assert!(!text.contains("hunter2") && !text.contains("alice"), "{text}");
        assert!(text.contains("***@"), "{text}");
    }
    let mut config = test_config("https://alice:hunter2@chat.example/api");
    config.policy = TransportPolicy::default();
    let error = RestClient::new(config).unwrap_err();
    assert!(matches!(error, RestError::InvalidUrl(_)), "{error:?}");
    assert!(!error.to_string().contains("hunter2"), "{error}");
}

#[test]
fn discovery_applies_transport_policy_to_invite_and_webapp() {
    let served = "https://chat.example/.well-known/fluxer";
    let policy = TransportPolicy::default();
    let doc = |key: &str, value: &str| {
        let mut doc = json!({
            "api_code_version": 1,
            "endpoints": { "api": "https://chat.example/api", "gateway": "wss://gateway.chat.example" }
        });
        doc["endpoints"][key] = json!(value);
        doc
    };
    for key in ["invite", "webapp"] {
        let error = parse_document_for_test(served, doc(key, "http://chat.example"), &policy).unwrap_err();
        assert!(error.contains("insecure"), "{key}: {error}");
        assert!(parse_document_for_test(served, doc(key, "https://chat.example"), &policy).is_ok(), "{key}");
    }
}

/// Message actions, reactions, typing, attachments, users/DMs/members.
mod actions;
mod social;
