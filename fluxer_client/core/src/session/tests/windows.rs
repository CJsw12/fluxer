//! Store tests that need real DPAPI.

use super::*;

fn store() -> (tempfile::TempDir, SessionStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SessionStore::open(dir.path().join("fluxer_client")).expect("open");
    (dir, store)
}

/// Drops `store` (releasing its lock) and opens the directory again.
fn reopen(store: SessionStore) -> SessionStore {
    let dir = store.dir().to_owned();
    drop(store);
    SessionStore::open(dir).expect("reopen")
}

fn file_bytes(store: &SessionStore) -> Vec<u8> {
    fs::read(store.dir().join(FILE_NAME)).expect("read store file")
}

fn read_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join(FILE_NAME)).expect("read")).expect("parse")
}

fn write_json(dir: &Path, json: &serde_json::Value) {
    fs::write(dir.join(FILE_NAME), serde_json::to_vec(json).expect("ser")).expect("write");
}

/// Edits the file of a closed store and reopens it.
fn edit_and_reopen(store: SessionStore, edit: impl FnOnce(&mut serde_json::Value)) -> SessionStore {
    let dir = store.dir().to_owned();
    drop(store);
    let mut json = read_json(&dir);
    edit(&mut json);
    write_json(&dir, &json);
    SessionStore::open(dir).expect("reopen")
}

fn record_mut<'a>(json: &'a mut serde_json::Value, user_id: &str) -> &'a mut serde_json::Value {
    json["accounts"]
        .as_array_mut()
        .expect("array")
        .iter_mut()
        .find(|r| r["user_id"] == user_id)
        .expect("record")
}

#[test]
fn dpapi_round_trip_and_entropy() {
    let blob = dpapi::protect(TOKEN.as_bytes(), b"entropy-a").expect("protect");
    assert!(!contains(&blob, TOKEN.as_bytes()));
    assert_eq!(
        dpapi::unprotect(&blob, b"entropy-a").expect("unprotect").as_slice(),
        TOKEN.as_bytes()
    );
    assert!(dpapi::unprotect(&blob, b"entropy-b").is_err());
    let mut tampered = blob.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    assert!(dpapi::unprotect(&tampered, b"entropy-a").is_err());
}

#[test]
fn save_reopen_load_token_round_trip() {
    let (_tmp, mut store) = store();
    let key = store.save_account(account("42", TOKEN)).expect("save");
    assert_eq!(key, "https://web.fluxer.app/api::42");

    let reopened = reopen(store);
    assert!(reopened.issues().is_empty(), "{:?}", reopened.issues());
    assert_eq!(reopened.load_token(&key).expect("token").expose(), TOKEN);
    let active = reopened.active_account().expect("active");
    assert_eq!(active.key, key);
    assert_eq!(active.display.username, "user42");
    assert_eq!(active.instance, instance());
    assert_eq!(reopened.list_accounts().len(), 1);
    assert_eq!(read_json(reopened.dir())["version"], FORMAT_VERSION);
}

#[test]
fn multiple_accounts_active_and_remove() {
    let (_tmp, mut store) = store();
    let a = store.save_account(account("1", "token-one")).expect("a");
    std::thread::sleep(Duration::from_millis(5));
    let b = store.save_account(account("2", "token-two")).expect("b");
    assert_eq!(store.active_account().map(|s| s.key), Some(b.clone()));
    assert_eq!(store.list_accounts()[0].key, b);

    std::thread::sleep(Duration::from_millis(5));
    store.set_active(&a).expect("set active");
    assert_eq!(store.list_accounts()[0].key, a);
    assert!(matches!(
        store.set_active("https://x.test::9"),
        Err(SessionError::UnknownAccount)
    ));

    // Re-saving replaces, does not duplicate.
    store.save_account(account("1", "token-one-new")).expect("resave");
    assert_eq!(store.list_accounts().len(), 2);
    assert_eq!(store.load_token(&a).expect("t").expose(), "token-one-new");

    assert!(store.remove_account(&a).expect("remove"));
    assert!(!store.remove_account(&a).expect("remove again"));
    assert_eq!(store.active_account(), None);
    assert!(matches!(store.load_token(&a), Err(SessionError::UnknownAccount)));

    let reopened = reopen(store);
    let keys: Vec<String> = reopened.list_accounts().into_iter().map(|s| s.key).collect();
    assert_eq!(keys, vec![b]);
    assert!(!contains(&file_bytes(&reopened), a.as_bytes()));
}

