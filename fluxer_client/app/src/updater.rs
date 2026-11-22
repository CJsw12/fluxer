//! Safe update checker and downloader against GitHub Releases.

use std::cmp::Ordering;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Duration;

use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub(crate) const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const DEFAULT_RELEASES_API_URL: &str = "https://api.github.com/repos/CJsw12/fluxer/releases/latest";

const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_ASSET_BYTES: usize = 128 * 1024 * 1024;
const MAX_ASSET_REDIRECTS: usize = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const ALLOWED_ASSET_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

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
        asset_name: Option<String>,
        asset_url: Option<String>,
        checksum_url: Option<String>,
    },
    Downloading {
        latest: String,
    },
    Downloaded {
        latest: String,
        path: String,
        sha256: String,
        verified: bool,
    },
    Error(String),
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
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

/// Validates a GitHub release asset URL: requires `https://` on an allowed GitHub host
/// (`github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`)
/// with no userinfo.
pub(crate) fn validate_asset_url(url: &str) -> Result<Url, String> {
    validate_asset_url_with_options(url, false)
}

fn validate_asset_url_with_options(url: &str, allow_loopback: bool) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|err| format!("Invalid asset URL: {err}"))?;
    if has_userinfo(&parsed) {
        return Err("Asset URL must not contain userinfo".to_owned());
    }
    match parsed.scheme() {
        "https" => {
            let host = parsed
                .host_str()
                .ok_or_else(|| "Asset URL is missing a host".to_owned())?;
            if ALLOWED_ASSET_HOSTS
                .iter()
                .any(|allowed| host.eq_ignore_ascii_case(allowed))
            {
                Ok(parsed)
            } else {
                Err(format!("Untrusted asset host: {host}"))
            }
        }
        "http" if allow_loopback && is_loopback_host(&parsed) => Ok(parsed),
        "http" => Err("Asset URL must use https".to_owned()),
        scheme => Err(format!("Unsupported asset URL scheme: {scheme}")),
    }
}

/// Sanitizes a release asset filename so it cannot escape the destination directory
/// or use Windows-invalid/reserved names.
pub(crate) fn sanitize_asset_filename(name: &str) -> Result<String, String> {
    if name.is_empty() || name.trim() != name {
        return Err("Asset filename must not be empty or have leading/trailing whitespace".to_owned());
    }
    if name == "." || name == ".." {
        return Err("Asset filename must not be a relative path component".to_owned());
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("Asset filename must not end with a dot or space".to_owned());
    }
    if name.chars().any(|c| {
        c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
    }) {
        return Err("Asset filename contains invalid characters".to_owned());
    }
    let stem = name.split('.').next().unwrap_or(name);
    let upper_stem = stem.to_ascii_uppercase();
    let is_reserved_windows_name = matches!(
        upper_stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if is_reserved_windows_name {
        return Err("Asset filename uses a reserved Windows device name".to_owned());
    }
    Ok(name.to_owned())
}

fn is_windows_client_asset(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("fluxer_client") && (lower.ends_with(".zip") || lower.ends_with(".exe"))
}

fn select_windows_assets(assets: &[GitHubAsset]) -> (Option<String>, Option<String>, Option<String>) {
    let selected_asset = assets
        .iter()
        .find(|asset| {
            let lower = asset.name.to_ascii_lowercase();
            lower.ends_with(".zip")
                && is_windows_client_asset(&asset.name)
                && sanitize_asset_filename(&asset.name).is_ok()
                && validate_asset_url(&asset.browser_download_url).is_ok()
        })
        .or_else(|| {
            assets.iter().find(|asset| {
                is_windows_client_asset(&asset.name)
                    && sanitize_asset_filename(&asset.name).is_ok()
                    && validate_asset_url(&asset.browser_download_url).is_ok()
            })
        });

    let Some(asset) = selected_asset else {
        return (None, None, None);
    };

    let checksum_url = assets
        .iter()
        .find(|entry| {
            entry.name.eq_ignore_ascii_case("SHA256SUMS.txt")
                && validate_asset_url(&entry.browser_download_url).is_ok()
        })
        .map(|entry| entry.browser_download_url.clone());

    (
        Some(asset.name.clone()),
        Some(asset.browser_download_url.clone()),
        checksum_url,
    )
}

/// Parses a `SHA256SUMS.txt` file and returns the lowercase hex SHA-256 digest
/// for `target_filename`, if present.
pub(crate) fn parse_sha256sums(contents: &str, target_filename: &str) -> Option<String> {
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((hash_part, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if hash_part.len() != 64 || !hash_part.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let rest = rest.trim_start();
        let entry_name = rest.strip_prefix('*').unwrap_or(rest);
        let entry_name = entry_name.strip_prefix("./").unwrap_or(entry_name);
        if entry_name == target_filename {
            return Some(hash_part.to_ascii_lowercase());
        }
    }
    None
}

fn encode_hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
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
        .redirect(reqwest::redirect::Policy::none())
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
        let (asset_name, asset_url, checksum_url) = select_windows_assets(&release.assets);
        UpdateStatus::Available {
            current,
            latest,
            release_url,
            asset_name,
            asset_url,
            checksum_url,
        }
    } else {
        UpdateStatus::UpToDate { current, latest }
    }
}

