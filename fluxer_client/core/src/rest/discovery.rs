//! Instance discovery (`GET /.well-known/fluxer`).
//!
//! Port of `packages/instance_bootstrap/src/Discovery.ts`,
//! `EndpointNormalization.ts`, `NetworkOrigin.ts` and the official-instance
//! alias in `InstanceSnapshotStore.ts::resolveDiscoveryApiEndpoint`.
//! Differences: https/wss is mandatory (http/ws only for loopback when
//! `allow_insecure_loopback` is set); no ETag cache; the web-only
//! "adopt serving origin" rewrite is not ported.

use std::time::Duration;

use serde_json::Value;
use url::{Host, Url};

use super::error::{DiscoveryAttempt, DiscoveryError};

pub const DISCOVERY_PATH: &str = "/.well-known/fluxer";
const CONVENTIONAL_API_PATH: &str = "/api";
pub const DISCOVERY_MAX_BYTES: usize = 1024 * 1024;
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);
const INPUT_MAX_BYTES: usize = 8192;
/// `CURRENT_INSTANCE_CODENAME` in `packages/instance_bootstrap/src/Types.ts`.
const SUPPORTED_CODENAME: &str = "tungsten";

/// Hosts that alias the official instance (`OFFICIAL_INSTANCE_HOSTS`).
const OFFICIAL_INSTANCE_HOSTS: &[&str] = &[
    "fluxer.com",
    "fluxer.app",
    "web.fluxer.app",
    "api.fluxer.app",
    "canary.fluxer.app",
    "web.canary.fluxer.app",
    "api.canary.fluxer.app",
    "canary.fluxer.com",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReleaseChannel {
    #[default]
    Stable,
    Canary,
}

impl ReleaseChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseChannel::Stable => "stable",
            ReleaseChannel::Canary => "canary",
        }
    }

    fn official_api_endpoint(self) -> &'static str {
        match self {
            ReleaseChannel::Stable => "https://web.fluxer.app/api",
            ReleaseChannel::Canary => "https://web.canary.fluxer.app/api",
        }
    }
}

/// Which plain-text transports are tolerated.
#[derive(Debug, Clone, Copy, Default)]
pub struct TransportPolicy {
    /// Allow `http://` / `ws://` when the host is `localhost` or a loopback IP.
    /// Every other host always requires `https://` / `wss://`.
    pub allow_insecure_loopback: bool,
}

impl TransportPolicy {
    /// True if `url` may be used under this policy.
    pub fn permits(&self, url: &Url) -> bool {
        match url.scheme() {
            "https" | "wss" => true,
            "http" | "ws" => self.allow_insecure_loopback && is_loopback(url),
            _ => false,
        }
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DiscoveryOptions {
    pub policy: TransportPolicy,
    pub release_channel: ReleaseChannel,
}

/// What the client needs from the discovery document.
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoints {
    /// `api_code_version`; REST paths are prefixed with `/v{N}`.
    pub api_code_version: u32,
    /// REST base (`endpoints.api_client`, falling back to `endpoints.api`),
    /// without trailing slash.
    pub api: String,
    /// Gateway WebSocket URL (`endpoints.gateway`).
    pub gateway: String,
    pub media: Option<String>,
    pub static_cdn: Option<String>,
    pub upload_relay: Option<String>,
    pub invite: Option<String>,
    pub webapp: Option<String>,
    /// The candidate URL that served the document.
    pub discovery_url: String,
    /// The raw document (captcha, features, limits, ...) for later use.
    pub document: Value,
}

struct EndpointRule {
    schemes: &'static [&'static str],
    allow_path: bool,
}

const SERVICE_RULE: EndpointRule = EndpointRule { schemes: &["http", "https"], allow_path: true };
const GATEWAY_RULE: EndpointRule = EndpointRule { schemes: &["ws", "wss"], allow_path: true };
const WEBAPP_RULE: EndpointRule = EndpointRule { schemes: &["http", "https"], allow_path: false };

/// `normalizeCanonicalNetworkEndpoint` for absolute endpoints: adds
/// `https://` when no scheme is given, rejects credentials, query, fragment
/// and port 0, and strips trailing slashes from the path.
fn normalize_endpoint(value: &str, rule: &EndpointRule) -> Option<String> {
    if value.len() > INPUT_MAX_BYTES {
        return None;
    }
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return None;
    }
    let candidate = if has_scheme_prefix(trimmed) {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&candidate).ok()?;
    if !rule.schemes.contains(&url.scheme()) {
        return None;
    }
    let host = url.host_str()?;
    if host.is_empty() || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    if url.port() == Some(0) || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    if !path.is_empty() && !rule.allow_path {
        return None;
    }
    let mut out = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        out.push_str(&format!(":{port}"));
    }
    out.push_str(path);
    Some(out)
}