#[test]
fn token_never_on_disk_in_plaintext() {
    let (_tmp, mut store) = store();
    store.save_account(account("7", TOKEN)).expect("save");
    let bytes = file_bytes(&store);
    assert!(!contains(&bytes, TOKEN.as_bytes()));
    assert!(!contains(&bytes, BASE64.encode(TOKEN).as_bytes()));
    // Sanity: the file does hold the non-secret fields.
    assert!(contains(&bytes, b"user7"));
    assert!(contains(&bytes, TOKEN_SCHEME.as_bytes()));
}

#[test]
fn interrupted_write_keeps_previous_state() {
    let (_tmp, mut store) = store();
    let key = store.save_account(account("5", TOKEN)).expect("save");
    let before = file_bytes(&store);

    // Simulate a crash after the temp file was (partly) written but
    // before the rename.
    let temp = write_temp(store.dir(), b"{\"version\":2,\"accounts\":[{\"key\":").expect("temp");
    assert!(temp.exists());

    let reopened = reopen(store);
    assert!(reopened.issues().is_empty());
    assert_eq!(file_bytes(&reopened), before);
    assert_eq!(reopened.load_token(&key).expect("token").expose(), TOKEN);
    // A fresh temp file may be another writer's; a stale one is removed.
    assert!(temp.exists());
    let old = SystemTime::now() - STALE_TEMP_AGE - Duration::from_secs(1);
    fs::OpenOptions::new()
        .write(true)
        .open(&temp)
        .and_then(|f| f.set_modified(old))
        .expect("age temp");
    let reopened = reopen(reopened);
    assert!(!temp.exists(), "stale temp file must be cleaned up");
    assert!(reopened.issues().is_empty());
}

#[test]
fn corrupt_file_is_quarantined() {
    let (_tmp, store) = store();
    let path = store.dir().join(FILE_NAME);
    fs::write(&path, b"\x00not json{{").expect("write garbage");

    let reopened = reopen(store);
    assert!(reopened.list_accounts().is_empty());
    assert_eq!(reopened.active_account(), None);
    let [LoadIssue::CorruptFile { backup, .. }] = reopened.issues() else {
        panic!("unexpected issues {:?}", reopened.issues());
    };
    assert_eq!(fs::read(backup).expect("backup"), b"\x00not json{{");
    assert!(!path.exists());
}

