//! End to end against the fixture using the real client core: discovery,
//! RestClient (login/MFA/captcha, messages, ack, logout) and the gateway task.

use std::time::Duration;

use fluxer_client_core::Dispatch;
use fluxer_client_core::gateway::{self, Compression, GatewayConfig, GatewayEvent, IdentifyProperties};
use fluxer_client_core::rest::{
    AckRequest, ClientIdentity, DiscoveryOptions, Endpoints, LoginOutcome, MessageQuery, RestClient, RestConfig,
    RestError, Secret, TransportPolicy, discover_with,
};
use fluxer_client_core::state::{ClientState, Snowflake};
use fluxer_devserver::{Config, DEV_TOKEN, EMAIL_CAPTCHA, EMAIL_MFA, EMAIL_USER, MFA_CODE, PASSWORD, start};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const WAIT: Duration = Duration::from_secs(10);
const INSECURE: TransportPolicy = TransportPolicy { allow_insecure_loopback: true };

async fn discover(instance: &str) -> Endpoints {
    let options = DiscoveryOptions { policy: INSECURE, ..DiscoveryOptions::default() };
    discover_with(instance, &options).await.expect("discovery")
}

fn rest_client(endpoints: &Endpoints) -> RestClient {
    let mut config = RestConfig::new(endpoints, ClientIdentity::default());
    config.policy = INSECURE;
    RestClient::new(config).expect("rest client")
}

fn token_of(outcome: LoginOutcome) -> String {
    match outcome {
        LoginOutcome::Token(success) => success.token.expose().to_owned(),
        other => panic!("expected a token, got {other:?}"),
    }
}

fn spawn_gateway(endpoints: &Endpoints, token: &str, compress: Compression) -> (gateway::GatewayHandle, mpsc::Receiver<GatewayEvent>) {
    let config = GatewayConfig {
        url: url::Url::parse(&endpoints.gateway).expect("gateway url"),
        token: token.to_owned(),
        properties: IdentifyProperties {
            os: "Windows".into(),
            browser: "Fluxer Rust".into(),
            device: "desktop".into(),
            extra: serde_json::Map::new(),
        },
        compress,
        flags: 0,
        initial_guild_id: None,
        presence: None,
    };
    gateway::spawn(config).expect("gateway spawn")
}

