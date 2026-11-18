//! Fixture tests for message actions, reactions, typing, attachment
//! uploads and user/DM/member reads. Every token is fake test data.

use super::*;

const OTHER_CHANNEL: &str = "323456789012345678";
const USER: &str = "423456789012345678";
const GUILD: &str = "523456789012345678";
const EMOJI_ID: &str = "623456789012345678";

fn line(req: &Req) -> (String, String) {
    (req.method.clone(), req.target.clone())
}

fn expect(method: &str, target: impl Into<String>) -> (String, String) {
    (method.to_owned(), target.into())
}

fn rate_limited_60s() -> Resp {
    Resp::json(429, json!({ "code": "RATE_LIMITED", "message": "", "retry_after": 60.0, "global": false }))
        .header("X-RateLimit-Reset-After", "60")
}

// --------------------------------------------------------- message actions

#[tokio::test]
async fn message_actions_use_server_routes() {
    let fixture = Fixture::start(|req, _| match req.method.as_str() {
        "DELETE" | "PUT" => Resp::empty(204),
        _ => Resp::json(200, json!({ "id": MESSAGE })),
    })
    .await;
    let client = authed(&fixture).await;
    client.get_message(CHANNEL, MESSAGE).await.unwrap();
    let edit = EditMessageRequest { content: Some("edited".into()), ..EditMessageRequest::default() };
    client.edit_message(CHANNEL, MESSAGE, &edit).await.unwrap();
    client.delete_message(CHANNEL, MESSAGE).await.unwrap();
    let pins = PinsQuery { limit: Some(25), before: Some("2026-01-02T03:04:05.000Z".into()) };
    client.get_pins(CHANNEL, &pins).await.unwrap();
    client.pin_message(CHANNEL, MESSAGE).await.unwrap();
    client.unpin_message(CHANNEL, MESSAGE).await.unwrap();
    let reply = SendMessageRequest {
        content: Some("re".into()),
        nonce: "1300000000000000001".into(),
        message_reference: Some(MessageReference { message_id: MESSAGE.into(), ..MessageReference::default() }),
        ..SendMessageRequest::default()
    };
    client.send_message_request(CHANNEL, &reply).await.unwrap();

    let requests = fixture.requests();
    let lines: Vec<_> = requests.iter().map(line).collect();
    assert_eq!(
        lines,
        vec![
            expect("GET", format!("/api/v1/channels/{CHANNEL}/messages/{MESSAGE}")),
            expect("PATCH", format!("/api/v1/channels/{CHANNEL}/messages/{MESSAGE}")),
            expect("DELETE", format!("/api/v1/channels/{CHANNEL}/messages/{MESSAGE}")),
            expect(
                "GET",
                format!("/api/v1/channels/{CHANNEL}/messages/pins?limit=25&before=2026-01-02T03%3A04%3A05.000Z")
            ),
            expect("PUT", format!("/api/v1/channels/{CHANNEL}/pins/{MESSAGE}")),
            expect("DELETE", format!("/api/v1/channels/{CHANNEL}/pins/{MESSAGE}")),
            expect("POST", format!("/api/v1/channels/{CHANNEL}/messages")),
        ]
    );
    for req in &requests {
        assert_eq!(req.header("authorization"), Some(FAKE_TOKEN));
    }
    assert_eq!(requests[1].json(), json!({ "content": "edited" }));
    assert_eq!(requests[1].header("content-type"), Some("application/json"));
    assert!(requests[2].body.is_empty() && requests[4].body.is_empty());
    assert_eq!(
        requests[6].json(),
        json!({ "content": "re", "nonce": "1300000000000000001", "message_reference": { "message_id": MESSAGE } })
    );

    // Bad input never reaches the server.
    assert!(matches!(client.get_message(CHANNEL, "1/../2").await, Err(RestError::InvalidArgument(_))));
    assert!(matches!(client.delete_message("x", MESSAGE).await, Err(RestError::InvalidArgument(_))));
    let bad_pins = PinsQuery { limit: Some(51), before: None };
    assert!(matches!(client.get_pins(CHANNEL, &bad_pins).await, Err(RestError::InvalidArgument(_))));
    let bad_reply = SendMessageRequest {
        nonce: "1".into(),
        message_reference: Some(MessageReference { message_id: "nope".into(), ..MessageReference::default() }),
        ..SendMessageRequest::default()
    };
    assert!(matches!(client.send_message_request(CHANNEL, &bad_reply).await, Err(RestError::InvalidArgument(_))));
    assert_eq!(fixture.requests().len(), 7);
}