/// `^[a-zA-Z][a-zA-Z0-9+.-]*://`
fn has_scheme_prefix(value: &str) -> bool {
    let Some(sep) = value.find("://") else {
        return false;
    };
    let scheme = &value[..sep];
    let mut chars = scheme.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// Normalizes what the user typed ("example.com", "https://example.com/api/")
/// into an absolute API endpoint and maps official-instance hosts to the
/// official client API (`resolveDiscoveryApiEndpoint`, desktop branch).
pub fn resolve_instance_input(input: &str, channel: ReleaseChannel) -> Result<String, DiscoveryError> {
    let normalized = normalize_endpoint(input, &SERVICE_RULE)
        .ok_or_else(|| DiscoveryError::InvalidInput(truncate_for_message(input)))?;
    let url = Url::parse(&normalized).map_err(|_| DiscoveryError::InvalidInput(truncate_for_message(input)))?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let authority = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    if url.scheme() == "https" {
        let path = url.path().trim_end_matches('/');
        if path == CONVENTIONAL_API_PATH {
            if let Some(official) = official_alias_channel(&authority) {
                return Ok(official.official_api_endpoint().to_owned());
            }
        }
        if path.is_empty() && OFFICIAL_INSTANCE_HOSTS.contains(&authority.as_str()) {
            return Ok(channel.official_api_endpoint().to_owned());
        }
    }
    Ok(normalized)
}

/// `OFFICIAL_CLIENT_API_HOST_CHANNELS`: host of each official client API and
/// the migrated web-app host.
fn official_alias_channel(authority: &str) -> Option<ReleaseChannel> {
    match authority {
        "web.fluxer.app" | "fluxer.com" => Some(ReleaseChannel::Stable),
        "web.canary.fluxer.app" | "canary.fluxer.com" => Some(ReleaseChannel::Canary),
        _ => None,
    }
}

/// User input for an error message: userinfo (`user:pass@`) masked, then
/// truncated to 200 chars.
fn truncate_for_message(input: &str) -> String {
    redact_userinfo(input).chars().take(200).collect()
}

/// True if `url` carries a username or password.
pub(crate) fn has_userinfo(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

/// Replaces any `userinfo@` in the authority of a URL-like string with
/// `***@`. Works on unparsable input too (no scheme, odd characters).
pub(crate) fn redact_userinfo(input: &str) -> String {
    let authority_start = input.find("://").map_or(0, |i| i + 3);
    let rest = &input[authority_start..];
    let authority_len = rest.find(['/', '?', '#', '\\']).unwrap_or(rest.len());
    match rest[..authority_len].rfind('@') {
        Some(at) => format!("{}***{}", &input[..authority_start], &rest[at..]),
        None => input.to_owned(),
    }
}

/// `buildInstanceDiscoveryCandidates`: origin root, the carried path, then
/// the conventional `/api` path, de-duplicated.
pub fn discovery_candidates(endpoint: &str) -> Vec<String> {
    let Some(base) = normalize_endpoint(endpoint, &SERVICE_RULE) else {
        return Vec::new();
    };
    let Ok(url) = Url::parse(&base) else {
        return Vec::new();
    };
    let origin = url.origin().ascii_serialization();
    let path = url.path().trim_end_matches('/');
    let mut candidates = Vec::with_capacity(3);
    let mut push = |value: String| {
        if !candidates.contains(&value) {
            candidates.push(value);
        }
    };
    push(format!("{origin}{DISCOVERY_PATH}"));
    if !path.is_empty() {
        push(format!("{origin}{path}{DISCOVERY_PATH}"));
    }
    push(format!("{origin}{CONVENTIONAL_API_PATH}{DISCOVERY_PATH}"));
    candidates
}

/// Discovers an instance with default options (https only, stable channel).
pub async fn discover(instance: &str) -> Result<Endpoints, DiscoveryError> {
    discover_with(instance, &DiscoveryOptions::default()).await
}

pub async fn discover_with(instance: &str, options: &DiscoveryOptions) -> Result<Endpoints, DiscoveryError> {
    let endpoint = resolve_instance_input(instance, options.release_channel)?;
    let endpoint_url = Url::parse(&endpoint).map_err(|_| DiscoveryError::InvalidInput(truncate_for_message(instance)))?;
    if !options.policy.permits(&endpoint_url) {
        return Err(DiscoveryError::InsecureUrl(endpoint));
    }
    // Same request shape as the TS client: no redirects, no cookies,
    // 15 s deadline, JSON only. TLS verification stays on.
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(DiscoveryError::Client)?;
    let mut attempts = Vec::new();
    for url in discovery_candidates(&endpoint) {
        match fetch_candidate(&http, &url, options).await {
            Ok(endpoints) => return Ok(endpoints),
            Err(CandidateError::Fatal(error)) => return Err(error),
            Err(CandidateError::Skip(reason)) => {
                tracing::debug!(%url, %reason, "discovery candidate failed");
                attempts.push(DiscoveryAttempt { url, reason });
            }
        }
    }
    Err(DiscoveryError::Unreachable(attempts))
}

enum CandidateError {
    /// Stop trying other candidates.
    Fatal(DiscoveryError),
    /// Record and try the next candidate.
    Skip(String),
}

async fn fetch_candidate(
    http: &reqwest::Client,
    url: &str,
    options: &DiscoveryOptions,
) -> Result<Endpoints, CandidateError> {
    let mut response = http
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| CandidateError::Skip(format!("request failed: {}", e.without_url())))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(CandidateError::Skip(format!("responded HTTP {status}")));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !is_json_content_type(content_type) {
        return Err(CandidateError::Skip("responded without a JSON content type".to_owned()));
    }
    if response.content_length().is_some_and(|len| len > DISCOVERY_MAX_BYTES as u64) {
        return Err(CandidateError::Skip("exceeded the discovery document byte cap".to_owned()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| CandidateError::Skip(format!("body read failed: {}", e.without_url())))?
    {
        if body.len() + chunk.len() > DISCOVERY_MAX_BYTES {
            return Err(CandidateError::Skip("exceeded the discovery document byte cap".to_owned()));
        }
        body.extend_from_slice(&chunk);
    }
    let document: Value = serde_json::from_slice(&body)
        .map_err(|e| CandidateError::Skip(format!("invalid JSON: {e}")))?;
    parse_discovery_document(url, document, &options.policy)
}

fn is_json_content_type(value: &str) -> bool {
    let essence = value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    essence == "application/json" || essence.ends_with("+json")
}

/// Validates a discovery document served from `served_from` and extracts the
/// endpoints. Public for tests and for callers that cache documents.
fn parse_discovery_document(
    served_from: &str,
    document: Value,
    policy: &TransportPolicy,
) -> Result<Endpoints, CandidateError> {
    let skip = |reason: &str| CandidateError::Skip(reason.to_owned());
    let source = document.as_object().ok_or_else(|| skip("the response body is not a JSON object"))?;
    if let Some(codename) = source.get("codename").and_then(Value::as_str) {
        if !codename.is_empty() && codename != SUPPORTED_CODENAME {
            return Err(CandidateError::Fatal(DiscoveryError::RequiresNewerClient(codename.to_owned())));
        }
    }
    let api_code_version = source
        .get("api_code_version")
        .and_then(Value::as_u64)
        .filter(|v| *v >= 1)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| skip("api_code_version is not a positive integer"))?;
    let endpoints = source
        .get("endpoints")
        .and_then(Value::as_object)
        .ok_or_else(|| skip("endpoints is missing or not an object"))?;
    let read = |key: &str, rule: &EndpointRule| -> Option<String> {
        endpoints.get(key).and_then(Value::as_str).and_then(|v| normalize_endpoint(v, rule))
    };
    let api = read("api", &SERVICE_RULE)
        .ok_or_else(|| skip("endpoints.api is missing or not an absolute http(s) endpoint"))?;
    let gateway = read("gateway", &GATEWAY_RULE)
        .ok_or_else(|| skip("endpoints.gateway is missing or not a ws(s) endpoint"))?;
    let api_client = read("api_client", &SERVICE_RULE);

    // requireDiscoveryDeclaresItsOwnOrigin
    let served_by = origin_of(served_from).ok_or_else(|| skip("candidate URL has no origin"))?;
    for declared in [Some(&api), api_client.as_ref()].into_iter().flatten() {
        if origin_of(declared).as_deref() != Some(served_by.as_str()) {
            return Err(CandidateError::Skip(
                DiscoveryError::OriginMismatch { served_by: served_by.clone(), declared: declared.clone() }
                    .to_string(),
            ));
        }
    }

    let endpoints_out = Endpoints {
        api_code_version,
        api: api_client.unwrap_or(api),
        gateway,
        media: read("media", &SERVICE_RULE),
        static_cdn: read("static_cdn", &SERVICE_RULE),
        upload_relay: read("upload_relay", &SERVICE_RULE),
        invite: read("invite", &SERVICE_RULE),
        webapp: read("webapp", &WEBAPP_RULE),
        discovery_url: served_from.to_owned(),
        document: Value::Null,
    };
    let declared = [
        Some(&endpoints_out.api),
        Some(&endpoints_out.gateway),
        endpoints_out.media.as_ref(),
        endpoints_out.static_cdn.as_ref(),
        endpoints_out.upload_relay.as_ref(),
        endpoints_out.invite.as_ref(),
        endpoints_out.webapp.as_ref(),
    ];
    for url in declared.into_iter().flatten() {
        let parsed = Url::parse(url).map_err(|_| skip("endpoint is not a valid URL"))?;
        if !policy.permits(&parsed) {
            return Err(CandidateError::Fatal(DiscoveryError::InsecureUrl(url.clone())));
        }
    }
    Ok(Endpoints { document, ..endpoints_out })
}

fn origin_of(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    let origin = url.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

#[cfg(test)]
pub(crate) fn parse_document_for_test(
    served_from: &str,
    document: Value,
    policy: &TransportPolicy,
) -> Result<Endpoints, String> {
    parse_discovery_document(served_from, document, policy).map_err(|e| match e {
        CandidateError::Fatal(error) => error.to_string(),
        CandidateError::Skip(reason) => reason,
    })
}
