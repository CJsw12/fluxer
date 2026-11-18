//! Message attachments as `multipart/form-data`: a `payload_json` part plus
//! `files[n]` parts (`MessageQueue.tsx::sendMultipartMessage`, server
//! `MessageRequestParser.ts`). This is the default path: presigned uploads
//! (`POST /channels/{c}/attachments`) are used by the web client only when
//! the instance advertises `features.presigned_attachment_uploads`, which
//! the server config defaults to `false` (`ConfigLoader.ts`).
//!
//! File bodies are streamed from disk with a known length, so the request
//! has a `Content-Length` and memory stays flat for large files.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{Stream, StreamExt, stream};
use reqwest::Method;
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio_util::io::ReaderStream;

use super::api::check_snowflake;
use super::client::RestClient;
use super::error::RestError;
use super::api::{SendMessageRequest, message_body};

/// `MAX_ATTACHMENTS_PER_MESSAGE` (`LimitConstants.ts`); the server may
/// lower it per user/guild (`max_attachments_per_message`).
pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;
/// `ATTACHMENT_MAX_SIZE_NON_PREMIUM` (`LimitConstants.ts`): the stock
/// per-file limit. The real limit is the user's `max_attachment_file_size`
/// runtime limit, which this module does not know; pass it in
/// [`UploadOptions::max_file_size`] when available.
pub const DEFAULT_MAX_ATTACHMENT_FILE_SIZE: u64 = 25 * 1024 * 1024;
/// Chunk size for in-memory bodies (progress granularity).
const MEMORY_CHUNK: usize = 64 * 1024;
/// Upload timeout: the request timeout plus this much per byte rate floor.
const UPLOAD_RATE_FLOOR_BYTES_PER_SEC: u64 = 32 * 1024;
const MAX_UPLOAD_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

/// Where an attachment's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadSource {
    /// Streamed from disk; reopened for each retry.
    Path(PathBuf),
    /// In memory (pasted image, generated file).
    Bytes(Bytes),
}

/// One attachment and its `attachments[]` metadata (`ClientAttachmentRequest`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFile {
    pub source: UploadSource,
    pub filename: String,
    /// MIME type; `application/octet-stream` when `None`.
    pub content_type: Option<String>,
    /// Alt text, 1..=4096 chars.
    pub description: Option<String>,
    /// `MessageAttachmentFlags` (e.g. spoiler).
    pub flags: u32,
}

/// `(bytes_sent, bytes_total)` over all files of one request attempt. A
/// retry starts again from 0.
pub type UploadProgress = Arc<dyn Fn(u64, u64) + Send + Sync>;

#[derive(Clone)]
pub struct UploadOptions {
    /// Per-file cap; checked before anything is sent.
    pub max_file_size: u64,
    pub progress: Option<UploadProgress>,
}

impl Default for UploadOptions {
    fn default() -> Self {
        Self { max_file_size: DEFAULT_MAX_ATTACHMENT_FILE_SIZE, progress: None }
    }
}

impl std::fmt::Debug for UploadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UploadOptions")
            .field("max_file_size", &self.max_file_size)
            .field("progress", &self.progress.is_some())
            .finish()
    }
}

/// A validated multipart body that can be rebuilt for every attempt.
pub(crate) struct MultipartUpload {
    payload_json: String,
    parts: Vec<PreparedPart>,
    total: u64,
    progress: Option<UploadProgress>,
}

struct PreparedPart {
    source: UploadSource,
    len: u64,
    filename: String,
    mime: String,
}

impl MultipartUpload {
    /// Builds a fresh form; files are opened here and streamed while sent.
    pub(crate) async fn build_form(&self) -> Result<Form, RestError> {
        let sent = Arc::new(AtomicU64::new(0));
        let mut form = Form::new().percent_encode_noop().text("payload_json", self.payload_json.clone());
        for (index, part) in self.parts.iter().enumerate() {
            let body = match &part.source {
                UploadSource::Path(path) => {
                    let file = tokio::fs::File::open(path).await.map_err(RestError::AttachmentIo)?;
                    self.counted(ReaderStream::new(file.take(part.len)), &sent)
                }
                UploadSource::Bytes(bytes) => {
                    let chunks: Vec<std::io::Result<Bytes>> = (0..bytes.len())
                        .step_by(MEMORY_CHUNK)
                        .map(|start| Ok(bytes.slice(start..bytes.len().min(start + MEMORY_CHUNK))))
                        .collect();
                    self.counted(stream::iter(chunks), &sent)
                }
            };
            let part = Part::stream_with_length(body, part.len)
                .file_name(part.filename.clone())
                .mime_str(&part.mime)
                .map_err(|_| RestError::InvalidArgument("attachment content_type"))?;
            form = form.part(format!("files[{index}]"), part);
        }
        Ok(form)
    }

