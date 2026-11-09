// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

use crate::sqlite_store::SqliteStore;

pub struct AccountEngineManager {
    store: Arc<SqliteStore>,
    foreground_account: Mutex<Option<String>>,
    client_config: Mutex<Option<Value>>,
    provided_accounts: Mutex<Vec<Value>>,
}

impl AccountEngineManager {
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self {
            store,
            foreground_account: Mutex::new(None),
            client_config: Mutex::new(None),
            provided_accounts: Mutex::new(Vec::new()),
        }
    }

    pub fn store(&self) -> &Arc<SqliteStore> {
        &self.store
    }

    /// 웹 렌더러로부터 전달된 AccountEngine 요청을 처리하고 결과를 반환합니다.
    pub fn handle_request(&self, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "configure" => {
                let mut cfg = self.client_config.lock().unwrap();
                *cfg = Some(params.clone());
                Ok(Value::Null)
            }
            "claimForeground" => {
                let acc = params.get("accountKey").and_then(Value::as_str).map(String::from);
                let mut fg = self.foreground_account.lock().unwrap();
                *fg = acc;
                Ok(Value::Null)
            }
            "getSummaries" => {
                let summaries = self.store.all_summaries().map_err(|e| e.to_string())?;
                Ok(Value::Array(summaries))
            }
            "getActivity" => {
                let channel_limit = params.get("channelLimit").and_then(Value::as_u64).unwrap_or(50) as u32;
                let mention_limit = params.get("mentionLimit").and_then(Value::as_u64).unwrap_or(50) as u32;

                let mut target_accounts = Vec::new();
                if let Some(keys) = params.get("accountKeys").and_then(Value::as_array) {
                    for k in keys {
                        if let Some(s) = k.as_str() {
                            target_accounts.push(s.to_string());
                        }
                    }
                }

                // 지정된 계정이 없으면 등록된 요약본 및 제공된 계정들에서 추출
                if target_accounts.is_empty() {
                    if let Ok(summaries) = self.store.all_summaries() {
                        for s in summaries {
                            if let Some(k) = s.get("accountKey").and_then(Value::as_str)
                                && !target_accounts.contains(&k.to_string())
                            {
                                target_accounts.push(k.to_string());
                            }
                        }
                    }
                    let prov = self.provided_accounts.lock().unwrap();
                    for a in prov.iter() {
                        if let Some(k) = a.get("storageKey").and_then(Value::as_str)
                            && !target_accounts.contains(&k.to_string())
                        {
                            target_accounts.push(k.to_string());
                        }
                    }
                }

                let mut activity_list = Vec::new();
                for acc in &target_accounts {
                    let mentions = self.store.list_mentions(acc, mention_limit).unwrap_or_default();
                    // 미읽음 채널 목록 추출 (rows 테이블의 channel_states 엔티티 접두사 조회)
                    let unread_channels = self
                        .store
                        .rows_by_prefix(acc, "channel_states", "")
                        .unwrap_or_default()
                        .into_iter()
                        .take(channel_limit as usize)
                        .map(|(k, _)| k)
                        .collect::<Vec<String>>();

                    activity_list.push(json!({
                        "accountKey": acc,
                        "unreadChannels": unread_channels,
                        "mentions": mentions
                    }));
                }

                Ok(Value::Array(activity_list))
            }
            "beginPromotion" => {
                let account_key = params
                    .get("accountKey")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                // 기존 커서 및 스냅샷 엔트리 로드
                let cursor = self.store.read_cursor(account_key).map_err(|e| e.to_string())?;
                if let Some(c) = cursor {
                    Ok(json!({
                        "accountKey": account_key,
                        "cursor": c,
                        "entries": {}
                    }))
                } else {
                    Ok(Value::Null)
                }
            }
            "completePromotion" => {
                Ok(Value::Null)
            }
            "abortPromotion" => {
                Ok(Value::Null)
            }
            "demote" => {
                // 강등 시 캡처된 스냅샷 엔트리를 rows 테이블에 영구 커밋
                let account_key = params
                    .get("accountKey")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                if let Some(cursor_val) = params.get("cursor")
                    && let Ok(cursor) = serde_json::from_value(cursor_val.clone())
                {
                    let _ = self.store.write_cursor(account_key, &cursor);
                }

                if let Some(entries_obj) = params.get("entries").and_then(Value::as_object) {
                    for (entity, records_val) in entries_obj {
                        if let Some(records) = records_val.as_object() {
                            let kvs: Vec<(String, String)> = records
                                .iter()
                                .map(|(k, v)| (k.clone(), v.as_str().map(String::from).unwrap_or_else(|| v.to_string())))
                                .collect();
                            let _ = self.store.replace_entity(account_key, entity, &kvs);
                        }
                    }
                }

                Ok(Value::Null)
            }
            "ackChannel" => {
                let account_key = params
                    .get("accountKey")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let message_id = params
                    .get("messageId")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                if !account_key.is_empty() && !message_id.is_empty() {
                    let _ = self.store.delete_mention(account_key, message_id);
                }
                Ok(json!(true))
            }
            "reconnect" => {
                Ok(Value::Null)
            }
            "setUnreadBadgeCustomization" => {
                Ok(Value::Null)
            }
            unknown => {
                eprintln!("[AccountEngine] Unknown request method: {unknown}");
                Ok(Value::Null)
            }
        }
    }

    /// 웹 렌더러가 주입한 계정 목록을 등록합니다.
    pub fn provide_accounts(&self, accounts: Vec<Value>) -> Result<bool, String> {
        let mut prov = self.provided_accounts.lock().unwrap();
        *prov = accounts;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_engine_request_routing() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("in-memory db"));
        let manager = AccountEngineManager::new(store);

        // 1. Configure
        let cfg_res = manager.handle_request("configure", &json!({"badgeEnabled": true})).unwrap();
        assert_eq!(cfg_res, Value::Null);

        // 2. Claim Foreground
        let fg_res = manager.handle_request("claimForeground", &json!({"accountKey": "acc_1"})).unwrap();
        assert_eq!(fg_res, Value::Null);
        assert_eq!(manager.foreground_account.lock().unwrap().as_deref(), Some("acc_1"));

        // 3. Get Summaries (초기 빈 배열)
        let sum_res = manager.handle_request("getSummaries", &Value::Null).unwrap();
        assert_eq!(sum_res, json!([]));

        // 4. 요약본 삽입 후 조회
        manager.store().set_summary("acc_1", r#"{"accountKey":"acc_1","unreadCount":5}"#, 1700000000).unwrap();
        let sum_res2 = manager.handle_request("getSummaries", &Value::Null).unwrap();
        let arr = sum_res2.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["accountKey"], "acc_1");
        assert_eq!(arr[0]["unreadCount"], 5);

        // 5. Activity 조회
        manager.store().insert_mention("acc_1", "msg_123", 1000, r#"{"messageId":"msg_123","content":"ping"}"#).unwrap();
        let act_res = manager.handle_request("getActivity", &json!({"accountKeys": ["acc_1"]})).unwrap();
        let act_arr = act_res.as_array().unwrap();
        assert_eq!(act_arr.len(), 1);
        assert_eq!(act_arr[0]["accountKey"], "acc_1");
        assert_eq!(act_arr[0]["mentions"].as_array().unwrap().len(), 1);

        // 6. AckChannel 멘션 삭제
        let ack_res = manager.handle_request("ackChannel", &json!({"accountKey": "acc_1", "messageId": "msg_123"})).unwrap();
        assert_eq!(ack_res, json!(true));
        let mentions_after = manager.store().list_mentions("acc_1", 10).unwrap();
        assert_eq!(mentions_after.len(), 0);
    }
}