#[test]
fn unsupported_version_is_quarantined_not_overwritten() {
    let (_tmp, store) = store();
    let path = store.dir().join(FILE_NAME);
    fs::write(&path, br#"{"version":99,"active":null,"accounts":[]}"#).expect("write");
    let reopened = reopen(store);
    assert!(matches!(reopened.issues(), [LoadIssue::CorruptFile { .. }]));
}

#[test]
fn bad_records_are_reported_and_preserved_across_a_save() {
    let (_tmp, mut store) = store();
    let good = store.save_account(account("1", "good-token")).expect("good");
    store.save_account(account("2", "bad-token")).expect("bad");
    store.save_account(account("3", "tampered")).expect("tampered");

    let unknown = serde_json::json!({"key": 5, "future_field": {"x": [1, 2]}});
    let mut bad_records = Vec::new();
    let reopened = edit_and_reopen(store, |json| {
        record_mut(json, "2")["token"]["data"] = serde_json::json!("AAAAAAAA");
        record_mut(json, "3")["key"] = serde_json::json!("https://evil.test::3");
        for id in ["2", "3"] {
            bad_records.push(record_mut(json, id).clone());
        }
        json["accounts"].as_array_mut().expect("array").push(unknown.clone());
        bad_records.push(unknown.clone());
        json["active"] = serde_json::json!("https://web.fluxer.app/api::2");
    });
    let keys: Vec<String> = reopened.list_accounts().into_iter().map(|s| s.key).collect();
    assert_eq!(keys, vec![good.clone()]);
    assert_eq!(reopened.active_account(), None, "active pointed at an unusable record");
    assert_eq!(reopened.issues().len(), 3, "{:?}", reopened.issues());
    assert!(reopened.issues().iter().any(|i| matches!(i, LoadIssue::UndecryptableToken { .. })));
    assert_eq!(
        reopened
            .issues()
            .iter()
            .filter(|i| matches!(i, LoadIssue::InvalidRecord { .. }))
            .count(),
        2
    );
    let printed = format!("{reopened:?} {:?}", reopened.issues());
    assert!(!printed.contains("good-token"));
    assert!(matches!(
        reopened.load_token("https://web.fluxer.app/api::2"),
        Err(SessionError::UnknownAccount)
    ));
    assert_eq!(reopened.load_token(&good).expect("token").expose(), "good-token");

    // A save writes the unusable records back unchanged.
    let mut reopened = reopened;
    reopened.save_account(account("4", "token-four")).expect("save four");
    let json = read_json(reopened.dir());
    let on_disk = json["accounts"].as_array().expect("array");
    for record in &bad_records {
        assert!(on_disk.contains(record), "lost {record}");
    }
    assert_eq!(on_disk.len(), 5);

    // Saving the same key again replaces the unusable record.
    reopened.save_account(account("2", "fresh-two")).expect("relogin two");
    let mut reopened = reopen(reopened);
    assert_eq!(reopened.issues().len(), 2, "{:?}", reopened.issues());
    assert_eq!(
        reopened.load_token("https://web.fluxer.app/api::2").expect("two").expose(),
        "fresh-two"
    );
    // An unusable record can be removed by key.
    assert!(reopened.remove_account("https://evil.test::3").expect("remove bad"));
    assert_eq!(read_json(reopened.dir())["accounts"].as_array().expect("array").len(), 4);
}

#[test]
fn blob_moved_between_accounts_fails() {
    let (_tmp, mut store) = store();
    store.save_account(account("1", "token-one")).expect("one");
    store.save_account(account("2", "token-two")).expect("two");
    let reopened = edit_and_reopen(store, |json| {
        let one = record_mut(json, "1")["token"].clone();
        let two = record_mut(json, "2")["token"].clone();
        record_mut(json, "1")["token"] = two;
        record_mut(json, "2")["token"] = one;
    });
    assert!(reopened.list_accounts().is_empty());
    assert_eq!(reopened.issues().len(), 2, "{:?}", reopened.issues());
    assert!(
        reopened
            .issues()
            .iter()
            .all(|i| matches!(i, LoadIssue::UndecryptableToken { .. }))
    );
}

#[test]
fn relabelled_record_or_edited_endpoint_fails() {
    let (_tmp, mut store) = store();
    store.save_account(account("1", TOKEN)).expect("save");
    // Same normalized key (trailing slash removed), different raw endpoint:
    // still rejected because the raw endpoint is bound.
    let reopened = edit_and_reopen(store, |json| {
        record_mut(json, "1")["instance"]["api_endpoint"] = serde_json::json!("https://web.fluxer.app/api");
    });
    assert!(reopened.list_accounts().is_empty());
    assert!(matches!(reopened.issues(), [LoadIssue::UndecryptableToken { .. }]));

    // Relabel to an attacker host with a self-consistent key.
    let (_tmp2, mut store) = self::store();
    store.save_account(account("1", TOKEN)).expect("save");
    let reopened = edit_and_reopen(store, |json| {
        let record = record_mut(json, "1");
        record["key"] = serde_json::json!("https://collect.attacker.test/api::1");
        record["instance"]["api_endpoint"] = serde_json::json!("https://collect.attacker.test/api");
        record["instance"]["origin"] = serde_json::json!("https://collect.attacker.test");
        record["instance"]["endpoints"] = serde_json::Value::Null;
    });
    assert!(reopened.list_accounts().is_empty());
    assert!(matches!(reopened.issues(), [LoadIssue::UndecryptableToken { .. }]));
}

#[test]
fn bad_cached_endpoints_are_dropped_record_kept() {
    let (_tmp, mut store) = store();
    let one = store.save_account(account("1", "token-one")).expect("one");
    let two = store.save_account(account("2", "token-two")).expect("two");
    let reopened = edit_and_reopen(store, |json| {
        record_mut(json, "1")["instance"]["endpoints"]["gateway"] = serde_json::json!("ws://gateway.fluxer.app");
        record_mut(json, "2")["instance"]["endpoints"]["api"] = serde_json::json!("https://evil.test/api");
    });
    assert_eq!(reopened.list_accounts().len(), 2);
    assert_eq!(reopened.issues().len(), 2, "{:?}", reopened.issues());
    assert!(
        reopened
            .issues()
            .iter()
            .all(|i| matches!(i, LoadIssue::InvalidCachedEndpoints { .. }))
    );
    for key in [&one, &two] {
        let summary = reopened
            .list_accounts()
            .into_iter()
            .find(|s| &s.key == key)
            .expect("kept");
        assert_eq!(summary.instance.endpoints, None);
        assert!(reopened.load_token(key).is_ok());
    }
}

#[test]
fn save_rejects_bad_instance_or_endpoints() {
    let (_tmp, mut store) = store();
    let mut wrong_origin = account("1", TOKEN);
    wrong_origin.instance.origin = "https://evil.test".to_owned();
    assert!(matches!(
        store.save_account(wrong_origin),
        Err(SessionError::InvalidAccount(_))
    ));
    let mut bad_cache = account("1", TOKEN);
    if let Some(e) = bad_cache.instance.endpoints.as_mut() {
        e.api = "https://evil.test/api".to_owned();
    }
    assert!(matches!(
        store.save_account(bad_cache),
        Err(SessionError::InvalidAccount(_))
    ));
    assert!(!store.dir().join(FILE_NAME).exists());
}

#[test]
fn loopback_records_need_the_allow_flag() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("fluxer_client");
    let local = NewAccount {
        user_id: "9".to_owned(),
        instance: InstanceRecord {
            origin: "http://localhost:8088".to_owned(),
            api_endpoint: "http://localhost:8088/api".to_owned(),
            endpoints: None,
        },
        display: DisplayInfo {
            username: "dev".to_owned(),
            global_name: None,
            avatar: None,
        },
        token: SecretToken::new("dev-token"),
    };
    let mut strict = SessionStore::open(&dir).expect("strict");
    assert!(matches!(
        strict.save_account(local.clone()),
        Err(SessionError::InvalidAccount(_))
    ));
    drop(strict);

    let mut dev = SessionStore::open_with(&dir, policy(true)).expect("dev");
    let key = dev.save_account(local).expect("save local");
    drop(dev);

    // Opened strictly, the record is unusable but kept.
    let mut strict = SessionStore::open(&dir).expect("strict again");
    assert!(strict.list_accounts().is_empty());
    assert!(matches!(strict.issues(), [LoadIssue::InvalidRecord { .. }]));
    strict.save_account(account("1", TOKEN)).expect("save remote");
    drop(strict);

    let dev = SessionStore::open_with(&dir, policy(true)).expect("dev again");
    assert_eq!(dev.load_token(&key).expect("local token").expose(), "dev-token");
    assert_eq!(dev.list_accounts().len(), 2);
}