    fn counted<S>(&self, body: S, sent: &Arc<AtomicU64>) -> reqwest::Body
    where
        S: Stream<Item = std::io::Result<Bytes>> + Send + 'static,
    {
        let Some(progress) = self.progress.clone() else {
            return reqwest::Body::wrap_stream(body);
        };
        let sent = sent.clone();
        let total = self.total;
        reqwest::Body::wrap_stream(body.inspect(move |chunk| {
            if let Ok(chunk) = chunk {
                let len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
                let now = sent.fetch_add(len, Ordering::Relaxed).saturating_add(len);
                progress(now.min(total), total);
            }
        }))
    }
}

impl RestClient {
    /// `POST /channels/{c}/messages` as multipart with 1..=10 attachments.
    /// Every file is size-checked against `options.max_file_size` before
    /// the request starts. Returns the created message.
    pub async fn send_message_with_files(
        &self,
        channel_id: &str,
        message: &SendMessageRequest,
        files: &[UploadFile],
        options: &UploadOptions,
    ) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        if files.is_empty() || files.len() > MAX_ATTACHMENTS_PER_MESSAGE {
            return Err(RestError::InvalidArgument("1..=10 attachments per message"));
        }
        let mut payload = message_body(message)?;
        let mut metadata = Vec::with_capacity(files.len());
        let mut parts = Vec::with_capacity(files.len());
        let mut total: u64 = 0;
        for (index, file) in files.iter().enumerate() {
            check_filename(&file.filename)?;
            if let Some(description) = &file.description {
                let chars = description.chars().count();
                if chars == 0 || chars > 4096 {
                    return Err(RestError::InvalidArgument("attachment description must be 1..=4096 chars"));
                }
            }
            let len = match &file.source {
                UploadSource::Path(path) => {
                    let meta = tokio::fs::metadata(path).await.map_err(RestError::AttachmentIo)?;
                    if !meta.is_file() {
                        return Err(RestError::InvalidArgument("attachment path is not a file"));
                    }
                    meta.len()
                }
                UploadSource::Bytes(bytes) => u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            };
            if len > options.max_file_size {
                return Err(RestError::AttachmentTooLarge { index, size: len, limit: options.max_file_size });
            }
            total = total.saturating_add(len);
            let mime = file.content_type.clone().unwrap_or_else(|| "application/octet-stream".to_owned());
            let mut entry = json!({ "id": index, "filename": file.filename });
            if let Some(content_type) = &file.content_type {
                entry["content_type"] = json!(content_type);
            }
            if let Some(description) = &file.description {
                entry["description"] = json!(description);
            }
            if file.flags != 0 {
                entry["flags"] = json!(file.flags);
            }
            metadata.push(entry);
            parts.push(PreparedPart { source: file.source.clone(), len, filename: file.filename.clone(), mime });
        }
        payload["attachments"] = Value::Array(metadata);
        let payload_json = serde_json::to_string(&payload).map_err(|e| RestError::Decode(e.to_string()))?;
        let upload = MultipartUpload { payload_json, parts, total, progress: options.progress.clone() };
        let path = format!("/channels/{channel_id}/messages");
        self.request_multipart(Method::POST, &path, &upload, self.upload_timeout(total)).await
    }

    /// The request timeout plus time for `total` bytes at a slow-link floor.
    fn upload_timeout(&self, total: u64) -> Duration {
        let transfer = Duration::from_secs(total / UPLOAD_RATE_FLOOR_BYTES_PER_SEC);
        self.request_timeout().saturating_add(transfer).min(MAX_UPLOAD_TIMEOUT)
    }
}

/// `FilenameType`-ish: non-empty, at most 255 chars, no path separators or
/// control characters (the name also lands in `Content-Disposition`).
fn check_filename(name: &str) -> Result<(), RestError> {
    let ok = !name.is_empty()
        && name.chars().count() <= 255
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '"'));
    if ok { Ok(()) } else { Err(RestError::InvalidArgument("attachment filename")) }
}