/// Next dispatch with event name `t`.
async fn next_dispatch(events: &mut mpsc::Receiver<GatewayEvent>, t: &str) -> Dispatch {
    tokio::time::timeout(WAIT, async {
        loop {
            match events.recv().await {
                Some(GatewayEvent::Dispatch(dispatch)) if dispatch.t == t => return dispatch,
                Some(GatewayEvent::Closed { code, reason }) => panic!("gateway closed {code} {reason}"),
                Some(_) => {}
                None => panic!("gateway task ended"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {t}"))
}

fn channel_named(state: &ClientState, name: &str) -> Snowflake {
    state
        .channels
        .values()
        .find(|c| c.name.as_deref() == Some(name))
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("no channel {name}"))
}

fn ids(list: &Value) -> Vec<u64> {
    list.as_array()
        .expect("message array")
        .iter()
        .map(|m| m["id"].as_str().expect("id").parse().expect("snowflake"))
        .collect()
}

#[tokio::test]
async fn client_core_end_to_end() {
    let server = start(Config::default()).await.expect("start");
    let instance = server.instance_url();

    // Discovery.
    let endpoints = discover(&instance).await;
    assert_eq!(endpoints.api, format!("{instance}/api"));
    assert_eq!(endpoints.gateway, format!("ws://{}/gateway", server.addr));
    assert_eq!(endpoints.api_code_version, 1);
    let rest = rest_client(&endpoints);
    let password = Secret::new(PASSWORD);

    // Login failures and branches.
    match rest.login(EMAIL_USER, &Secret::new("wrong")).await {
        Err(RestError::Api(api)) => {
            assert_eq!((api.status, api.code.as_str()), (400, "INVALID_FORM_BODY"));
            assert_eq!(api.errors.len(), 2);
        }
        other => panic!("expected INVALID_FORM_BODY, got {other:?}"),
    }
    match rest.login(EMAIL_CAPTCHA, &password).await.expect("captcha login") {
        LoginOutcome::CaptchaRequired { code, provider, challenge } => {
            assert_eq!(code, "CAPTCHA_REQUIRED");
            assert_eq!(provider.as_deref(), Some("altcha"));
            assert!(challenge.is_some_and(|c| c.get("parameters").is_some() && c.get("signature").is_some()));
        }
        other => panic!("expected captcha, got {other:?}"),
    }
    token_of(rest.login_with_captcha(EMAIL_CAPTCHA, &password, &Secret::new("solved")).await.expect("captcha retry"));
    let ticket = match rest.login(EMAIL_MFA, &password).await.expect("mfa login") {
        LoginOutcome::Mfa { ticket, methods } => {
            assert!(methods.totp && !methods.webauthn);
            assert_eq!(methods.allowed, vec!["totp".to_owned()]);
            ticket
        }
        other => panic!("expected MFA, got {other:?}"),
    };
    assert!(matches!(
        rest.login_mfa_totp(&ticket, &Secret::new("000000")).await,
        Err(RestError::Api(api)) if api.status == 400
    ));
    token_of(rest.login_mfa_totp(&ticket, &Secret::new(MFA_CODE)).await.expect("totp"));

    // Authorization is required.
    rest.set_token(Some(fluxer_client_core::rest::SecretToken::new("flx_notavalidtoken"))).await;
    assert!(matches!(rest.get_me().await, Err(RestError::Api(api)) if api.status == 401));

    let token = token_of(rest.login(EMAIL_USER, &password).await.expect("login"));
    assert!(token.starts_with("flx_") && token.len() == 40);
    rest.set_token(Some(fluxer_client_core::rest::SecretToken::new(token.clone()))).await;
    let me = rest.get_me().await.expect("users/@me");
    assert_eq!(me["username"], "devuser");

    // Gateway (zstd-stream, as the app uses) -> READY into the real state.
    let (handle, mut events) = spawn_gateway(&endpoints, &token, Compression::ZstdStream);
    let ready = next_dispatch(&mut events, "READY").await;
    let mut state = ClientState::new();
    state.apply(&ready);
    assert_eq!(state.current_user.as_ref().map(|u| u.username.as_str()), Some("devuser"));
    assert_eq!(state.sorted_guilds().len(), 2);
    assert_eq!(state.dm_channels().len(), 2);
    let general = channel_named(&state, "general");
    let announcements = channel_named(&state, "announcements");
    let offtopic = channel_named(&state, "off-topic");
    let random = channel_named(&state, "random");
    assert!(state.can_send_messages(general));
    assert!(!state.can_send_messages(announcements), "@everyone is denied SEND_MESSAGES");
    assert!(state.channel_unread(general).unread);
    assert!(!state.channel_unread(random).unread);
    assert_eq!(state.channel_unread(offtopic).mention_count, 2);
    let guild_a = state.sorted_guilds()[0].id;
    let tree = state.channel_tree(guild_a);
    assert!(tree.iter().any(|node| node.channel.kind.is_category() && !node.children.is_empty()));
    let dm_mentions: u32 = state.dm_channels().iter().map(|c| state.channel_unread(c.id).mention_count).sum();
    assert_eq!(dm_mentions, 1);

    // History paging: 400 messages, newest first.
    let channel = general.to_string();
    let first = rest.get_messages(&channel, &MessageQuery::default()).await.expect("messages");
    assert_eq!(ids(&first).len(), 50);
    let mut all = Vec::new();
    let mut before = None;
    loop {
        let query = MessageQuery { before: before.clone(), limit: Some(100), ..MessageQuery::default() };
        let page = ids(&rest.get_messages(&channel, &query).await.expect("page"));
        if page.is_empty() {
            break;
        }
        assert!(page.windows(2).all(|w| w[0] > w[1]), "newest first");
        before = page.last().map(u64::to_string);
        all.extend(page);
    }
    assert_eq!(all.len(), 400);
    assert!(all.windows(2).all(|w| w[0] > w[1]));
    let pivot = all[200];
    let after = ids(&rest
        .get_messages(&channel, &MessageQuery { after: Some(pivot.to_string()), limit: Some(10), ..MessageQuery::default() })
        .await
        .expect("after"));
    assert_eq!(after.len(), 10);
    assert!(after.iter().all(|id| *id > pivot));
    assert_eq!(after.last(), Some(&all[199]), "after returns the oldest ones above the cursor");
    let around = ids(&rest
        .get_messages(&channel, &MessageQuery { around: Some(pivot.to_string()), limit: Some(10), ..MessageQuery::default() })
        .await
        .expect("around"));
    assert_eq!(around.len(), 10);
    assert!(around.contains(&pivot));
    let korean = rest
        .get_messages(&channel_named(&state, "한국어").to_string(), &MessageQuery { limit: Some(5), ..MessageQuery::default() })
        .await
        .expect("korean");
    let hangul = |m: &Value| m["content"].as_str().is_some_and(|c| c.chars().any(|ch| ('가'..='힣').contains(&ch)));
    assert!(korean.as_array().is_some_and(|list| list.len() == 5 && list.iter().filter(|m| hangul(m)).count() >= 4));

    // Send: REST echo with server id + gateway MESSAGE_CREATE with the nonce.
    let nonce = Snowflake::from_unix_ms(1_800_000_000_000).to_string();
    let sent = rest.send_message(&channel, "hello from the e2e test **bold** 한글", &nonce).await.expect("send");
    assert_eq!(sent["nonce"], nonce);
    let created = next_dispatch(&mut events, "MESSAGE_CREATE").await;
    assert_eq!(created.d["nonce"], nonce);
    assert_eq!(created.d["id"], sent["id"]);
    assert_eq!(created.d["guild_id"], guild_a.to_string());
    state.apply(&created);
    assert_eq!(state.channels[&general].last_message_id.map(|id| id.to_string()), sent["id"].as_str().map(str::to_owned));
    assert!(!state.channel_unread(general).unread, "own message acks the channel");
    let newest = rest.get_messages(&channel, &MessageQuery { limit: Some(1), ..MessageQuery::default() }).await.expect("newest");
    assert_eq!(newest[0]["id"], sent["id"]);

    match rest.send_message(&announcements.to_string(), "nope", "1").await {
        Err(RestError::Api(api)) => assert_eq!((api.status, api.code.as_str()), (403, "MISSING_PERMISSIONS")),
        other => panic!("expected 403, got {other:?}"),
    }

    // Ack.
    let message_id = all[0].to_string();
    let acked = rest
        .ack_message(&AckRequest { channel_id: offtopic.to_string(), message_id: message_id.clone(), mention_count: None, manual: false })
        .await
        .expect("ack");
    assert_eq!(acked["read_states"][0]["id"], offtopic.to_string());
    let ack_event = next_dispatch(&mut events, "MESSAGE_ACK").await;
    assert_eq!(ack_event.d["channel_id"], offtopic.to_string());

    // Logout revokes the token.
    rest.logout().await.expect("logout");
    rest.set_token(Some(fluxer_client_core::rest::SecretToken::new(token))).await;
    assert!(matches!(rest.get_me().await, Err(RestError::Api(api)) if api.status == 401));
    handle.close();
}

#[tokio::test]
async fn plain_gateway_and_chatter() {
    let server = start(Config { port: 0, chatter: Some(Duration::from_millis(200)) }).await.expect("start");
    let endpoints = discover(&server.instance_url()).await;
    let (handle, mut events) = spawn_gateway(&endpoints, DEV_TOKEN, Compression::None);
    let ready = next_dispatch(&mut events, "READY").await;
    let me = ready.d["user"]["id"].clone();
    let chatter = next_dispatch(&mut events, "MESSAGE_CREATE").await;
    assert_ne!(chatter.d["author"]["id"], me);
    assert!(chatter.s > ready.s);
    handle.close();
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Next text frame as JSON, or `{"close": code}`.
async fn recv(ws: &mut Ws) -> Value {
    loop {
        match tokio::time::timeout(WAIT, ws.next()).await.expect("timeout").expect("open").expect("frame") {
            Message::Text(text) => return serde_json::from_str(text.as_str()).expect("json"),
            Message::Close(frame) => return json!({ "close": frame.map(|f| u16::from(f.code)) }),
            _ => {}
        }
    }
}

/// Raw protocol checks the client task does not expose: heartbeat ACK,
/// op 14 tolerance, RESUME -> RESUMED, bad token -> 4004.
#[tokio::test]
async fn raw_gateway_protocol() {
    let server = start(Config::default()).await.expect("start");
    let url = format!("ws://{}/gateway?v=1&encoding=json", server.addr);

    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.expect("connect");
    let hello = recv(&mut ws).await;
    assert_eq!(hello["op"], 10);
    assert_eq!(hello["d"]["heartbeat_interval"], 41_250);
    let identify = json!({ "op": 2, "d": { "token": DEV_TOKEN, "properties": { "os": "x", "browser": "x", "device": "x" }, "flags": 0 } });
    ws.send(Message::text(identify.to_string())).await.expect("identify");
    let ready = recv(&mut ws).await;
    assert_eq!((ready["t"].as_str(), ready["s"].as_u64()), (Some("READY"), Some(1)));
    let session_id = ready["d"]["session_id"].as_str().expect("session id").to_owned();
    ws.send(Message::text(json!({ "op": 14, "d": { "subscriptions": {} } }).to_string())).await.expect("lazy");
    ws.send(Message::text(json!({ "op": 1, "d": 1 }).to_string())).await.expect("heartbeat");
    assert_eq!(recv(&mut ws).await["op"], 11);
    drop(ws);

    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.expect("reconnect");
    assert_eq!(recv(&mut ws).await["op"], 10);
    let resume = json!({ "op": 6, "d": { "token": DEV_TOKEN, "session_id": session_id, "seq": 1 } });
    ws.send(Message::text(resume.to_string())).await.expect("resume");
    let resumed = recv(&mut ws).await;
    assert_eq!((resumed["t"].as_str(), resumed["s"].as_u64()), (Some("RESUMED"), Some(2)));

    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.expect("connect bad");
    recv(&mut ws).await;
    let bad = json!({ "op": 2, "d": { "token": "flx_bad", "properties": {}, "flags": 0 } });
    ws.send(Message::text(bad.to_string())).await.expect("identify bad");
    assert_eq!(recv(&mut ws).await["close"], 4004);
}