async fn get_with_validated_redirects(
    client: &reqwest::Client,
    initial_url: &str,
    allow_loopback: bool,
) -> Result<reqwest::Response, String> {
    let mut current_url = validate_asset_url_with_options(initial_url, allow_loopback)?;
    let mut redirects = 0usize;

    loop {
        let response = client
            .get(current_url.clone())
            .send()
            .await
            .map_err(|err| format!("Update download request failed: {err}"))?;

        if response.status().is_redirection() {
            if redirects >= MAX_ASSET_REDIRECTS {
                return Err("Too many redirects while downloading update".to_owned());
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|val| val.to_str().ok())
                .ok_or_else(|| "Redirect response missing Location header".to_owned())?;
            let next_url = current_url
                .join(location)
                .map_err(|err| format!("Invalid redirect URL: {err}"))?;
            current_url = validate_asset_url_with_options(next_url.as_str(), allow_loopback)?;
            redirects += 1;
            continue;
        }

        if !response.status().is_success() {
            return Err(format!("Update download returned HTTP {}", response.status()));
        }

        return Ok(response);
    }
}

async fn fetch_expected_sha256(
    client: &reqwest::Client,
    checksum_url: &str,
    target_filename: &str,
    allow_loopback: bool,
) -> Result<String, String> {
    let mut response = get_with_validated_redirects(client, checksum_url, allow_loopback).await?;
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BYTES as u64)
    {
        return Err("Checksum file exceeded size limit".to_owned());
    }

    let mut body = Vec::new();
    loop {
        match response
            .chunk()
            .await
            .map_err(|err| format!("Failed reading checksum file: {err}"))?
        {
            Some(chunk) => {
                if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err("Checksum file exceeded size limit".to_owned());
                }
                body.extend_from_slice(&chunk);
            }
            None => break,
        }
    }

    let text = std::str::from_utf8(&body)
        .map_err(|err| format!("Checksum file is not valid UTF-8: {err}"))?;
    parse_sha256sums(text, target_filename)
        .ok_or_else(|| format!("Checksum entry for {target_filename} not found in SHA256SUMS.txt"))
}

pub(crate) async fn download_update(
    latest: &str,
    asset_name: &str,
    asset_url: &str,
    checksum_url: Option<&str>,
    dest_dir: &Path,
) -> UpdateStatus {
    download_update_with_options(latest, asset_name, asset_url, checksum_url, dest_dir, false).await
}