fn legacy_record(user_id: &str, token: serde_json::Value) -> serde_json::Value {
    let mut value = serde_json::to_value(AccountSummary {
        key: account_key(API, user_id).expect("key"),
        user_id: user_id.to_owned(),
        instance: instance(),
        display: DisplayInfo {
            username: format!("user{user_id}"),
            global_name: None,
            avatar: None,
        },
        last_active_ms: 1,
    })
    .expect("summary");
    value["token"] = token;
    value
}

#[test]
fn legacy_file_is_migrated_and_bad_legacy_records_dropped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("fluxer_client");
    fs::create_dir_all(&dir).expect("mkdir");
    let blob = dpapi::protect(TOKEN.as_bytes(), LEGACY_ENTROPY).expect("protect");
    let good_key = account_key(API, "1").expect("key");
    let body = serde_json::json!({
        "version": 1,
        "active": good_key,
        "accounts": [
            legacy_record("1", serde_json::json!({"scheme": LEGACY_TOKEN_SCHEME, "data": BASE64.encode(&blob)})),
            legacy_record("2", serde_json::json!({"scheme": LEGACY_TOKEN_SCHEME, "data": "AAAA"})),
            {"key": 7},
        ],
    });
    write_json(&dir, &body);

    let store = SessionStore::open(&dir).expect("open legacy");
    assert_eq!(store.issues().len(), 2, "{:?}", store.issues());
    assert!(
        store
            .issues()
            .iter()
            .all(|i| matches!(i, LoadIssue::DroppedLegacyRecord(_)))
    );
    assert_eq!(store.load_token(&good_key).expect("token").expose(), TOKEN);
    assert_eq!(store.active_account().map(|a| a.key), Some(good_key.clone()));

    let json = read_json(&dir);
    assert_eq!(json["version"], FORMAT_VERSION);
    let accounts = json["accounts"].as_array().expect("array");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["token"]["scheme"], TOKEN_SCHEME);

    let reopened = reopen(store);
    assert!(reopened.issues().is_empty(), "{:?}", reopened.issues());
    assert_eq!(reopened.load_token(&good_key).expect("token").expose(), TOKEN);
}

