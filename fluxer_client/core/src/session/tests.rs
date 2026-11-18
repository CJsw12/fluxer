use super::*;

const TOKEN: &str = "flx_TEST.token-Value_0123456789abcdefXYZ";
const API: &str = "https://Web.Fluxer.app/api/";

fn instance() -> InstanceRecord {
    InstanceRecord {
        origin: "https://web.fluxer.app".to_owned(),
        api_endpoint: API.to_owned(),
        endpoints: Some(CachedEndpoints {
            api_code_version: 1,
            api: "https://web.fluxer.app/api".to_owned(),
            gateway: "wss://gateway.fluxer.app".to_owned(),
            media: Some("https://media.fluxer.app".to_owned()),
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: Some("https://web.fluxer.app".to_owned()),
            discovered_at_ms: 1,
        }),
    }
}

fn account(user_id: &str, token: &str) -> NewAccount {
    NewAccount {
        user_id: user_id.to_owned(),
        instance: instance(),
        display: DisplayInfo {
            username: format!("user{user_id}"),
            global_name: None,
            avatar: Some("abc123".to_owned()),
        },
        token: SecretToken::new(token),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn account_key_matches_ts_normalization() {
    assert_eq!(
        account_key(API, "42").as_deref(),
        Some("https://web.fluxer.app/api::42")
    );
    assert_eq!(
        instance_key("http://localhost:8088/").as_deref(),
        Some("http://localhost:8088")
    );
    assert_eq!(instance_key("https://x.test/api?x=1"), None);
    assert_eq!(instance_key("https://u:p@x.test/api"), None);
    assert_eq!(instance_key("ftp://x.test"), None);
    assert_eq!(account_key(API, ""), None);
    assert_eq!(account_key(API, "a::b"), None);
}

#[test]
fn debug_output_is_masked() {
    let new = account("1", TOKEN);
    let printed = format!("{new:?}");
    assert!(!printed.contains(TOKEN), "{printed}");
    let enc = EncryptedToken {
        scheme: TOKEN_SCHEME.to_owned(),
        data: "Y2lwaGVy".to_owned(),
    };
    assert_eq!(format!("{enc:?}"), "EncryptedToken(***)");
}

#[cfg(not(windows))]
#[test]
fn non_windows_is_unsupported() {
    assert!(matches!(dpapi::protect(b"x", b"e"), Err(CryptoError::Unsupported)));
}

fn policy(allow_loopback: bool) -> TransportPolicy {
    TransportPolicy {
        allow_insecure_loopback: allow_loopback,
    }
}

#[test]
fn serde_error_descriptions_carry_no_values() {
    let err = serde_json::from_value::<StoredAccount>(serde_json::json!({
        "key": "SENTINEL-KEY-VALUE",
        "user_id": "1",
        "last_active_ms": "SENTINEL-NUMBER-VALUE",
    }))
    .expect_err("type error");
    let described = describe_serde(&err);
    assert!(!described.contains("SENTINEL"), "{described}");

    let err = serde_json::from_value::<CachedEndpoints>(serde_json::json!({
        "api_code_version": "SENTINEL-VERSION",
    }))
    .expect_err("type error");
    assert!(err.to_string().contains("SENTINEL"), "precondition: serde quotes values");
    let described = describe_serde(&err);
    assert!(!described.contains("SENTINEL"), "{described}");

    let err = serde_json::from_slice::<FileIn>(b"{\"version\": \"SENTINEL-VER\"}").err().expect("bad");
    let described = describe_serde(&err);
    assert!(!described.contains("SENTINEL"), "{described}");
    assert!(described.contains("line 1"), "{described}");
}

#[test]
fn instance_origin_must_match_api_endpoint() {
    let strict = policy(false);
    assert_eq!(validate_instance(&instance(), &strict), Ok(()));
    let mut other = instance();
    other.origin = "https://evil.test".to_owned();
    assert!(validate_instance(&other, &strict).is_err());
    other.origin = "fluxer.app".to_owned();
    assert!(validate_instance(&other, &strict).is_err());
    other.origin = "https://web.fluxer.app/api".to_owned();
    assert!(validate_instance(&other, &strict).is_err());

    let local = InstanceRecord {
        origin: "http://localhost:8088".to_owned(),
        api_endpoint: "http://localhost:8088/api".to_owned(),
        endpoints: None,
    };
    assert!(validate_instance(&local, &strict).is_err());
    assert_eq!(validate_instance(&local, &policy(true)), Ok(()));
    let remote_http = InstanceRecord {
        origin: "http://example.test".to_owned(),
        api_endpoint: "http://example.test/api".to_owned(),
        endpoints: None,
    };
    assert!(validate_instance(&remote_http, &policy(true)).is_err());
}

#[test]
fn cached_endpoints_follow_discovery_rules() {
    let strict = policy(false);
    let good = instance().endpoints.expect("endpoints");
    assert_eq!(validate_endpoints(&good, API, &strict), Ok(()));

    let mut cross_origin = good.clone();
    cross_origin.api = "https://evil.test/api".to_owned();
    assert!(validate_endpoints(&cross_origin, API, &strict).is_err());

    let mut plain_gateway = good.clone();
    plain_gateway.gateway = "ws://gateway.fluxer.app".to_owned();
    assert!(validate_endpoints(&plain_gateway, API, &strict).is_err());

    let mut gateway_as_http = good.clone();
    gateway_as_http.gateway = "https://gateway.fluxer.app".to_owned();
    assert!(validate_endpoints(&gateway_as_http, API, &strict).is_err());

    let mut plain_media = good.clone();
    plain_media.media = Some("http://media.fluxer.app".to_owned());
    assert!(validate_endpoints(&plain_media, API, &strict).is_err());

    let mut creds = good;
    creds.upload_relay = Some("https://u:p@relay.fluxer.app".to_owned());
    assert!(validate_endpoints(&creds, API, &strict).is_err());

    let local = CachedEndpoints {
        api_code_version: 1,
        api: "http://127.0.0.1:8088/api".to_owned(),
        gateway: "ws://127.0.0.1:8088/gateway".to_owned(),
        media: None,
        static_cdn: None,
        upload_relay: None,
        invite: None,
        webapp: None,
        discovered_at_ms: 1,
    };
    let local_api = "http://127.0.0.1:8088/api";
    assert!(validate_endpoints(&local, local_api, &strict).is_err());
    assert_eq!(validate_endpoints(&local, local_api, &policy(true)), Ok(()));
}

#[test]
fn second_open_is_locked_until_first_is_dropped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("fluxer_client");
    let first = SessionStore::open(&dir).expect("first open");
    let second = SessionStore::open(&dir);
    assert!(matches!(second, Err(SessionError::Locked { .. })), "{second:?}");
    drop(first);
    SessionStore::open(&dir).expect("open after release");
}

#[test]
fn stale_temp_files_are_removed_fresh_ones_kept() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("fluxer_client");
    fs::create_dir_all(&dir).expect("mkdir");
    let stale = dir.join(format!("{TEMP_PREFIX}-1-00"));
    let legacy = dir.join(TEMP_PREFIX);
    let fresh = dir.join(format!("{TEMP_PREFIX}-2-ff"));
    let old = SystemTime::now() - STALE_TEMP_AGE - Duration::from_secs(60);
    for path in [&stale, &legacy] {
        let file = fs::File::create(path).expect("create");
        file.set_modified(old).expect("set mtime");
    }
    fs::write(&fresh, b"other writer").expect("fresh");

    let store = SessionStore::open(&dir).expect("open");
    assert!(!stale.exists());
    assert!(!legacy.exists());
    assert!(fresh.exists(), "a recent temp file may belong to another writer");
    assert!(store.issues().is_empty());
}

