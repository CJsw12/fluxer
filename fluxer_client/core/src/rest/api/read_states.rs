//! Read-state acks (`ReadStates.ts::postAckEntries`).

use reqwest::Method;
use serde_json::{Value, json};

use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;
use super::check_snowflake;

/// `ReadStateAckRequest.read_states` / `ReadStateAckBulkRequest` max length.
const MAX_ACKS_PER_REQUEST: usize = 100;

/// One entry for `POST /read-states/ack`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckRequest {
    pub channel_id: String,
    pub message_id: String,
    pub mention_count: Option<u32>,
    pub manual: bool,
}

impl RestClient {
    /// Marks one message read; see [`Self::ack_messages`].
    pub async fn ack_message(&self, ack: &AckRequest) -> Result<Value, RestError> {
        self.ack_messages(std::slice::from_ref(ack)).await
    }

    /// `POST /read-states/ack` with 1..=100 entries; returns the server's
    /// `{read_states}`. Like `ReadStates.ts::postAckEntries`, a 404/405 falls
    /// back to the legacy endpoints and returns `Value::Null`: automatic acks
    /// go to `POST /read-states/ack-bulk` as `{channel_id, message_id}`,
    /// manual ones to `POST /channels/{c}/messages/{m}/ack` with
    /// `{manual: true, mention_count: mention_count ?? 0}`.
    pub async fn ack_messages(&self, acks: &[AckRequest]) -> Result<Value, RestError> {
        if acks.is_empty() || acks.len() > MAX_ACKS_PER_REQUEST {
            return Err(RestError::InvalidArgument("1..=100 acks per request"));
        }
        let mut entries = Vec::with_capacity(acks.len());
        for ack in acks {
            check_snowflake(&ack.channel_id, "channel_id")?;
            check_snowflake(&ack.message_id, "message_id")?;
            let mut entry = json!({ "channel_id": ack.channel_id, "message_id": ack.message_id });
            if let Some(count) = ack.mention_count {
                entry["mention_count"] = json!(count);
            }
            if ack.manual {
                entry["manual"] = json!(true);
            }
            entries.push(entry);
        }
        let body = json!({ "read_states": entries });
        match self.request(Method::POST, "/read-states/ack", &[], Some(&body), Auth::Required).await {
            Err(RestError::Api(api)) if matches!(api.status, 404 | 405) => {
                self.post_legacy_acks(acks).await?;
                Ok(Value::Null)
            }
            other => other,
        }
    }

    /// `postLegacyAckEntries`. Entries are already validated.
    async fn post_legacy_acks(&self, acks: &[AckRequest]) -> Result<(), RestError> {
        let automatic: Vec<Value> = acks
            .iter()
            .filter(|ack| !ack.manual)
            .map(|ack| json!({ "channel_id": ack.channel_id, "message_id": ack.message_id }))
            .collect();
        if !automatic.is_empty() {
            let body = json!({ "read_states": automatic });
            self.request(Method::POST, "/read-states/ack-bulk", &[], Some(&body), Auth::Required).await?;
        }
        for ack in acks.iter().filter(|ack| ack.manual) {
            let path = format!("/channels/{}/messages/{}/ack", ack.channel_id, ack.message_id);
            let body = json!({ "manual": true, "mention_count": ack.mention_count.unwrap_or(0) });
            self.request(Method::POST, &path, &[], Some(&body), Auth::Required).await?;
        }
        Ok(())
    }
}
