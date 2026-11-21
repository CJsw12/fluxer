//! Safe, zero-new-dependency update checker against GitHub Releases.

use std::cmp::Ordering;
use std::time::Duration;

use reqwest::Url;
use serde::Deserialize;

pub(crate) const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const DEFAULT_RELEASES_API_URL: &str = "https://api.github.com/repos/CJsw12/fluxer/releases/latest";

const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_REDIRECTS: usize = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate {
        current: String,
        latest: String,
    },
    Available {
        current: String,
        latest: String,
        release_url: String,
    },
    Error(String),
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

#[derive(Debug, PartialEq, Eq)]
struct SemVer<'a> {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Option<&'a str>,
}

fn parse_semver(raw: &str) -> Option<SemVer<'_>> {
    let trimmed = raw.trim();
    let stripped = trimmed.strip_prefix(['v', 'V']).unwrap_or(trimmed);
    let without_build = match stripped.split_once('+') {
        Some((before, build)) => {
            if build.is_empty() {
                return None;
            }
            before
        }
        None => stripped,
    };
    let (core, pre) = match without_build.split_once('-') {
        Some((core, pre)) => {
            if pre.is_empty() {
                return None;
            }
            (core, Some(pre))
        }
        None => (without_build, None),
    };
    let mut parts = core.split('.');
    let major = parse_u64_strict(parts.next()?)?;
    let minor = parse_u64_strict(parts.next()?)?;
    let patch = parse_u64_strict(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(SemVer {
        major,
        minor,
        patch,
        pre,
    })
}

fn parse_u64_strict(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse::<u64>().ok()
}

