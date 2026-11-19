//! Trusted external domain checks (`TrustedDomain.ts`): separates safe
//! first-party / user-trusted domains from arbitrary external links shown in
//! messages, embeds, and the media viewer.

use url::Url;

pub const BUILT_IN_TRUST_PATTERNS: &[&str] = &[
    "fluxer.app",
    "*.fluxer.app",
    "fluxer.com",
    "*.fluxer.com",
    "fluxer.dev",
    "*.fluxer.dev",
    "fluxerstatus.com",
    "*.fluxerstatus.com",
    "fluxer.gg",
    "fluxer.gift",
];

pub const TRUST_ALL_PATTERN: &str = "*";
pub const MAX_TRUSTED_DOMAINS: usize = 1000;
const MAX_DOMAIN_LEN: usize = 253;

/// Normalizes a hostname or pattern (`trim`, ASCII lowercase, strip trailing dot).
pub fn to_comparable_host(host: &str) -> String {
    let trimmed = host.trim().to_ascii_lowercase();
    trimmed.strip_suffix('.').unwrap_or(&trimmed).to_owned()
}

/// Parses an external HTTP/HTTPS link and returns `(normalized_url, comparable_host)`.
/// Rejects non-HTTP schemes (`javascript:`, `file:`, `data:`, etc.) and empty hosts.
pub fn parse_external_http_url(raw: &str) -> Option<(String, String)> {
    let url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str().map(to_comparable_host).filter(|h| !h.is_empty())?;
    Some((url.as_str().to_owned(), host))
}

/// Extracts unique normalized hostnames from configured HTTP/HTTPS/WS/WSS endpoints.
pub fn hosts_from_urls<'a>(urls: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut hosts = Vec::new();
    for raw in urls {
        let Ok(url) = Url::parse(raw.trim()) else { continue };
        if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") {
            continue;
        }
        let Some(host) = url.host_str().map(to_comparable_host).filter(|h| !h.is_empty()) else {
            continue;
        };
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    hosts
}

/// Whether `pattern` covers `hostname` (`TrustedDomain.ts` semantics).
pub fn covers_host(pattern: &str, hostname: &str) -> bool {
    let normalized_host = to_comparable_host(hostname);
    let raw_pattern = pattern.trim().to_ascii_lowercase();
    if normalized_host.is_empty() || raw_pattern.is_empty() {
        return false;
    }
    if raw_pattern == TRUST_ALL_PATTERN {
        return true;
    }
    if let Some(base) = raw_pattern.strip_prefix("*.") {
        let base = base.trim_end_matches('.');
        if base.is_empty() || base.contains('*') {
            return false;
        }
        return normalized_host == base || normalized_host.ends_with(&format!(".{base}"));
    }
    let normalized_pattern = to_comparable_host(&raw_pattern);
    !normalized_pattern.is_empty() && !normalized_pattern.contains('*') && normalized_host == normalized_pattern
}

/// Whether `hostname` is trusted via built-in patterns, the current instance's
/// configured endpoints, or the user's trusted domain list.
pub fn is_trusted(hostname: &str, instance_hosts: &[String], user_trusted: &[String]) -> bool {
    let normalized = to_comparable_host(hostname);
    if normalized.is_empty() {
        return false;
    }
    BUILT_IN_TRUST_PATTERNS
        .iter()
        .copied()
        .chain(instance_hosts.iter().map(String::as_str))
        .chain(user_trusted.iter().map(String::as_str))
        .any(|pattern| covers_host(pattern, &normalized))
}

/// Safe non-HTTP external URI schemes launched directly (blocks `javascript:`, `file:`, `data:`, etc.).
pub fn is_safe_non_http_uri(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw.trim()) else { return false };
    matches!(url.scheme(), "mailto" | "tel")
}

/// Normalizes and appends `host` to `domains` when valid and not already covered.
pub fn add_trusted_domain(domains: &mut Vec<String>, host: &str) -> bool {
    let raw = host.trim().to_ascii_lowercase();
    let normalized = to_comparable_host(&raw);
    if raw == TRUST_ALL_PATTERN {
        if domains.iter().any(|existing| to_comparable_host(existing) == TRUST_ALL_PATTERN) {
            return false;
        }
        domains.clear();
        domains.push(TRUST_ALL_PATTERN.to_owned());
        return true;
    }
    if normalized.is_empty()
        || normalized.len() > MAX_DOMAIN_LEN
        || raw.contains('*')
        || normalized.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return false;
    }
    if domains.iter().any(|existing| {
        let cmp = to_comparable_host(existing);
        cmp == TRUST_ALL_PATTERN || cmp == normalized
    }) {
        return false;
    }
    if domains.len() >= MAX_TRUSTED_DOMAINS {
        return false;
    }
    domains.push(normalized);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_and_instance_hosts_are_trusted_while_spoofed_urls_are_not() {
        let instance = hosts_from_urls(["https://chat.selfhost.example/api", "wss://gw.selfhost.example/ws"]);
        assert_eq!(instance, vec!["chat.selfhost.example", "gw.selfhost.example"]);

        assert!(is_trusted("fluxer.app", &instance, &[]));
        assert!(is_trusted("CDN.FLUXER.APP.", &instance, &[]));
        assert!(is_trusted("chat.selfhost.example", &instance, &[]));
        assert!(!is_trusted("evil.example", &instance, &[]));
        assert!(!is_trusted("notfluxer.app", &instance, &[]));
        assert!(!is_trusted("", &instance, &[]));

        let (url, host) = parse_external_http_url("https://fluxer.app@evil.example/login").expect("valid http url");
        assert_eq!(url, "https://fluxer.app@evil.example/login");
        assert_eq!(host, "evil.example");
        assert!(!is_trusted(&host, &instance, &[]));

        assert_eq!(parse_external_http_url("javascript:alert(1)"), None);
        assert_eq!(parse_external_http_url("file:///C:/Windows/System32"), None);
    }

    #[test]
    fn covers_host_handles_wildcards_and_user_list_mutations() {
        assert!(covers_host("*.fluxer.dev", "fluxer.dev"));
        assert!(covers_host("*.fluxer.dev", "docs.fluxer.dev"));
        assert!(!covers_host("*.fluxer.dev", "badfluxer.dev"));
        assert!(!covers_host("*.", "example.org"));
        assert!(!covers_host("*..", "example.org"));
        assert!(!covers_host("*.*.fluxer.app", "a.b.fluxer.app"));
        assert!(covers_host("*", "anything.example"));

        let mut list = Vec::new();
        assert!(add_trusted_domain(&mut list, "Docs.RS."));
        assert!(!add_trusted_domain(&mut list, "docs.rs"));
        assert!(!add_trusted_domain(&mut list, "bad host"));
        assert_eq!(list, vec!["docs.rs"]);
        assert!(is_trusted("docs.rs", &[], &list));

        let mut all = vec!["*".to_owned()];
        assert!(!add_trusted_domain(&mut all, "new.example"));
    }
}
