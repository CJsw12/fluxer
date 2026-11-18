//! `gpui::http_client::HttpClient` for `img()` (attachment previews and
//! avatars). Requests run on the tokio runtime with reqwest + rustls
//! (certificates verified); plain http is refused unless the instance was
//! opened with `--allow-insecure-loopback` and the host is loopback.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use gpui_kit::http_client::http::HeaderValue;
use gpui_kit::http_client::{AsyncBody, HttpClient, Request, Response, Url};
use tokio::runtime::Handle;

/// Larger bodies are refused (images only).
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

pub struct ImageHttpClient {
    runtime: Handle,
    client: reqwest::Client,
    allow_insecure_loopback: bool,
}

impl ImageHttpClient {
    pub fn new(runtime: Handle, allow_insecure_loopback: bool) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("FluxerRust/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if !url_allowed(attempt.url(), allow_insecure_loopback) {
                    attempt.error("redirect to a disallowed URL")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .context("could not build the image HTTP client")?;
        Ok(Self { runtime, client, allow_insecure_loopback })
    }
}

/// https anywhere; http only for loopback hosts when explicitly allowed.
pub fn url_allowed(url: &Url, allow_insecure_loopback: bool) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => {
            allow_insecure_loopback
                && match url.host() {
                    Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
                    Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                    Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                    None => false,
                }
        }
        _ => false,
    }
}

async fn fetch(client: reqwest::Client, url: Url) -> Result<(u16, Vec<u8>)> {
    let mut response = client.get(url).send().await?;
    let status = response.status().as_u16();
    if response.content_length().is_some_and(|len| len > MAX_BODY_BYTES as u64) {
        bail!("image is larger than {MAX_BODY_BYTES} bytes");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            bail!("image is larger than {MAX_BODY_BYTES} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok((status, body))
}

impl HttpClient for ImageHttpClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(&self, req: Request<AsyncBody>) -> Pin<Box<dyn Future<Output = Result<Response<AsyncBody>>> + Send + 'static>> {
        let client = self.client.clone();
        let runtime = self.runtime.clone();
        let allow_insecure_loopback = self.allow_insecure_loopback;
        Box::pin(async move {
            if req.method() != gpui_kit::http_client::Method::GET {
                bail!("only GET is supported");
            }
            let url = Url::parse(&req.uri().to_string()).context("invalid image URL")?;
            if !url_allowed(&url, allow_insecure_loopback) {
                bail!("refusing to load {}: only https is allowed", url.scheme());
            }
            // reqwest needs the tokio reactor; the JoinHandle can be awaited anywhere.
            let (status, body) = runtime
                .spawn(fetch(client, url))
                .await
                .map_err(|err| anyhow!("image request task failed: {err}"))??;
            Ok(Response::builder().status(status).body(AsyncBody::from(body))?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(s: &str, insecure: bool) -> bool {
        url_allowed(&Url::parse(s).expect("url"), insecure)
    }

    #[test]
    fn only_https_unless_loopback_is_allowed() {
        assert!(allowed("https://media.example.com/a.png", false));
        assert!(!allowed("http://media.example.com/a.png", false));
        assert!(!allowed("http://localhost:8080/a.png", false));
        assert!(allowed("http://localhost:8080/a.png", true));
        assert!(allowed("http://127.0.0.1/a.png", true));
        assert!(allowed("http://[::1]/a.png", true));
        assert!(!allowed("http://example.com/a.png", true));
        assert!(!allowed("file:///C:/a.png", true));
    }
}
