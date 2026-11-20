//! Messages with attachments: `POST /channels/{c}/messages` as multipart
//! (`RestClient::send_message_with_files`) with an optimistic pending row
//! and progress events.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use fluxer_client_core::rest::{
    MessageReference, RestError, SendMessageRequest, UploadFile, UploadOptions, UploadProgress,
};
use fluxer_client_core::state::{Message, Snowflake};
use serde_json::json;

use super::{Backend, UiEvent, UploadFailure, lock};

/// One multipart send as the test log records it.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct SentUpload {
    pub channel_id: Snowflake,
    pub request: SendMessageRequest,
    pub files: Vec<UploadFile>,
    pub max_file_size: u64,
}

/// Reply target of a message with files: (message, ping the author).
pub type ReplyTo = (Snowflake, bool);

impl Backend {
    /// Optimistic send of `content` plus `files` (1..=10): a pending row
    /// now ("Uploading… N%" while `UiEvent::UploadProgress` arrives), then
    /// the multipart POST. Returns the pending row's nonce; `None` before
    /// READY (no current user). `UiEvent::UploadEnded` reports the end
    /// either way, including failure after the message window was reset.
    pub fn send_with_files(
        &self,
        channel_id: Snowflake,
        content: String,
        files: Vec<UploadFile>,
        reply: Option<ReplyTo>,
        max_file_size: u64,
    ) -> Option<Snowflake> {
        let nonce = self.next_nonce();
        let (guild_id, author_id) = {
            let mut state = self.state();
            if !state.can_attach_files(channel_id) {
                return None;
            }
            let author = state.current_user.clone()?;
            let author_id = author.id;
            let placeholders = files
                .iter()
                .enumerate()
                .map(|(i, f)| fluxer_client_core::state::Attachment {
                    id: Snowflake(i as u64),
                    filename: f.filename.clone(),
                    title: None,
                    description: None,
                    content_type: f.content_type.clone(),
                    size: 0,
                    url: None,
                    proxy_url: None,
                    width: None,
                    height: None,
                    placeholder: None,
                    flags: f.flags,
                    duration: None,
                })
                .collect();
            state.messages.insert_pending_full(
                channel_id,
                nonce,
                content.clone(),
                author,
                reply,
                placeholders,
            );
            (state.channels.get(&channel_id).and_then(|c| c.guild_id), author_id)
        };
        let request = SendMessageRequest {
            content: (!content.is_empty()).then_some(content),
            nonce: nonce.to_string(),
            message_reference: reply.map(|(message_id, _)| MessageReference {
                message_id: message_id.to_string(),
                channel_id: Some(channel_id.to_string()),
                guild_id: guild_id.map(|id| id.to_string()),
                kind: Some(0),
            }),
            allowed_mentions: reply.map(|(_, mention)| json!({ "replied_user": mention })),
            flags: 0,
        };
        #[cfg(test)]
        super::lock_sent(&self.sent_uploads).push(SentUpload {
            channel_id,
            request: request.clone(),
            files: files.clone(),
            max_file_size,
        });
        let _ = self.events.send(UiEvent::UploadProgress { nonce, percent: 0 });
        let _ = self.events.send(UiEvent::StateChanged);
        let (rest, state, events) = (self.rest.clone(), self.state.clone(), self.events.clone());
        let options = UploadOptions { max_file_size, progress: Some(progress_events(nonce, events.clone())) };
        self.runtime.spawn(async move {
            let result = rest.send_message_with_files(&channel_id.to_string(), &request, &files, &options).await;
            let failure = match result {
                Ok(body) => {
                    match serde_json::from_value::<Message>(body) {
                        Ok(message) => {
                            lock(&state).messages.apply_incoming(message);
                        }
                        Err(err) => tracing::warn!(%err, "upload response did not parse"),
                    }
                    None
                }
                Err(err) => {
                    let mut state = lock(&state);
                    if state.messages.has_confirmed_nonce(channel_id, author_id, nonce) {
                        None
                    } else {
                        state.messages.mark_failed(nonce);
                        let uncertain = matches!(
                            &err,
                            RestError::Transport(_) | RestError::Decode(_) | RestError::ResponseTooLarge { .. }
                        ) || err.status().is_some_and(|status| status >= 500);
                        Some(UploadFailure { message: upload_error(&err, &files), uncertain })
                    }
                }
            };
            let _ = events.send(UiEvent::UploadEnded { nonce, error: failure });
        });
        Some(nonce)
    }
}

/// The pending row's text: the message, then a `📎 name` line per file.
#[allow(dead_code)]
fn pending_text(content: &str, files: &[UploadFile]) -> String {
    let mut text = content.to_owned();
    for file in files {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("📎 ");
        text.push_str(&file.filename);
    }
    text
}

/// Sends `UploadProgress` when the whole percent changes.
fn progress_events(nonce: Snowflake, events: tokio::sync::mpsc::UnboundedSender<UiEvent>) -> UploadProgress {
    let last = AtomicU8::new(0);
    Arc::new(move |sent: u64, total: u64| {
        let percent = if total == 0 { 100 } else { u8::try_from(sent.saturating_mul(100) / total).unwrap_or(100) };
        if last.swap(percent, Ordering::Relaxed) != percent {
            let _ = events.send(UiEvent::UploadProgress { nonce, percent });
        }
    })
}

/// "file.png is too large (30.0 MB; the limit is 25.0 MB)" for the size
/// error, the REST error text otherwise.
fn upload_error(err: &RestError, files: &[UploadFile]) -> String {
    match err {
        RestError::AttachmentTooLarge { index, size, limit } => {
            let name = files.get(*index).map_or("A file", |f| f.filename.as_str());
            format!(
                "{name} is too large ({}; the limit is {})",
                crate::ui::attachments::format_size(*size),
                crate::ui::attachments::format_size(*limit)
            )
        }
        other => other.to_string(),
    }
}