#[test]
fn oversized_or_padded_file_is_quarantined() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("fluxer_client");
    fs::create_dir_all(&dir).expect("mkdir");
    let mut padded = br#"{"version":2,"active":null,"accounts":[]}"#.to_vec();
    padded.resize(usize::try_from(MAX_FILE_BYTES).expect("cap") + 10, b' ');
    fs::write(dir.join(FILE_NAME), &padded).expect("write");
    let store = SessionStore::open(&dir).expect("open");
    assert!(
        matches!(store.issues(), [LoadIssue::CorruptFile { reason, .. }] if reason == "file is too large"),
        "{:?}",
        store.issues()
    );
}

#[test]
fn too_many_records_are_quarantined() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("fluxer_client");
    fs::create_dir_all(&dir).expect("mkdir");
    let records: Vec<serde_json::Value> =
        (0..=MAX_RECORDS).map(|i| serde_json::json!({ "key": i })).collect();
    let body = serde_json::json!({"version": 2, "active": null, "accounts": records});
    fs::write(dir.join(FILE_NAME), serde_json::to_vec(&body).expect("ser")).expect("write");
    let store = SessionStore::open(&dir).expect("open");
    assert!(matches!(store.issues(), [LoadIssue::CorruptFile { .. }]));
}

#[test]
fn only_newest_corrupt_backups_are_kept() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("fluxer_client");
    fs::create_dir_all(&dir).expect("mkdir");
    for stamp in 1..=5u64 {
        fs::write(dir.join(format!("{CORRUPT_PREFIX}{stamp}")), b"old").expect("backup");
    }
    fs::write(dir.join(FILE_NAME), b"garbage").expect("garbage");
    let store = SessionStore::open(&dir).expect("open");
    let [LoadIssue::CorruptFile { backup, .. }] = store.issues() else {
        panic!("unexpected issues {:?}", store.issues());
    };
    assert!(backup.exists(), "the new backup is the newest");
    let count = fs::read_dir(&dir)
        .expect("list")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(CORRUPT_PREFIX))
        .count();
    assert_eq!(count, MAX_CORRUPT_BACKUPS);
    assert!(!dir.join(format!("{CORRUPT_PREFIX}1")).exists());
}

#[cfg(windows)]
mod windows;