// --------------------------------------------------------------- reactions

#[test]
fn encode_uri_component_matches_javascript() {
    // Expected values are JavaScript `encodeURIComponent` outputs.
    assert_eq!(encode_uri_component("AZaz09-_.!~*'()"), "AZaz09-_.!~*'()");
    assert_eq!(encode_uri_component("a b/c?d#e&f=g:h@i%"), "a%20b%2Fc%3Fd%23e%26f%3Dg%3Ah%40i%25");
    assert_eq!(encode_uri_component("\u{1F44D}"), "%F0%9F%91%8D");
    assert_eq!(encode_uri_component("\u{2764}\u{FE0F}"), "%E2%9D%A4%EF%B8%8F");
    assert_eq!(encode_uri_component("\u{e9}"), "%C3%A9");
}

#[tokio::test]
async fn reactions_encode_unicode_and_custom_emoji() {
    let fixture = Fixture::start(|req, _| {
        if req.method == "GET" {
            Resp::json(200, json!({ "items": [], "has_more": false }))
        } else {
            Resp::empty(204)
        }
    })
    .await;
    let client = authed(&fixture).await;
    let thumbs = ReactionEmoji::unicode("\u{1F44D}");
    let custom = ReactionEmoji::custom("blob_wave", EMOJI_ID);
    client.add_reaction(CHANNEL, MESSAGE, &thumbs, Some("sess-1")).await.unwrap();
    client.remove_own_reaction(CHANNEL, MESSAGE, &thumbs, None).await.unwrap();
    client.add_reaction(CHANNEL, MESSAGE, &custom, None).await.unwrap();
    let query = ReactionUsersQuery { limit: Some(25), after: Some(USER.into()) };
    let page = client.get_reaction_users(CHANNEL, MESSAGE, &custom, &query).await.unwrap();
    assert_eq!(page["has_more"], false);

    let base = format!("/api/v1/channels/{CHANNEL}/messages/{MESSAGE}/reactions");
    let lines: Vec<_> = fixture.requests().iter().map(line).collect();
    assert_eq!(
        lines,
        vec![
            expect("PUT", format!("{base}/%F0%9F%91%8D/@me?session_id=sess-1")),
            expect("DELETE", format!("{base}/%F0%9F%91%8D/@me")),
            expect("PUT", format!("{base}/blob_wave%3A{EMOJI_ID}/@me")),
            expect("GET", format!("{base}/blob_wave%3A{EMOJI_ID}/users?limit=25&after={USER}")),
        ]
    );

    for bad in [
        ReactionEmoji::unicode(""),
        ReactionEmoji::unicode(".."),
        ReactionEmoji::unicode("a\u{0}"),
        ReactionEmoji::custom("blob", "12ab"),
        ReactionEmoji::unicode("x".repeat(129)),
    ] {
        let result = client.add_reaction(CHANNEL, MESSAGE, &bad, None).await;
        assert!(matches!(result, Err(RestError::InvalidArgument(_))), "{bad:?}");
    }
    let long_session = "s".repeat(65);
    assert!(matches!(
        client.add_reaction(CHANNEL, MESSAGE, &thumbs, Some(&long_session)).await,
        Err(RestError::InvalidArgument(_))
    ));
    let bad_limit = ReactionUsersQuery { limit: Some(101), after: None };
    assert!(matches!(
        client.get_reaction_users(CHANNEL, MESSAGE, &thumbs, &bad_limit).await,
        Err(RestError::InvalidArgument(_))
    ));
    assert_eq!(fixture.requests().len(), 4);
}

// ------------------------------------------------------------------ typing

#[tokio::test]
async fn typing_returns_cooldowns_only_on_200() {
    let fixture = Fixture::start(|req, _| {
        if req.target.contains(OTHER_CHANNEL) {
            Resp::json(200, json!({ "message_send_cooldown_ms": 4200 }))
        } else {
            Resp::empty(204)
        }
    })
    .await;
    let client = authed(&fixture).await;
    assert_eq!(client.trigger_typing(CHANNEL).await.unwrap(), None);
    let cooldown = client.trigger_typing(OTHER_CHANNEL).await.unwrap();
    assert_eq!(
        cooldown,
        Some(TypingCooldown { message_send_cooldown_ms: Some(4200), thread_create_cooldown_ms: None })
    );
    let req = &fixture.requests()[0];
    assert_eq!(line(req), expect("POST", format!("/api/v1/channels/{CHANNEL}/typing")));
    assert!(req.body.is_empty());
}

