// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};

use crate::config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopStorageEntry {
    pub scope: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopStorageWrite {
    pub scope: String,
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StorageFilePayload {
    #[serde(default)]
    scopes: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    markers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopAccountRecord {
    #[serde(rename = "storageKey")]
    pub storage_key: String,
    pub record: Value,
}

/// 싱글톤 메모리 캐시 및 디스크 동기화 스토리지 엔진
static STORAGE_ENGINE: LazyLock<RwLock<StorageEngine>> = LazyLock::new(|| {
    RwLock::new(StorageEngine::load_or_init())
});

struct StorageEngine {
    storage_path: PathBuf,
    accounts_path: PathBuf,
    payload: StorageFilePayload,
    accounts: HashMap<String, DesktopAccountRecord>,
}

impl StorageEngine {
    fn load_or_init() -> Self {
        let storage_dir = config::get_config_dir().join("storage");
        let _ = fs::create_dir_all(&storage_dir);

        let storage_path = storage_dir.join("desktop_storage.json");
        let accounts_path = storage_dir.join("desktop_accounts.json");

        let mut payload = if storage_path.exists() {
            fs::read_to_string(&storage_path)
                .ok()
                .and_then(|text| serde_json::from_str::<StorageFilePayload>(&text).ok())
                .unwrap_or_default()
        } else {
            StorageFilePayload::default()
        };

        // 데스크톱 스토리지 권한 마커의 무결성을 보장합니다.
        // authority가 committed.v1일 때 phase 마커가 없으면 프론트엔드가 부트스트랩을 중단하므로,
        // 항상 phase: "done" 마커가 완벽한 쌍으로 존재하도록 자동 보장 및 자가 치유합니다.
        let mut modified = false;
        if !payload.markers.contains_key("legacy_import.authority") {
            payload.markers.insert("legacy_import.authority".to_string(), "committed.v1".to_string());
            modified = true;
        }
        if !payload.markers.contains_key("legacy_import.v1") {
            let done_marker = json!({
                "version": 1,
                "phase": "done",
                "updatedAt": 1700000000000_u64
            }).to_string();
            payload.markers.insert("legacy_import.v1".to_string(), done_marker);
            modified = true;
        }

        let accounts: HashMap<String, DesktopAccountRecord> = if accounts_path.exists() {
            fs::read_to_string(&accounts_path)
                .ok()
                .and_then(|text| serde_json::from_str::<Vec<DesktopAccountRecord>>(&text).ok())
                .map(|list| {
                    list.into_iter()
                        .map(|acc| (acc.storage_key.clone(), acc))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            HashMap::new()
        };

        let engine = Self {
            storage_path,
            accounts_path,
            payload,
            accounts,
        };

        if modified {
            engine.persist_storage();
        }

        engine
    }

    fn persist_storage(&self) {
        if let Ok(serialized) = serde_json::to_string_pretty(&self.payload) {
            let _ = fs::write(&self.storage_path, serialized.as_bytes());
        }
    }

    fn persist_accounts(&self) {
        let list: Vec<&DesktopAccountRecord> = self.accounts.values().collect();
        if let Ok(serialized) = serde_json::to_string_pretty(&list) {
            let _ = fs::write(&self.accounts_path, serialized.as_bytes());
        }
    }
}

// ---------------------------------------------------------
// 공개 스토리지 인터페이스 (desktopStorage)
// ---------------------------------------------------------

pub fn get_storage_status() -> Value {
    json!({
        "available": true,
        "authorityExpected": true,
        "quarantined": false
    })
}

pub fn load_scope(scope: &str) -> Vec<DesktopStorageEntry> {
    let engine = STORAGE_ENGINE.read().unwrap();
    if let Some(map) = engine.payload.scopes.get(scope) {
        map.iter()
            .map(|(k, v)| DesktopStorageEntry {
                scope: scope.to_string(),
                key: k.clone(),
                value: v.clone(),
            })
            .collect()
    } else {
        Vec::new()
    }
}

pub fn get_entry(scope: &str, key: &str) -> Option<DesktopStorageEntry> {
    let engine = STORAGE_ENGINE.read().unwrap();
    engine.payload.scopes.get(scope).and_then(|map| {
        map.get(key).map(|val| DesktopStorageEntry {
            scope: scope.to_string(),
            key: key.to_string(),
            value: val.clone(),
        })
    })
}

pub fn set_entry(scope: &str, key: &str, value: &str) {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    let scope_map = engine
        .payload
        .scopes
        .entry(scope.to_string())
        .or_default();
    scope_map.insert(key.to_string(), value.to_string());
    engine.persist_storage();
}

pub fn delete_entry(scope: &str, key: &str) {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    if let Some(scope_map) = engine.payload.scopes.get_mut(scope) {
        scope_map.remove(key);
        engine.persist_storage();
    }
}

pub fn clear_all_for_scope(scope: &str) {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    if engine.payload.scopes.remove(scope).is_some() {
        engine.persist_storage();
    }
}

pub fn clear_all_except(keys_to_keep: &[String]) {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    let keep_set: std::collections::HashSet<&str> = keys_to_keep.iter().map(|s| s.as_str()).collect();

    for scope_map in engine.payload.scopes.values_mut() {
        scope_map.retain(|k, _| keep_set.contains(k.as_str()));
    }
    engine.persist_storage();
}

pub fn set_many(writes: &[DesktopStorageWrite]) {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    for write in writes {
        let scope_map = engine
            .payload
            .scopes
            .entry(write.scope.clone())
            .or_default();
        if let Some(val) = &write.value {
            scope_map.insert(write.key.clone(), val.clone());
        } else {
            scope_map.remove(&write.key);
        }
    }
    engine.persist_storage();
}

pub fn get_marker(key: &str) -> Option<String> {
    let engine = STORAGE_ENGINE.read().unwrap();
    engine.payload.markers.get(key).cloned()
}

pub fn set_marker(key: &str, value: &str) {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    engine
        .payload
        .markers
        .insert(key.to_string(), value.to_string());
    engine.persist_storage();
}

// ---------------------------------------------------------
// 공개 계정 저장소 인터페이스 (desktopAccounts)
// ---------------------------------------------------------

pub fn get_all_accounts() -> Vec<DesktopAccountRecord> {
    let engine = STORAGE_ENGINE.read().unwrap();
    engine.accounts.values().cloned().collect()
}

pub fn get_account(storage_key: &str) -> Option<DesktopAccountRecord> {
    let engine = STORAGE_ENGINE.read().unwrap();
    engine.accounts.get(storage_key).cloned()
}

pub fn upsert_account(account: DesktopAccountRecord) {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    engine.accounts.insert(account.storage_key.clone(), account);
    engine.persist_accounts();
}

pub fn compare_and_swap_account(
    expected: &DesktopAccountRecord,
    replacement: &DesktopAccountRecord,
) -> bool {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    if let Some(current) = engine.accounts.get(&expected.storage_key) {
        if current.record == expected.record {
            engine
                .accounts
                .insert(replacement.storage_key.clone(), replacement.clone());
            engine.persist_accounts();
            return true;
        }
    }
    false
}

pub fn delete_account(storage_key: &str) {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    if engine.accounts.remove(storage_key).is_some() {
        engine.persist_accounts();
    }
}

pub fn prune_accounts(keys_to_keep: &[String]) -> usize {
    let mut engine = STORAGE_ENGINE.write().unwrap();
    let keep_set: std::collections::HashSet<&str> = keys_to_keep.iter().map(|s| s.as_str()).collect();
    let before_len = engine.accounts.len();
    engine.accounts.retain(|k, _| keep_set.contains(k.as_str()));
    let pruned = before_len.saturating_sub(engine.accounts.len());
    if pruned > 0 {
        engine.persist_accounts();
    }
    pruned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_crud_operations() {
        let scope = "test_scope";
        let key = "test_key";
        let value = "test_val_123";

        set_entry(scope, key, value);
        let entry = get_entry(scope, key).expect("entry must exist");
        assert_eq!(entry.value, value);

        let list = load_scope(scope);
        assert!(list.iter().any(|e| e.key == key && e.value == value));

        delete_entry(scope, key);
        assert!(get_entry(scope, key).is_none());
    }

    #[test]
    fn test_storage_marker_operations() {
        let key = "legacy_import.authority";
        let val = "committed.v1";

        set_marker(key, val);
        assert_eq!(get_marker(key), Some(val.to_string()));
    }

    #[test]
    fn test_account_crud_operations() {
        let acc = DesktopAccountRecord {
            storage_key: "test_acc_1".into(),
            record: json!({ "userId": "u123", "token": "secret_tok" }),
        };

        upsert_account(acc.clone());
        let loaded = get_account(&acc.storage_key).expect("account must exist");
        assert_eq!(loaded.record["userId"], "u123");

        let all = get_all_accounts();
        assert!(all.iter().any(|a| a.storage_key == acc.storage_key));

        delete_account(&acc.storage_key);
        assert!(get_account(&acc.storage_key).is_none());
    }
}
