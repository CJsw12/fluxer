//! Message search endpoint (`POST /search/messages`).
//! Route: `fluxer_api/src/api/search/controllers/SearchController.ts`;
//! schemas: `packages/schema/src/domains/message/MessageRequestSchemas.ts`
//! and `MessageResponseSchemas.ts`.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;
use crate::state::{Channel, Message};

const MAX_CONTENT_CHARS: usize = 1024;
const MAX_HITS_PER_PAGE: u16 = 25;
const MAX_PAGE: u16 = 400;
const MAX_CURSOR_LEN: usize = 16;
const MAX_CHANNEL_IDS: usize = 500;
const MAX_AUTHOR_IDS: usize = 100;
const MAX_HAS_FILTERS: usize = 20;

/// Global search scope (`GlobalSearchMessagesRequest.scope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSearchScope {
    Current,
    OpenDms,
    AllDms,
    AllGuilds,
    All,
    OpenDmsAndAllGuilds,
}

/// Sort field (`MessageSearchRequest.sort_by`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSearchSortBy {
    Timestamp,
    Relevance,
}

/// Sort direction (`MessageSearchRequest.sort_order`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSearchSortOrder {
    Asc,
    Desc,
}

/// Attachment/content filter (`MessageSearchRequest.has`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageContentType {
    Image,
    Sound,
    Video,
    File,
    Sticker,
    Embed,
    Link,
    Poll,
    Snapshot,
}

/// Request body for `POST /search/messages` (`GlobalSearchMessagesRequest`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MessageSearchRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits_per_page: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has: Option<Vec<MessageContentType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<MessageSearchSortBy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<MessageSearchSortOrder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_nsfw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<MessageSearchScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_guild_id: Option<String>,
}

/// Decoded results page from `POST /search/messages`.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageSearchPage {
    pub messages: Vec<Message>,
    pub channels: Vec<Channel>,
    pub threads: Vec<Channel>,
    pub total: u64,
    pub hits_per_page: u32,
    pub page: u32,
    pub cursor: Option<Vec<String>>,
}

/// Union returned by `POST /search/messages`: either search index warming
/// (`{ "indexing": true }`) or a page of hits.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageSearchOutcome {
    Indexing,
    Results(MessageSearchPage),
}

fn check_snowflake_list(ids: &[String], max: usize, what: &'static str) -> Result<(), RestError> {
    if ids.len() > max {
        return Err(RestError::InvalidArgument(what));
    }
    for id in ids {
        check_snowflake(id, what)?;
    }
    Ok(())
}

/// Validates schema bounds on `MessageSearchRequest` before sending REST I/O.
pub fn validate_search_request(req: &MessageSearchRequest) -> Result<(), RestError> {
    if let Some(hits) = req.hits_per_page
        && !(1..=MAX_HITS_PER_PAGE).contains(&hits)
    {
        return Err(RestError::InvalidArgument("hits_per_page"));
    }
    if let Some(page) = req.page
        && !(1..=MAX_PAGE).contains(&page)
    {
        return Err(RestError::InvalidArgument("page"));
    }
    if let Some(cursor) = &req.cursor
        && cursor.len() > MAX_CURSOR_LEN
    {
        return Err(RestError::InvalidArgument("cursor"));
    }
    if let Some(content) = &req.content {
        let len = content.encode_utf16().count();
        if len == 0 || len > MAX_CONTENT_CHARS {
            return Err(RestError::InvalidArgument("content"));
        }
    }
    if let Some(ids) = &req.channel_id {
        check_snowflake_list(ids, MAX_CHANNEL_IDS, "channel_id")?;
    }
    if let Some(ids) = &req.channel_ids {
        check_snowflake_list(ids, MAX_CHANNEL_IDS, "channel_ids")?;
    }
    if let Some(ids) = &req.author_id {
        check_snowflake_list(ids, MAX_AUTHOR_IDS, "author_id")?;
    }
    if let Some(has) = &req.has
        && has.len() > MAX_HAS_FILTERS
    {
        return Err(RestError::InvalidArgument("has"));
    }
    if let Some(id) = &req.context_channel_id {
        check_snowflake(id, "context_channel_id")?;
    }
    if let Some(id) = &req.context_guild_id {
        check_snowflake(id, "context_guild_id")?;
    }
    Ok(())
}

/// Parses `MessageSearchResponse` (`MessageSearchIndexingResponse | MessageSearchResultsResponse`).
pub fn parse_message_search_response(body: &Value) -> Result<MessageSearchOutcome, RestError> {
    let obj = body
        .as_object()
        .ok_or_else(|| RestError::Decode("expected message search response object".to_owned()))?;
    if obj.get("indexing").and_then(Value::as_bool) == Some(true) {
        return Ok(MessageSearchOutcome::Indexing);
    }

    let raw_messages = obj
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| RestError::Decode("missing messages array in search response".to_owned()))?;
    let messages: Vec<Message> = raw_messages
        .iter()
        .filter_map(|item| match serde_json::from_value::<Message>(item.clone()) {
            Ok(message) => Some(message),
            Err(err) => {
                tracing::debug!(%err, "skipping malformed search message");
                None
            }
        })
        .collect();

    let channels = decode_channels(obj.get("channels"), "search channel");
    let threads = decode_channels(obj.get("threads"), "search thread");

    let total = obj
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| u64::try_from(messages.len()).unwrap_or(u64::MAX));
    let hits_per_page = obj
        .get("hits_per_page")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(u32::from(MAX_HITS_PER_PAGE));
    let page = obj.get("page").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(1);
    let cursor = obj.get("cursor").and_then(Value::as_array).map(|items| {
        items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect::<Vec<_>>()
    });

    Ok(MessageSearchOutcome::Results(MessageSearchPage {
        messages,
        channels,
        threads,
        total,
        hits_per_page,
        page,
        cursor,
    }))
}