#[tokio::test]
async fn rate_limit_buckets_are_per_method_and_path() {
    let fixture = Fixture::start(|req, _| {
        if req.target.contains(CHANNEL) { rate_limited_60s() } else { Resp::empty(204) }
    })
    .await;
    let client = authed(&fixture).await;
    assert!(matches!(client.trigger_typing(CHANNEL).await, Err(RestError::RateLimited { global: false, .. })));
    // Same "POST /channels/{c}/typing" bucket: refused locally.
    assert!(matches!(client.trigger_typing(CHANNEL).await, Err(RestError::RateLimited { .. })));
    // Another channel (path) is another bucket, and so is a reaction on it.
    assert_eq!(client.trigger_typing(OTHER_CHANNEL).await.unwrap(), None);
    let thumbs = ReactionEmoji::unicode("\u{1F44D}");
    client.add_reaction(OTHER_CHANNEL, MESSAGE, &thumbs, None).await.unwrap();
    assert_eq!(fixture.requests().len(), 3);
}

#[tokio::test]
async fn reaction_buckets_include_the_encoded_emoji() {
    let fixture = Fixture::start(|req, _| {
        if req.target.contains("%F0%9F%91%8D") { rate_limited_60s() } else { Resp::empty(204) }
    })
    .await;
    let client = authed(&fixture).await;
    let thumbs = ReactionEmoji::unicode("\u{1F44D}");
    let heart = ReactionEmoji::unicode("\u{2764}");
    assert!(client.add_reaction(CHANNEL, MESSAGE, &thumbs, None).await.is_err());
    assert!(matches!(client.add_reaction(CHANNEL, MESSAGE, &thumbs, None).await, Err(RestError::RateLimited { .. })));
    client.add_reaction(CHANNEL, MESSAGE, &heart, None).await.unwrap();
    // DELETE of the same path is a different bucket ("METHOD path").
    assert!(client.remove_own_reaction(CHANNEL, MESSAGE, &thumbs, None).await.is_err());
    assert_eq!(fixture.requests().len(), 3);
}

#[test]
fn typing_throttle_matches_plan_typing_send() {
    use std::time::Instant;
    let t0 = Instant::now();
    let ms = |n: u64| t0 + Duration::from_millis(n);
    let mut throttle = TypingThrottle::new();
    assert_eq!(throttle.start(CHANNEL, t0), TypingPlan::SendAfter(TYPING_SEND_DELAY));
    // Pending send: further keystrokes are throttled.
    assert_eq!(throttle.start(CHANNEL, ms(500)), TypingPlan::Throttled);
    assert!(throttle.take_due(CHANNEL));
    assert!(!throttle.take_due(CHANNEL));
    // Within 8 s of the last scheduled send.
    assert_eq!(throttle.start(CHANNEL, ms(7_999)), TypingPlan::Throttled);
    assert_eq!(throttle.start(CHANNEL, ms(8_000)), TypingPlan::SendAfter(TYPING_SEND_DELAY));
    assert!(throttle.take_due(CHANNEL));
    // Idle for 16 s since the last send: send immediately.
    assert_eq!(throttle.start(CHANNEL, ms(24_000)), TypingPlan::SendAfter(Duration::ZERO));
    // Switching channels drops the pending send of the old one.
    assert_eq!(throttle.start(OTHER_CHANNEL, ms(24_100)), TypingPlan::SendAfter(TYPING_SEND_DELAY));
    assert!(!throttle.take_due(CHANNEL));
    // Clearing the composer cancels a pending send.
    throttle.stop(OTHER_CHANNEL);
    assert!(!throttle.take_due(OTHER_CHANNEL));
    assert_eq!(throttle.start(OTHER_CHANNEL, ms(24_200)), TypingPlan::SendAfter(TYPING_SEND_DELAY));
    throttle.message_sent();
    assert!(!throttle.take_due(OTHER_CHANNEL));
    assert_eq!(throttle.start(OTHER_CHANNEL, ms(24_300)), TypingPlan::SendAfter(TYPING_SEND_DELAY));
}

// ------------------------------------------------------------- attachments

struct TempFile(std::path::PathBuf);