async fn download_update_with_options(
    latest: &str,
    asset_name: &str,
    asset_url: &str,
    checksum_url: Option<&str>,
    dest_dir: &Path,
    allow_loopback: bool,
) -> UpdateStatus {
    let safe_name = match sanitize_asset_filename(asset_name) {
        Ok(name) => name,
        Err(err) => return UpdateStatus::Error(err),
    };

    let client = match reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(format!("fluxer-client/{CURRENT_VERSION}"))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(err) => return UpdateStatus::Error(format!("Could not build HTTP client: {err}")),
    };

    let expected_sha256 = match checksum_url {
        Some(url) => match fetch_expected_sha256(&client, url, &safe_name, allow_loopback).await {
            Ok(hash) => Some(hash),
            Err(err) => return UpdateStatus::Error(err),
        },
        None => None,
    };

    let mut response = match get_with_validated_redirects(&client, asset_url, allow_loopback).await {
        Ok(resp) => resp,
        Err(err) => return UpdateStatus::Error(err),
    };

    if response
        .content_length()
        .is_some_and(|len| len > MAX_ASSET_BYTES as u64)
    {
        return UpdateStatus::Error("Update asset exceeded 128 MiB size limit".to_owned());
    }

    if let Err(err) = std::fs::create_dir_all(dest_dir) {
        return UpdateStatus::Error(format!("Could not create update directory: {err}"));
    }

    let final_path = dest_dir.join(&safe_name);
    let part_path = dest_dir.join(format!("{safe_name}.part"));

    let stream_result = stream_to_part_and_hash(&mut response, &part_path).await;
    let actual_sha256 = match stream_result {
        Ok(hash) => hash,
        Err(err) => {
            let _ = std::fs::remove_file(&part_path);
            return UpdateStatus::Error(err);
        }
    };

    let verified = match expected_sha256 {
        Some(expected) => {
            if actual_sha256 != expected {
                let _ = std::fs::remove_file(&part_path);
                return UpdateStatus::Error(format!(
                    "SHA-256 checksum mismatch for {safe_name}: expected {expected}, got {actual_sha256}"
                ));
            }
            true
        }
        None => false,
    };

    if let Err(err) = std::fs::rename(&part_path, &final_path) {
        let _ = std::fs::remove_file(&part_path);
        return UpdateStatus::Error(format!("Could not finalize downloaded update: {err}"));
    }

    UpdateStatus::Downloaded {
        latest: normalize_version_label(latest),
        path: final_path.to_string_lossy().into_owned(),
        sha256: actual_sha256,
        verified,
    }
}

