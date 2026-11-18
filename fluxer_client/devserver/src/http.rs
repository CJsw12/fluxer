//! Minimal HTTP/1.1 request reader and response writer (keep-alive,
//! `Content-Length` bodies only). Enough for reqwest and a WebSocket upgrade.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const MAX_HEAD_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_HEADERS: usize = 64;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    /// Path without the query string.
    pub path: String,
    pub query: Vec<(String, String)>,
    /// Header names are lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.body).ok()
    }

    pub fn is_websocket_upgrade(&self) -> bool {
        self.method == "GET"
            && self
                .header("upgrade")
                .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
    }

    fn wants_close(&self) -> bool {
        self.header("connection")
            .is_some_and(|v| v.eq_ignore_ascii_case("close"))
    }
}

pub struct Response {
    pub status: u16,
    pub body: Option<Value>,
}

impl Response {
    pub fn json(status: u16, body: Value) -> Self {
        Self { status, body: Some(body) }
    }

    pub fn no_content() -> Self {
        Self { status: 204, body: None }
    }

    /// Fluxer error body `{code, message, ...extra}`.
    pub fn error(status: u16, code: &str, message: &str) -> Self {
        Self::json(status, serde_json::json!({ "code": code, "message": message }))
    }
}

/// Reads one request. `buf` carries bytes left over between keep-alive
/// requests. Returns `None` when the peer closed between requests.
pub async fn read_request(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Result<Option<Request>> {
    loop {
        if !buf.is_empty() {
            let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
            let mut parsed = httparse::Request::new(&mut headers);
            if let httparse::Status::Complete(head_len) = parsed.parse(buf).context("malformed HTTP request")? {
                let method = parsed.method.unwrap_or_default().to_owned();
                let target = parsed.path.unwrap_or_default().to_owned();
                let headers: Vec<(String, String)> = parsed
                    .headers
                    .iter()
                    .map(|h| {
                        (
                            h.name.to_ascii_lowercase(),
                            String::from_utf8_lossy(h.value).trim().to_owned(),
                        )
                    })
                    .collect();
                return finish_request(stream, buf, head_len, method, target, headers).await.map(Some);
            }
            if buf.len() > MAX_HEAD_BYTES {
                bail!("request head too large");
            }
        }
        if read_more(stream, buf).await? == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            bail!("connection closed mid-request");
        }
    }
}

async fn finish_request(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    head_len: usize,
    method: String,
    target: String,
    headers: Vec<(String, String)>,
) -> Result<Request> {
    let find = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    if find("transfer-encoding").is_some() {
        bail!("chunked request bodies are not supported");
    }
    let body_len: usize = match find("content-length") {
        Some(value) => value.parse().context("bad content-length")?,
        None => 0,
    };
    if body_len > MAX_BODY_BYTES {
        bail!("request body too large");
    }
    while buf.len() < head_len + body_len {
        if read_more(stream, buf).await? == 0 {
            bail!("connection closed mid-body");
        }
    }
    let body = buf[head_len..head_len + body_len].to_vec();
    buf.drain(..head_len + body_len);
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (
            path.to_owned(),
            url::form_urlencoded::parse(query.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
        ),
        None => (target, Vec::new()),
    };
    Ok(Request { method, path, query, headers, body })
}

async fn read_more(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Result<usize> {
    let mut chunk = [0u8; 8192];
    let n = stream.read(&mut chunk).await.context("socket read")?;
    buf.extend_from_slice(&chunk[..n]);
    Ok(n)
}

/// Writes `response`; returns whether the connection may be kept open.
pub async fn write_response(stream: &mut TcpStream, request: &Request, response: &Response) -> Result<bool> {
    let body = match &response.body {
        Some(value) => serde_json::to_vec(value).context("encode response")?,
        None => Vec::new(),
    };
    let keep_alive = !request.wants_close();
    let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, reason(response.status));
    if response.status != 204 {
        head.push_str("Content-Type: application/json\r\n");
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str(if keep_alive { "Connection: keep-alive\r\n\r\n" } else { "Connection: close\r\n\r\n" });
    stream.write_all(head.as_bytes()).await.context("socket write")?;
    stream.write_all(&body).await.context("socket write")?;
    stream.flush().await.context("socket flush")?;
    Ok(keep_alive)
}

/// Raw head-only response (used for 101 Switching Protocols).
pub async fn write_raw(stream: &mut TcpStream, head: &str) -> Result<()> {
    stream.write_all(head.as_bytes()).await.context("socket write")?;
    stream.flush().await.context("socket flush")
}

fn reason(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Unknown",
    }
}