impl TempFile {
    fn new(contents: &[u8]) -> Self {
        let name = format!("fluxer-rest-upload-{}.bin", new_installation_id());
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, contents).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn upload_file(source: UploadSource, filename: &str) -> UploadFile {
    UploadFile { source, filename: filename.to_owned(), content_type: None, description: None, flags: 0 }
}

fn memory(bytes: &'static [u8]) -> UploadSource {
    UploadSource::Bytes(bytes::Bytes::from_static(bytes))
}

#[tokio::test]
async fn attachments_are_sent_as_multipart_payload_json_and_files() {
    let fixture = Fixture::start(|req, index| {
        if index == 0 {
            Resp::json(429, json!({ "code": "RATE_LIMITED", "message": "", "retry_after": 0.1, "global": false }))
                .header("X-RateLimit-Reset-After", "0.1")
        } else {
            Resp::json(200, json!({ "id": MESSAGE, "bytes": req.body.len() }))
        }
    })
    .await;
    let client = authed(&fixture).await;
    let disk_contents: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let temp = TempFile::new(&disk_contents);
    let files = vec![
        UploadFile {
            content_type: Some("image/png".into()),
            description: Some("alt text".into()),
            flags: 8,
            ..upload_file(UploadSource::Path(temp.0.clone()), "photo.png")
        },
        upload_file(memory(b"hello attachment"), "notes \u{d55c}.txt"),
    ];
    let progress: Arc<Mutex<Vec<(u64, u64)>>> = Arc::default();
    let seen = progress.clone();
    let options = UploadOptions {
        max_file_size: 1024 * 1024,
        progress: Some(Arc::new(move |sent, total| seen.lock().unwrap().push((sent, total)))),
    };
    let message = SendMessageRequest {
        content: Some("look".into()),
        nonce: "1300000000000000002".into(),
        ..SendMessageRequest::default()
    };
    let sent = client.send_message_with_files(CHANNEL, &message, &files, &options).await.unwrap();
    assert_eq!(sent["id"], MESSAGE);

    let requests = fixture.requests();
    assert_eq!(requests.len(), 2, "a 429 retry must resend the whole form");
    for req in &requests {
        assert_eq!(line(req), expect("POST", format!("/api/v1/channels/{CHANNEL}/messages")));
        assert_eq!(req.header("authorization"), Some(FAKE_TOKEN));
        let content_type = req.header("content-type").unwrap();
        let boundary = content_type.strip_prefix("multipart/form-data; boundary=").unwrap();
        assert_eq!(req.header("content-length").unwrap().parse::<usize>().unwrap(), req.body.len());
        let body = &req.body;
        let text = String::from_utf8_lossy(body);
        assert!(text.starts_with(&format!("--{boundary}\r\n")));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
        let payload_head = "Content-Disposition: form-data; name=\"payload_json\"\r\n\r\n";
        assert!(text.contains(payload_head));
        assert!(text.contains(
            "Content-Disposition: form-data; name=\"files[0]\"; filename=\"photo.png\"\r\nContent-Type: image/png\r\n\r\n"
        ));
        assert!(text.contains(
            "Content-Disposition: form-data; name=\"files[1]\"; filename=\"notes \u{d55c}.txt\"\r\nContent-Type: application/octet-stream\r\n\r\nhello attachment\r\n"
        ));
        // The disk file arrives byte-exact.
        let marker = b"Content-Type: image/png\r\n\r\n";
        let start = body.windows(marker.len()).position(|w| w == marker).unwrap() + marker.len();
        let end = start + disk_contents.len();
        assert_eq!(&body[start..end], disk_contents.as_slice());
        assert_eq!(&body[end..end + 2], b"\r\n");
        // payload_json carries the message and per-file metadata keyed by index.
        let payload_start = text.find(payload_head).unwrap() + payload_head.len();
        let payload_end = payload_start + text[payload_start..].find("\r\n--").unwrap();
        let payload: Value = serde_json::from_str(&text[payload_start..payload_end]).unwrap();
        assert_eq!(
            payload,
            json!({
                "content": "look",
                "nonce": "1300000000000000002",
                "attachments": [
                    { "id": 0, "filename": "photo.png", "content_type": "image/png", "description": "alt text", "flags": 8 },
                    { "id": 1, "filename": "notes \u{d55c}.txt" },
                ],
            })
        );
    }
    let total = (disk_contents.len() + b"hello attachment".len()) as u64;
    let progress = progress.lock().unwrap();
    assert_eq!(progress.last(), Some(&(total, total)));
    assert!(progress.iter().all(|(sent, t)| *t == total && *sent <= total));
}

#[tokio::test]
async fn attachment_limits_are_checked_before_sending() {
    let fixture = Fixture::start(|_, _| Resp::json(200, json!({}))).await;
    let client = authed(&fixture).await;
    let message = SendMessageRequest { nonce: "1".into(), ..SendMessageRequest::default() };
    let big = TempFile::new(&[7u8; 2048]);
    let options = UploadOptions { max_file_size: 1024, progress: None };
    let files = vec![upload_file(memory(b"ok"), "a.txt"), upload_file(UploadSource::Path(big.0.clone()), "big.bin")];
    let error = client.send_message_with_files(CHANNEL, &message, &files, &options).await.unwrap_err();
    assert!(matches!(error, RestError::AttachmentTooLarge { index: 1, size: 2048, limit: 1024 }), "{error:?}");

    let missing = vec![upload_file(UploadSource::Path(big.0.with_extension("missing")), "gone.bin")];
    let error = client.send_message_with_files(CHANNEL, &message, &missing, &options).await.unwrap_err();
    assert!(matches!(error, RestError::AttachmentIo(_)), "{error:?}");

    let defaults = UploadOptions::default();
    assert!(matches!(
        client.send_message_with_files(CHANNEL, &message, &[], &defaults).await,
        Err(RestError::InvalidArgument(_))
    ));
    let eleven: Vec<_> = (0..11).map(|_| upload_file(memory(b"x"), "x.txt")).collect();
    assert!(matches!(
        client.send_message_with_files(CHANNEL, &message, &eleven, &defaults).await,
        Err(RestError::InvalidArgument(_))
    ));
    for name in ["", "../etc/passwd", "a\"b", "line\nbreak"] {
        let file = upload_file(memory(b"x"), name);
        let result = client.send_message_with_files(CHANNEL, &message, &[file], &defaults).await;
        assert!(matches!(result, Err(RestError::InvalidArgument(_))), "{name:?}");
    }
    assert!(fixture.requests().is_empty());
}

// ------------------------------------------------------------ users / DMs

#[tokio::test]
async fn user_dm_and_member_reads() {
    let fixture = Fixture::start(|_, _| Resp::json(200, json!([]))).await;
    let client = authed(&fixture).await;
    client.get_user_profile(USER, &ProfileQuery::default()).await.unwrap();
    let scoped = ProfileQuery { guild_id: Some(GUILD.into()), with_mutual_friends: false, with_mutual_guilds: true };
    client.get_user_profile("@me", &scoped).await.unwrap();
    client.get_private_channels().await.unwrap();
    client.open_dm(USER).await.unwrap();
    let page = MemberQuery { limit: Some(1000), after: Some(USER.into()) };
    client.list_guild_members(GUILD, &page).await.unwrap();
    client.list_guild_members(GUILD, &MemberQuery::default()).await.unwrap();

    let requests = fixture.requests();
    let lines: Vec<_> = requests.iter().map(line).collect();
    assert_eq!(
        lines,
        vec![
            expect("GET", format!("/api/v1/users/{USER}/profile?with_mutual_friends=true&with_mutual_guilds=true")),
            expect(
                "GET",
                format!("/api/v1/users/@me/profile?guild_id={GUILD}&with_mutual_friends=false&with_mutual_guilds=true")
            ),
            expect("GET", "/api/v1/users/@me/channels"),
            expect("POST", "/api/v1/users/@me/channels"),
            expect("GET", format!("/api/v1/guilds/{GUILD}/members?limit=1000&after={USER}")),
            expect("GET", format!("/api/v1/guilds/{GUILD}/members")),
        ]
    );
    assert_eq!(requests[3].json(), json!({ "recipient_id": USER }));

    let defaults = ProfileQuery::default();
    assert!(matches!(client.get_user_profile("@you", &defaults).await, Err(RestError::InvalidArgument(_))));
    assert!(matches!(client.open_dm("1 2").await, Err(RestError::InvalidArgument(_))));
    for limit in [0, 1001] {
        let bad = MemberQuery { limit: Some(limit), after: None };
        assert!(matches!(client.list_guild_members(GUILD, &bad).await, Err(RestError::InvalidArgument(_))));
    }
    assert_eq!(fixture.requests().len(), 6);
}