fn compare_pre_release(a: &str, b: &str) -> Ordering {
    let mut a_parts = a.split('.');
    let mut b_parts = b.split('.');
    loop {
        match (a_parts.next(), b_parts.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ap), Some(bp)) => {
                let ord = match (parse_u64_strict(ap), parse_u64_strict(bp)) {
                    (Some(an), Some(bn)) => an.cmp(&bn),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => ap.cmp(bp),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

/// Returns `true` if `latest` is strictly newer than `current` according to SemVer.
/// Returns `false` if either version fails to parse.
pub(crate) fn is_newer_version(current: &str, latest: &str) -> bool {
    let (Some(cur), Some(lat)) = (parse_semver(current), parse_semver(latest)) else {
        return false;
    };
    match (lat.major, lat.minor, lat.patch).cmp(&(cur.major, cur.minor, cur.patch)) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => match (cur.pre, lat.pre) {
            (Some(_), None) => true,
            (None, Some(_)) | (None, None) => false,
            (Some(c_pre), Some(l_pre)) => compare_pre_release(l_pre, c_pre) == Ordering::Greater,
        },
    }
}

fn has_userinfo(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Validates an update API URL: allows `https://` or loopback `http://` (for local tests),
/// and rejects userinfo or non-http(s) schemes.
pub(crate) fn validate_api_url(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|err| format!("Invalid update URL: {err}"))?;
    if has_userinfo(&parsed) {
        return Err("Update URL must not contain userinfo".to_owned());
    }
    match parsed.scheme() {
        "https" => {
            if parsed.host().is_none() {
                return Err("Update URL is missing a host".to_owned());
            }
            Ok(parsed)
        }
        "http" => {
            if is_loopback_host(&parsed) {
                Ok(parsed)
            } else {
                Err("Plain HTTP update URL is only allowed for loopback hosts".to_owned())
            }
        }
        scheme => Err(format!("Unsupported update URL scheme: {scheme}")),
    }
}

/// Validates a GitHub release page URL: requires `https://github.com/` with no userinfo.
pub(crate) fn validate_release_url(url: &str) -> Result<String, String> {
    let parsed = Url::parse(url).map_err(|err| format!("Invalid release URL: {err}"))?;
    if has_userinfo(&parsed) {
        return Err("Release URL must not contain userinfo".to_owned());
    }
    if parsed.scheme() != "https" {
        return Err("Release URL must use https".to_owned());
    }
    if !matches!(parsed.host_str(), Some(host) if host.eq_ignore_ascii_case("github.com")) {
        return Err("Release URL must point to github.com".to_owned());
    }
    Ok(parsed.into())
}

fn normalize_version_label(version: &str) -> String {
    let trimmed = version.trim();
    trimmed.strip_prefix(['v', 'V']).unwrap_or(trimmed).to_owned()
}

pub(crate) async fn check_for_updates(api_url: &str, current_version: &str) -> UpdateStatus {
    let url = match validate_api_url(api_url) {
        Ok(url) => url,
        Err(err) => return UpdateStatus::Error(err),
    };

    let client = match reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(format!("fluxer-client/{current_version}"))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if validate_api_url(attempt.url().as_str()).is_err() {
                attempt.error("redirect to a disallowed URL")
            } else {
                attempt.follow()
            }
        }))
        .build()
    {
        Ok(client) => client,
        Err(err) => return UpdateStatus::Error(format!("Could not build HTTP client: {err}")),
    };

    let mut response = match client.get(url).send().await {
        Ok(resp) => resp,
        Err(err) => return UpdateStatus::Error(format!("Update check failed: {err}")),
    };

    if !response.status().is_success() {
        return UpdateStatus::Error(format!("Update check returned HTTP {}", response.status()));
    }

    if response.content_length().is_some_and(|len| len > MAX_RESPONSE_BYTES as u64) {
        return UpdateStatus::Error("Update response exceeded size limit".to_owned());
    }

    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    return UpdateStatus::Error("Update response exceeded size limit".to_owned());
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(err) => return UpdateStatus::Error(format!("Failed reading update response: {err}")),
        }
    }

    let release: GitHubRelease = match serde_json::from_slice(&body) {
        Ok(release) => release,
        Err(err) => return UpdateStatus::Error(format!("Invalid update response JSON: {err}")),
    };

    let release_url = match validate_release_url(&release.html_url) {
        Ok(url) => url,
        Err(err) => return UpdateStatus::Error(err),
    };

    let current = normalize_version_label(current_version);
    let latest = normalize_version_label(&release.tag_name);

    if is_newer_version(current_version, &release.tag_name) {
        UpdateStatus::Available {
            current,
            latest,
            release_url,
        }
    } else {
        UpdateStatus::UpToDate { current, latest }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn semver_comparison_handles_bumps_prefixes_prereleases_and_malformed() {
        assert!(is_newer_version("0.1.0", "0.1.1"));
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.1.0", "1.0.0"));
        assert!(is_newer_version("v0.1.0", "V0.2.0"));
        assert!(is_newer_version("0.2.0-rc.1", "0.2.0"));
        assert!(is_newer_version("0.2.0-alpha.1", "0.2.0-alpha.2"));
        assert!(is_newer_version("0.1.0+build.1", "0.1.1+build.2"));

        assert!(!is_newer_version("0.2.0", "0.2.0"));
        assert!(!is_newer_version("v0.2.0", "0.2.0"));
        assert!(!is_newer_version("0.2.0", "0.2.0-rc.1"));
        assert!(!is_newer_version("0.2.0", "0.1.9"));
        assert!(!is_newer_version("1.0.0", "0.9.9"));

        assert!(!is_newer_version("0.1", "0.2.0"));
        assert!(!is_newer_version("0.1.0", "not-a-version"));
        assert!(!is_newer_version("0.1.0-", "0.2.0"));
        assert!(!is_newer_version("0.1.0", "0.2.0+"));
    }

    #[test]
    fn validate_release_url_accepts_github_https_and_rejects_untrusted() {
        assert_eq!(
            validate_release_url("https://github.com/CJsw12/fluxer/releases/tag/v0.2.0").as_deref(),
            Ok("https://github.com/CJsw12/fluxer/releases/tag/v0.2.0")
        );
        assert!(validate_release_url("http://github.com/CJsw12/fluxer/releases/tag/v0.2.0").is_err());
        assert!(validate_release_url("file:///C:/Windows/System32/calc.exe").is_err());
        assert!(validate_release_url("https://evil.example.com/CJsw12/fluxer/releases").is_err());
        assert!(validate_release_url("https://user:pass@github.com/CJsw12/fluxer/releases").is_err());
        assert!(validate_release_url("https://user@github.com/CJsw12/fluxer/releases").is_err());
    }

    async fn spawn_mock_http_server(status_line: &str, body: &str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        (format!("http://{addr}/releases/latest"), handle)
    }

    #[tokio::test]
    async fn check_for_updates_returns_available_when_newer() {
        let body = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/CJsw12/fluxer/releases/tag/v0.2.0"}"#;
        let (url, server) = spawn_mock_http_server("200 OK", body).await;

        let status = check_for_updates(&url, "0.1.0").await;
        server.await.expect("server task");

        assert_eq!(
            status,
            UpdateStatus::Available {
                current: "0.1.0".to_owned(),
                latest: "0.2.0".to_owned(),
                release_url: "https://github.com/CJsw12/fluxer/releases/tag/v0.2.0".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn check_for_updates_returns_up_to_date_when_equal_or_older() {
        let body = r#"{"tag_name":"v0.1.0","html_url":"https://github.com/CJsw12/fluxer/releases/tag/v0.1.0"}"#;
        let (url, server) = spawn_mock_http_server("200 OK", body).await;

        let status = check_for_updates(&url, "0.1.0").await;
        server.await.expect("server task");

        assert_eq!(
            status,
            UpdateStatus::UpToDate {
                current: "0.1.0".to_owned(),
                latest: "0.1.0".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn check_for_updates_rejects_untrusted_html_url() {
        let body = r#"{"tag_name":"v0.2.0","html_url":"https://evil.example.com/payload"}"#;
        let (url, server) = spawn_mock_http_server("200 OK", body).await;

        let status = check_for_updates(&url, "0.1.0").await;
        server.await.expect("server task");

        assert!(matches!(status, UpdateStatus::Error(_)));
    }

    #[tokio::test]
    async fn check_for_updates_rejects_invalid_json() {
        let (url, server) = spawn_mock_http_server("200 OK", "{not valid json").await;

        let status = check_for_updates(&url, "0.1.0").await;
        server.await.expect("server task");

        assert!(matches!(status, UpdateStatus::Error(_)));
    }
}