fn decode_channels(raw: Option<&Value>, label: &'static str) -> Vec<Channel> {
    raw.and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match serde_json::from_value::<Channel>(item.clone()) {
                    Ok(channel) => Some(channel),
                    Err(err) => {
                        tracing::debug!(%err, kind = label, "skipping malformed search channel");
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

impl RestClient {
    /// `POST /search/messages` -> `MessageSearchOutcome`.
    pub async fn search_messages(&self, request: &MessageSearchRequest) -> Result<MessageSearchOutcome, RestError> {
        validate_search_request(request)?;
        let body = serde_json::to_value(request).map_err(|err| RestError::Decode(err.to_string()))?;
        let response = self.request(Method::POST, "/search/messages", &[], Some(&body), Auth::Required).await?;
        parse_message_search_response(&response)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_search_request_bounds() {
        assert!(validate_search_request(&MessageSearchRequest::default()).is_ok());

        let bad_hits = MessageSearchRequest { hits_per_page: Some(26), ..Default::default() };
        assert!(matches!(validate_search_request(&bad_hits), Err(RestError::InvalidArgument("hits_per_page"))));

        let zero_hits = MessageSearchRequest { hits_per_page: Some(0), ..Default::default() };
        assert!(matches!(validate_search_request(&zero_hits), Err(RestError::InvalidArgument("hits_per_page"))));

        let bad_page = MessageSearchRequest { page: Some(401), ..Default::default() };
        assert!(matches!(validate_search_request(&bad_page), Err(RestError::InvalidArgument("page"))));

        let empty_content = MessageSearchRequest { content: Some(String::new()), ..Default::default() };
        assert!(matches!(validate_search_request(&empty_content), Err(RestError::InvalidArgument("content"))));

        let long_content = MessageSearchRequest { content: Some("a".repeat(1025)), ..Default::default() };
        assert!(matches!(validate_search_request(&long_content), Err(RestError::InvalidArgument("content"))));

        // Surrogate-pair emoji takes 2 UTF-16 code units: 513 * 2 = 1026 > 1024
        let long_emoji = MessageSearchRequest { content: Some("😀".repeat(513)), ..Default::default() };
        assert!(matches!(validate_search_request(&long_emoji), Err(RestError::InvalidArgument("content"))));

        let bad_ctx = MessageSearchRequest { context_channel_id: Some("1/../2".into()), ..Default::default() };
        assert!(matches!(
            validate_search_request(&bad_ctx),
            Err(RestError::InvalidArgument("context_channel_id"))
        ));

        let bad_author = MessageSearchRequest { author_id: Some(vec!["not-snowflake".into()]), ..Default::default() };
        assert!(matches!(validate_search_request(&bad_author), Err(RestError::InvalidArgument("author_id"))));
    }

    #[test]
    fn parses_indexing_and_results_responses() {
        let indexing = json!({ "indexing": true });
        assert_eq!(parse_message_search_response(&indexing).unwrap(), MessageSearchOutcome::Indexing);

        let results = json!({
            "messages": [
                {
                    "id": "900000000000000010",
                    "channel_id": "200000000000000001",
                    "author": { "id": "300000000000000001", "username": "alice" },
                    "content": "hello search",
                    "timestamp": "2026-11-19T00:00:00.000Z"
                },
                { "malformed": true }
            ],
            "channels": [
                {
                    "id": "200000000000000001",
                    "guild_id": "100000000000000001",
                    "name": "general",
                    "type": 0
                }
            ],
            "threads": [
                {
                    "id": "200000000000000099",
                    "guild_id": "100000000000000001",
                    "name": "thread-one",
                    "type": 11
                }
            ],
            "total": 42,
            "hits_per_page": 25,
            "page": 1,
            "cursor": ["tok-1"]
        });
        let outcome = parse_message_search_response(&results).unwrap();
        let MessageSearchOutcome::Results(page) = outcome else { panic!("expected Results") };
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].content, "hello search");
        assert_eq!(page.channels.len(), 1);
        assert_eq!(page.channels[0].name.as_deref(), Some("general"));
        assert_eq!(page.threads.len(), 1);
        assert_eq!(page.threads[0].name.as_deref(), Some("thread-one"));
        assert_eq!(page.total, 42);
        assert_eq!(page.hits_per_page, 25);
        assert_eq!(page.page, 1);
        assert_eq!(page.cursor, Some(vec!["tok-1".to_owned()]));

        assert!(matches!(parse_message_search_response(&json!("bad")), Err(RestError::Decode(_))));
    }
}
