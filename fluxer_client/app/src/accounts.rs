//! Glue between a login and the persistent session store
//! (`fluxer_client_core::session`): records built from discovered
//! endpoints, endpoints rebuilt from the cache, display info from @me.

use std::time::{SystemTime, UNIX_EPOCH};

use fluxer_client_core::rest::{Endpoints, TransportPolicy};
use fluxer_client_core::session::{
    AccountSummary, CachedEndpoints, DisplayInfo, InstanceRecord, NewAccount, SessionError, SessionStore, default_dir,
    instance_key,
};
use fluxer_client_core::state::Snowflake;
use gpui_kit::SharedString;
use serde_json::Value;

use crate::login::Authenticated;

/// Opens the store in the default directory. `SessionError::Locked` means
/// another client instance is running.
pub fn open_store(allow_insecure_loopback: bool) -> Result<SessionStore, SessionError> {
    SessionStore::open_with(default_dir()?, TransportPolicy { allow_insecure_loopback })
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// `scheme://host[:port]` of the API endpoint; `None` if it is not a URL.
pub fn api_origin(api: &str) -> Option<String> {
    let url = url::Url::parse(api).ok()?;
    let origin = url.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

pub fn cached_endpoints(endpoints: &Endpoints, discovered_at_ms: u64) -> CachedEndpoints {
    CachedEndpoints {
        api_code_version: endpoints.api_code_version,
        api: endpoints.api.clone(),
        gateway: endpoints.gateway.clone(),
        media: endpoints.media.clone(),
        static_cdn: endpoints.static_cdn.clone(),
        upload_relay: endpoints.upload_relay.clone(),
        invite: endpoints.invite.clone(),
        webapp: endpoints.webapp.clone(),
        discovered_at_ms,
    }
}

/// The instance record for freshly discovered endpoints.
pub fn instance_record(endpoints: &Endpoints, discovered_at_ms: u64) -> Option<InstanceRecord> {
    Some(InstanceRecord {
        origin: api_origin(&endpoints.api)?,
        api_endpoint: endpoints.api.clone(),
        endpoints: Some(cached_endpoints(endpoints, discovered_at_ms)),
    })
}

/// Endpoints from the cache. The cache has no discovery document; nothing
/// in this client reads it yet.
pub fn endpoints_from_cache(cache: &CachedEndpoints, origin: &str) -> Endpoints {
    Endpoints {
        api_code_version: cache.api_code_version,
        api: cache.api.clone(),
        gateway: cache.gateway.clone(),
        media: cache.media.clone(),
        static_cdn: cache.static_cdn.clone(),
        upload_relay: cache.upload_relay.clone(),
        invite: cache.invite.clone(),
        webapp: cache.webapp.clone(),
        discovery_url: format!("{}/.well-known/fluxer", origin.trim_end_matches('/')),
        document: Value::Null,
    }
}

/// True if both API endpoints name the same instance (same account keys).
pub fn same_instance(a: &str, b: &str) -> bool {
    matches!((instance_key(a), instance_key(b)), (Some(a), Some(b)) if a == b)
}

/// Username, global name and avatar hash of a user object.
pub fn display_info(user: &Value) -> DisplayInfo {
    let text = |key: &str| user.get(key).and_then(Value::as_str).map(str::to_owned);
    DisplayInfo {
        username: text("username").unwrap_or_default(),
        global_name: text("global_name").filter(|s| !s.is_empty()),
        avatar: text("avatar").filter(|s| !s.is_empty()),
    }
}

/// The account to save after a login. `None` if the API endpoint has no
/// origin (discovery already rejects those).
pub fn new_account(auth: &Authenticated) -> Option<NewAccount> {
    Some(NewAccount {
        user_id: auth.user_id.clone(),
        instance: instance_record(&auth.instance.endpoints, now_ms())?,
        display: display_info(&auth.user),
        token: auth.token.clone(),
    })
}

/// Token-free summary of a stored account rendered by `LoginView` and `AccountSwitcherModal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAccountItem {
    pub key: String,
    pub user_id: String,
    pub display_name: SharedString,
    pub username: SharedString,
    pub initials: SharedString,
    pub avatar_url: Option<SharedString>,
    pub default_avatar: usize,
    pub instance_label: SharedString,
    pub is_active: bool,
}

/// Concise `host[:port]` display label derived from an instance origin URL.
pub fn instance_label(origin: &str) -> String {
    let trimmed = origin.trim();
    let Ok(url) = url::Url::parse(trimmed) else {
        return trimmed.to_owned();
    };
    let Some(host) = url.host_str() else {
        return trimmed.to_owned();
    };
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// Static webp avatar URL for a saved account when cached media endpoint, numeric id, and valid hash exist.
pub fn saved_account_avatar_url(account: &AccountSummary) -> Option<String> {
    if account.user_id.is_empty() || !account.user_id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let media = account.instance.endpoints.as_ref()?.media.as_deref()?;
    let hash = account.display.avatar.as_deref().filter(|h| !h.is_empty())?;
    let hash = hash.strip_prefix("a_").unwrap_or(hash);
    if hash.is_empty() || !hash.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return None;
    }
    Some(format!("{}/avatars/{}/{hash}.webp?size=80", media.trim_end_matches('/'), account.user_id))
}

/// Converts stored accounts into token-free UI items with any active account placed first.
pub fn saved_account_items(accounts: &[AccountSummary], active_key: Option<&str>) -> Vec<SavedAccountItem> {
    let mut out: Vec<SavedAccountItem> = accounts
        .iter()
        .map(|account| {
            let display_name = account
                .display
                .global_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    let u = account.display.username.trim();
                    (!u.is_empty()).then_some(u)
                })
                .unwrap_or(&account.user_id)
                .to_owned();
            let initials = crate::format::initials(&display_name);
            let default_avatar = crate::format::default_avatar_index(
                account.user_id.parse::<u64>().map(Snowflake).unwrap_or(Snowflake(0)),
            );
            let avatar_url = saved_account_avatar_url(account).map(SharedString::from);
            SavedAccountItem {
                key: account.key.clone(),
                user_id: account.user_id.clone(),
                display_name: display_name.into(),
                username: account.display.username.clone().into(),
                initials: initials.into(),
                avatar_url,
                default_avatar,
                instance_label: instance_label(&account.instance.origin).into(),
                is_active: active_key == Some(account.key.as_str()),
            }
        })
        .collect();
    out.sort_by(|a, b| b.is_active.cmp(&a.is_active));
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn endpoints() -> Endpoints {
        Endpoints {
            api_code_version: 1,
            api: "https://web.fluxer.app/api".to_owned(),
            gateway: "wss://gateway.fluxer.app".to_owned(),
            media: Some("https://media.fluxer.app".to_owned()),
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: Some("https://web.fluxer.app".to_owned()),
            discovery_url: "https://web.fluxer.app/.well-known/fluxer".to_owned(),
            document: json!({}),
        }
    }

    #[test]
    fn record_uses_the_api_origin_and_round_trips_the_cache() {
        let record = instance_record(&endpoints(), 7).expect("record");
        assert_eq!(record.origin, "https://web.fluxer.app");
        assert_eq!(record.api_endpoint, "https://web.fluxer.app/api");
        let cache = record.endpoints.expect("cache");
        assert_eq!(cache.discovered_at_ms, 7);
        let back = endpoints_from_cache(&cache, &record.origin);
        assert_eq!(back.api, endpoints().api);
        assert_eq!(back.gateway, endpoints().gateway);
        assert_eq!(back.media, endpoints().media);
        assert_eq!(api_origin("http://localhost:8088/api").as_deref(), Some("http://localhost:8088"));
        assert!(same_instance("https://Web.Fluxer.app/api/", "https://web.fluxer.app/api"));
        assert!(!same_instance("https://web.fluxer.app/api", "https://other.app/api"));
    }

    #[test]
    fn display_info_reads_the_user_object() {
        let info = display_info(&json!({ "id": "1", "username": "me", "global_name": "", "avatar": "abc" }));
        assert_eq!(info.username, "me");
        assert_eq!(info.global_name, None);
        assert_eq!(info.avatar.as_deref(), Some("abc"));
        assert_eq!(display_info(&Value::Null).username, "");
    }

    #[test]
    fn instance_label_and_saved_account_items_format_and_sort_active_first() {
        assert_eq!(instance_label("https://web.fluxer.app"), "web.fluxer.app");
        assert_eq!(instance_label("http://127.0.0.1:8088"), "127.0.0.1:8088");
        assert_eq!(instance_label("not a url"), "not a url");

        let a1 = AccountSummary {
            key: "https://web.fluxer.app/api::100".to_owned(),
            user_id: "100".to_owned(),
            instance: InstanceRecord {
                origin: "https://web.fluxer.app".to_owned(),
                api_endpoint: "https://web.fluxer.app/api".to_owned(),
                endpoints: Some(cached_endpoints(&endpoints(), 1)),
            },
            display: DisplayInfo {
                username: "alice".to_owned(),
                global_name: Some("Alice Display".to_owned()),
                avatar: Some("a_gifhash".to_owned()),
            },
            last_active_ms: 20,
        };
        let a2 = AccountSummary {
            key: "http://localhost:8088/api::200".to_owned(),
            user_id: "200".to_owned(),
            instance: InstanceRecord {
                origin: "http://localhost:8088".to_owned(),
                api_endpoint: "http://localhost:8088/api".to_owned(),
                endpoints: None,
            },
            display: DisplayInfo {
                username: "".to_owned(),
                global_name: None,
                avatar: None,
            },
            last_active_ms: 10,
        };
        let items = saved_account_items(&[a1.clone(), a2], Some("http://localhost:8088/api::200"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].key, "http://localhost:8088/api::200");
        assert!(items[0].is_active);
        assert_eq!(items[0].display_name.as_ref(), "200");
        assert_eq!(items[0].instance_label.as_ref(), "localhost:8088");
        assert_eq!(items[0].avatar_url, None);

        assert_eq!(items[1].key, "https://web.fluxer.app/api::100");
        assert!(!items[1].is_active);
        assert_eq!(items[1].display_name.as_ref(), "Alice Display");
        assert_eq!(
            items[1].avatar_url.as_deref(),
            Some("https://media.fluxer.app/avatars/100/gifhash.webp?size=80")
        );

        let mut evil = a1.clone();
        evil.display.avatar = Some("../evil?x=1".to_owned());
        assert_eq!(saved_account_avatar_url(&evil), None);
        evil.display.avatar = Some("good_hash-1".to_owned());
        evil.user_id = "100/../../evil".to_owned();
        assert_eq!(saved_account_avatar_url(&evil), None);
    }
}