#[test]
fn legacy_scheme_inside_v2_file_is_not_accepted() {
    let (_tmp, mut store) = store();
    store.save_account(account("1", TOKEN)).expect("save");
    let blob = dpapi::protect(TOKEN.as_bytes(), LEGACY_ENTROPY).expect("protect");
    let reopened = edit_and_reopen(store, |json| {
        record_mut(json, "1")["token"] =
            serde_json::json!({"scheme": LEGACY_TOKEN_SCHEME, "data": BASE64.encode(&blob)});
    });
    assert!(reopened.list_accounts().is_empty());
    assert!(matches!(reopened.issues(), [LoadIssue::UndecryptableToken { .. }]));
}

#[test]
fn rename_retries_while_target_is_briefly_held() {
    use std::os::windows::fs::OpenOptionsExt;

    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target.json");
    let source = tmp.path().join("source.json");
    fs::write(&target, b"old").expect("target");
    fs::write(&source, b"new").expect("source");
    // Read-only sharing (no FILE_SHARE_DELETE) blocks replacing the file.
    let holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&target)
        .expect("hold");
    let blocked = fs::rename(&source, &target).expect_err("held target blocks rename");
    assert!(is_transient_rename_error(&blocked), "{blocked:?}");
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        drop(holder);
    });
    rename_with_retry(&source, &target).expect("rename after retry");
    release.join().expect("join");
    assert_eq!(fs::read(&target).expect("read"), b"new");
}

#[test]
fn store_debug_and_errors_never_show_token() {
    let (_tmp, mut store) = store();
    let key = store.save_account(account("8", TOKEN)).expect("save");
    let token = store.load_token(&key).expect("token");
    let printed = format!("{store:?} {token:?} {token}");
    assert!(!printed.contains(TOKEN), "{printed}");
    let err = store.load_token("nope").expect_err("unknown");
    assert!(!err.to_string().contains(TOKEN));
}