async fn stream_to_part_and_hash(
    response: &mut reqwest::Response,
    part_path: &Path,
) -> Result<String, String> {
    let file = std::fs::File::create(part_path)
        .map_err(|err| format!("Could not create temporary update file: {err}"))?;
    let mut writer = BufWriter::new(file);
    let mut hasher = Sha256::new();
    let mut total_bytes = 0usize;

    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|err| format!("Failed reading update asset stream: {err}"))?;
        let Some(chunk) = chunk else {
            break;
        };
        total_bytes = total_bytes
            .checked_add(chunk.len())
            .filter(|&sum| sum <= MAX_ASSET_BYTES)
            .ok_or_else(|| "Update asset exceeded 128 MiB size limit".to_owned())?;
        hasher.update(&chunk);
        writer
            .write_all(&chunk)
            .map_err(|err| format!("Failed writing update file: {err}"))?;
    }

    writer
        .flush()
        .map_err(|err| format!("Failed flushing update file: {err}"))?;
    drop(writer);

    Ok(encode_hex_lower(&hasher.finalize()))
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
                asset_name: None,
                asset_url: None,
                checksum_url: None,
            }
        );
    }

    #[tokio::test]
    async fn check_for_updates_selects_windows_asset_and_checksum() {
        let body = r#"{
            "tag_name":"v0.2.0",
            "html_url":"https://github.com/CJsw12/fluxer/releases/tag/v0.2.0",
            "assets":[
                {"name":"fluxer_client-linux-x86_64.AppImage","browser_download_url":"https://github.com/CJsw12/fluxer/releases/download/v0.2.0/fluxer_client-linux-x86_64.AppImage"},
                {"name":"fluxer_client-windows-x86_64.zip","browser_download_url":"https://github.com/CJsw12/fluxer/releases/download/v0.2.0/fluxer_client-windows-x86_64.zip"},
                {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/CJsw12/fluxer/releases/download/v0.2.0/SHA256SUMS.txt"}
            ]
        }"#;
        let (url, server) = spawn_mock_http_server("200 OK", body).await;

        let status = check_for_updates(&url, "0.1.0").await;
        server.await.expect("server task");

        assert_eq!(
            status,
            UpdateStatus::Available {
                current: "0.1.0".to_owned(),
                latest: "0.2.0".to_owned(),
                release_url: "https://github.com/CJsw12/fluxer/releases/tag/v0.2.0".to_owned(),
                asset_name: Some("fluxer_client-windows-x86_64.zip".to_owned()),
                asset_url: Some(
                    "https://github.com/CJsw12/fluxer/releases/download/v0.2.0/fluxer_client-windows-x86_64.zip"
                        .to_owned(),
                ),
                checksum_url: Some(
                    "https://github.com/CJsw12/fluxer/releases/download/v0.2.0/SHA256SUMS.txt".to_owned(),
                ),
            }
        );
    }

    #[tokio::test]
    async fn check_for_updates_ignores_untrusted_asset_url() {
        let body = r#"{
            "tag_name":"v0.2.0",
            "html_url":"https://github.com/CJsw12/fluxer/releases/tag/v0.2.0",
            "assets":[
                {"name":"fluxer_client-windows-x86_64.exe","browser_download_url":"https://evil.example.com/fluxer_client-windows-x86_64.exe"},
                {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/CJsw12/fluxer/releases/download/v0.2.0/SHA256SUMS.txt"}
            ]
        }"#;
        let (url, server) = spawn_mock_http_server("200 OK", body).await;

        let status = check_for_updates(&url, "0.1.0").await;
        server.await.expect("server task");

        assert_eq!(
            status,
            UpdateStatus::Available {
                current: "0.1.0".to_owned(),
                latest: "0.2.0".to_owned(),
                release_url: "https://github.com/CJsw12/fluxer/releases/tag/v0.2.0".to_owned(),
                asset_name: None,
                asset_url: None,
                checksum_url: None,
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

    #[tokio::test]
    async fn check_for_updates_rejects_cross_origin_redirect() {
        let target_body =
            r#"{"tag_name":"v0.2.0","html_url":"https://github.com/CJsw12/fluxer/releases/tag/v0.2.0"}"#;
        let (target_url, target_server) = spawn_mock_http_server("200 OK", target_body).await;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let redirect_response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let redirect_server = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(redirect_response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        let redirect_url = format!("http://{addr}/releases/latest");

        let status = check_for_updates(&redirect_url, "0.1.0").await;
        redirect_server.await.expect("redirect server task");
        target_server.abort();

        assert!(matches!(status, UpdateStatus::Error(_)));
    }

    struct TempDirGuard(std::path::PathBuf);

    impl TempDirGuard {
        fn new(label: &str) -> Self {
            let unique = format!(
                "fluxer_updater_test_{label}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system time")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).expect("create temp test dir");
            Self(path)
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn validate_asset_url_accepts_allowed_github_hosts_and_rejects_others() {
        assert!(validate_asset_url("https://github.com/CJsw12/fluxer/releases/download/v0.2.0/fluxer_client.zip").is_ok());
        assert!(validate_asset_url("https://objects.githubusercontent.com/github-production-release-asset/123").is_ok());
        assert!(validate_asset_url("https://release-assets.githubusercontent.com/github-production-release-asset/123").is_ok());

        assert!(validate_asset_url("http://github.com/CJsw12/fluxer/releases/download/v0.2.0/fluxer_client.zip").is_err());
        assert!(validate_asset_url("http://127.0.0.1:8080/fluxer_client.zip").is_err());
        assert!(validate_asset_url("https://evil.example.com/fluxer_client.zip").is_err());
        assert!(validate_asset_url("https://user:pass@github.com/fluxer_client.zip").is_err());
        assert!(validate_asset_url("https://user@objects.githubusercontent.com/fluxer_client.zip").is_err());
        assert!(validate_asset_url("file:///C:/fluxer_client.zip").is_err());

        assert!(validate_asset_url_with_options("http://127.0.0.1:8080/fluxer_client.zip", true).is_ok());
        assert!(validate_asset_url_with_options("http://user@127.0.0.1:8080/fluxer_client.zip", true).is_err());
        assert!(validate_asset_url_with_options("https://evil.example.com/fluxer_client.zip", true).is_err());
    }

    #[test]
    fn sanitize_asset_filename_accepts_safe_and_rejects_unsafe() {
        assert_eq!(
            sanitize_asset_filename("fluxer_client-windows-x86_64.zip").as_deref(),
            Ok("fluxer_client-windows-x86_64.zip")
        );
        assert_eq!(
            sanitize_asset_filename("fluxer_client-setup.exe").as_deref(),
            Ok("fluxer_client-setup.exe")
        );

        assert!(sanitize_asset_filename("").is_err());
        assert!(sanitize_asset_filename(" fluxer_client.zip").is_err());
        assert!(sanitize_asset_filename(".").is_err());
        assert!(sanitize_asset_filename("..").is_err());
        assert!(sanitize_asset_filename("../fluxer_client.zip").is_err());
        assert!(sanitize_asset_filename("dir\\fluxer_client.zip").is_err());
        assert!(sanitize_asset_filename("fluxer:client.zip").is_err());
        assert!(sanitize_asset_filename("fluxer_client.zip.").is_err());
        assert!(sanitize_asset_filename("CON.zip").is_err());
        assert!(sanitize_asset_filename("nul").is_err());
    }

    #[test]
    fn parse_sha256sums_handles_standard_binary_and_dot_slash() {
        let valid_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let upper_hash = "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789";
        let content = format!(
            "# release checksums\n\
             {valid_hash}  fluxer_client-windows-x86_64.zip\n\
             {upper_hash} *./fluxer_client-setup.exe\n\
             notahash  other.zip\n"
        );

        assert_eq!(
            parse_sha256sums(&content, "fluxer_client-windows-x86_64.zip").as_deref(),
            Some(valid_hash)
        );
        assert_eq!(
            parse_sha256sums(&content, "fluxer_client-setup.exe").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert_eq!(parse_sha256sums(&content, "other.zip"), None);
        assert_eq!(parse_sha256sums(&content, "missing.zip"), None);
    }

    async fn spawn_mock_bytes_server(
        status_line: &str,
        extra_headers: &str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let header = format!(
            "HTTP/1.1 {status_line}\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            }
        });
        (format!("http://{addr}/asset"), handle)
    }

    #[tokio::test]
    async fn download_update_verifies_sha256_and_renames_atomically() {
        let temp = TempDirGuard::new("verified");
        let payload = b"fluxer-windows-installer-bytes".to_vec();
        let expected_hash = encode_hex_lower(&Sha256::digest(&payload));
        let sums_body = format!("{expected_hash}  fluxer_client-windows-x86_64.zip\n").into_bytes();

        let (asset_url, asset_srv) = spawn_mock_bytes_server("200 OK", "", payload.clone()).await;
        let (sums_url, sums_srv) = spawn_mock_bytes_server("200 OK", "", sums_body).await;

        let status = download_update_with_options(
            "v0.2.0",
            "fluxer_client-windows-x86_64.zip",
            &asset_url,
            Some(&sums_url),
            &temp.0,
            true,
        )
        .await;
        asset_srv.await.expect("asset server");
        sums_srv.await.expect("sums server");

        let final_path = temp.0.join("fluxer_client-windows-x86_64.zip");
        let part_path = temp.0.join("fluxer_client-windows-x86_64.zip.part");
        assert_eq!(
            status,
            UpdateStatus::Downloaded {
                latest: "0.2.0".to_owned(),
                path: final_path.to_string_lossy().into_owned(),
                sha256: expected_hash,
                verified: true,
            }
        );
        assert_eq!(std::fs::read(&final_path).expect("read downloaded"), payload);
        assert!(!part_path.exists());
    }

    #[tokio::test]
    async fn download_update_without_checksum_marks_unverified() {
        let temp = TempDirGuard::new("unverified");
        let payload = b"fluxer-zip-payload".to_vec();
        let expected_hash = encode_hex_lower(&Sha256::digest(&payload));

        let (asset_url, asset_srv) = spawn_mock_bytes_server("200 OK", "", payload.clone()).await;

        let status = download_update_with_options(
            "0.2.0",
            "fluxer_client-windows-x86_64.zip",
            &asset_url,
            None,
            &temp.0,
            true,
        )
        .await;
        asset_srv.await.expect("asset server");

        let final_path = temp.0.join("fluxer_client-windows-x86_64.zip");
        assert_eq!(
            status,
            UpdateStatus::Downloaded {
                latest: "0.2.0".to_owned(),
                path: final_path.to_string_lossy().into_owned(),
                sha256: expected_hash,
                verified: false,
            }
        );
        assert_eq!(std::fs::read(&final_path).expect("read downloaded"), payload);
    }

    #[tokio::test]
    async fn download_update_deletes_part_on_checksum_mismatch() {
        let temp = TempDirGuard::new("mismatch");
        let payload = b"corrupted-installer-bytes".to_vec();
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let sums_body = format!("{wrong_hash}  fluxer_client-windows-x86_64.exe\n").into_bytes();

        let (asset_url, asset_srv) = spawn_mock_bytes_server("200 OK", "", payload).await;
        let (sums_url, sums_srv) = spawn_mock_bytes_server("200 OK", "", sums_body).await;

        let status = download_update_with_options(
            "0.2.0",
            "fluxer_client-windows-x86_64.exe",
            &asset_url,
            Some(&sums_url),
            &temp.0,
            true,
        )
        .await;
        asset_srv.await.expect("asset server");
        sums_srv.await.expect("sums server");

        assert!(matches!(status, UpdateStatus::Error(ref msg) if msg.contains("mismatch")));
        assert!(!temp.0.join("fluxer_client-windows-x86_64.exe").exists());
        assert!(!temp.0.join("fluxer_client-windows-x86_64.exe.part").exists());
    }

    #[tokio::test]
    async fn download_update_allows_five_redirects_and_rejects_sixth_or_untrusted() {
        let temp = TempDirGuard::new("redirects");
        let payload = b"redirected-payload".to_vec();
        let expected_hash = encode_hex_lower(&Sha256::digest(&payload));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let server_payload = payload.clone();
        let server = tokio::spawn(async move {
            for hop in 0..6 {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                if hop < 5 {
                    let resp = format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://{addr}/hop/{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        hop + 1
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                } else {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        server_payload.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&server_payload).await;
                }
                let _ = stream.shutdown().await;
            }
        });

        let start_url = format!("http://{addr}/hop/0");
        let status = download_update_with_options(
            "0.2.0",
            "fluxer_client.zip",
            &start_url,
            None,
            &temp.0,
            true,
        )
        .await;
        server.await.expect("5-redirect server");
        assert_eq!(
            status,
            UpdateStatus::Downloaded {
                latest: "0.2.0".to_owned(),
                path: temp.0.join("fluxer_client.zip").to_string_lossy().into_owned(),
                sha256: expected_hash,
                verified: false,
            }
        );

        // 6th redirect must be rejected
        let listener6 = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr6 = listener6.local_addr().expect("local addr");
        let server6 = tokio::spawn(async move {
            for hop in 0..6 {
                let Ok((mut stream, _)) = listener6.accept().await else {
                    break;
                };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{addr6}/hop/{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    hop + 1
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        let status6 = download_update_with_options(
            "0.2.0",
            "fluxer_client_6.zip",
            &format!("http://{addr6}/hop/0"),
            None,
            &temp.0,
            true,
        )
        .await;
        server6.await.expect("6-redirect server");
        assert!(matches!(status6, UpdateStatus::Error(ref msg) if msg.contains("Too many redirects")));

        // Redirect to untrusted host must be rejected
        let (bad_redirect_url, bad_srv) = spawn_mock_bytes_server(
            "302 Found",
            "Location: https://evil.example.com/payload.zip\r\n",
            Vec::new(),
        )
        .await;
        let bad_status = download_update_with_options(
            "0.2.0",
            "fluxer_client_bad.zip",
            &bad_redirect_url,
            None,
            &temp.0,
            true,
        )
        .await;
        bad_srv.await.expect("bad redirect server");
        assert!(matches!(bad_status, UpdateStatus::Error(ref msg) if msg.contains("Untrusted asset host")));
    }

    #[tokio::test]
    async fn download_update_rejects_oversized_content_length_without_leaving_part() {
        let temp = TempDirGuard::new("oversized");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let oversized = MAX_ASSET_BYTES + 1;
        let server = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {oversized}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        let status = download_update_with_options(
            "0.2.0",
            "fluxer_client.zip",
            &format!("http://{addr}/asset"),
            None,
            &temp.0,
            true,
        )
        .await;
        server.await.expect("oversized server");

        assert!(matches!(status, UpdateStatus::Error(ref msg) if msg.contains("128 MiB")));
        assert!(!temp.0.join("fluxer_client.zip").exists());
        assert!(!temp.0.join("fluxer_client.zip.part").exists());
    }
}
