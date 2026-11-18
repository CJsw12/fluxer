//! Glue between a login and the persistent session store
//! (`fluxer_client_core::session`): records built from discovered
//! endpoints, endpoints rebuilt from the cache, display info from @me.

use std::time::{SystemTime, UNIX_EPOCH};

use fluxer_client_core::rest::{Endpoints, TransportPolicy};
use fluxer_client_core::session::{
    CachedEndpoints, DisplayInfo, InstanceRecord, NewAccount, SessionError, SessionStore, default_dir, instance_key,
};
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
}
