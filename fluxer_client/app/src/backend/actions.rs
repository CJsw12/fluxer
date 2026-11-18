//! Message interactions on top of [`Backend`]: reactions, replies, edits,
//! deletes and the typing signal. Each action updates the local state first
//! (so the UI redraws at once), then calls REST on the tokio runtime and
//! rolls the local change back when the call fails. Results reach the UI as
//! `UiEvent::StateChanged`; failures as `UiEvent::ActionFailed` with a message
//! naming the action.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{
    EditMessageRequest, MessageReference, ReactionEmoji, RestError, SendMessageRequest, TypingPlan, TypingThrottle,
};
use fluxer_client_core::state::{ClientState, MessageState, PartialEmoji, Snowflake, toggled_suppress_embeds};
use serde_json::{Value, json};

use super::{Backend, UiEvent, lock};

/// Paces `POST /channels/{c}/typing` for one composer
/// (`TypingSendThrottle.ts`). Clones share the same slot. The composer
/// owner keeps one and passes it to [`Backend::typing_started`].
#[derive(Debug, Clone, Default)]
pub struct TypingSender {
    throttle: Arc<Mutex<TypingThrottle>>,
}

impl TypingSender {
    pub fn new() -> Self {
        Self::default()
    }

    fn throttle(&self) -> MutexGuard<'_, TypingThrottle> {
        self.throttle.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The composer was cleared: a scheduled send for `channel_id` is dropped.
    pub fn stopped(&self, channel_id: Snowflake) {
        self.throttle().stop(&channel_id.to_string());
    }

    /// Our message went out: the next keystroke starts a new slot.
    pub fn message_sent(&self) {
        self.throttle().message_sent();
    }
}

/// Whether toggling `emoji` on the message adds it (`None`: the message is
/// not loaded or not sent yet). `ReactionCommands`: a reaction the user
/// already has is removed, anything else is added.
pub fn reaction_adds(state: &ClientState, channel_id: Snowflake, message_id: Snowflake, emoji: &PartialEmoji) -> Option<bool> {
    let message = state.messages.message(channel_id, message_id)?;
    if message.state != MessageState::Sent {
        return None;
    }
    Some(!message.reactions.iter().any(|r| r.emoji == *emoji && r.me))
}

/// The REST path form of a state emoji.
pub fn reaction_emoji(emoji: &PartialEmoji) -> ReactionEmoji {
    match emoji.id {
        Some(id) => ReactionEmoji::custom(emoji.name.clone(), id.to_string()),
        None => ReactionEmoji::unicode(emoji.name.clone()),
    }
}

/// A dispatch built locally, applied through the same reducer as the
/// gateway's (`MESSAGE_UPDATE` merge, `MESSAGE_DELETE` removal).
fn local_dispatch(t: &str, d: Value) -> Dispatch {
    Dispatch { t: t.to_owned(), s: 0, d }
}

fn content_patch(channel_id: Snowflake, message_id: Snowflake, content: &str) -> Dispatch {
    local_dispatch(
        "MESSAGE_UPDATE",
        json!({ "id": message_id.to_string(), "channel_id": channel_id.to_string(), "content": content }),
    )
}

impl Backend {
    /// Adds or removes the current user's `emoji` on a message: the pill
    /// changes now, then `PUT`/`DELETE .../reactions/{emoji}/@me`; a failed
    /// call reverts the pill. Returns false when the message is not loaded
    /// (or still pending), so nothing was done.
    pub fn toggle_reaction(&self, channel_id: Snowflake, message_id: Snowflake, emoji: PartialEmoji) -> bool {
        let (add, session_id) = {
            let mut state = self.state();
            let Some(add) = reaction_adds(&state, channel_id, message_id, &emoji) else { return false };
            state.apply_local_reaction(channel_id, message_id, emoji.clone(), add);
            (add, state.session_id.clone())
        };
        let _ = self.events.send(UiEvent::StateChanged);
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let wire = reaction_emoji(&emoji);
            let (channel, message) = (channel_id.to_string(), message_id.to_string());
            let session = session_id.as_deref();
            let result = if add {
                rest.add_reaction(&channel, &message, &wire, session).await
            } else {
                rest.remove_own_reaction(&channel, &message, &wire, session).await
            };
            if let Err(err) = result {
                lock(&state).apply_local_reaction(channel_id, message_id, emoji, !add);
                let verb = if add { "add" } else { "remove" };
                let _ = events.send(UiEvent::ActionFailed { error: format!("Could not {verb} the reaction: {err}") });
                let _ = events.send(UiEvent::StateChanged);
            }
        });
        true
    }

    /// Optimistic reply: a pending row now, then `POST .../messages` with a
    /// `message_reference` to `reply_to` and `allowed_mentions.replied_user
    /// = mention` (`useMessageSubmission.tsx`). The pending row has no reply
    /// header; the server message that replaces it does. Returns false
    /// before READY (no current user).
    pub fn send_reply(&self, channel_id: Snowflake, content: String, reply_to: Snowflake, mention: bool) -> bool {
        let nonce = self.next_nonce();
        let guild_id = {
            let mut state = self.state();
            let Some(author) = state.current_user.clone() else { return false };
            state.messages.insert_pending(channel_id, nonce, content.clone(), author);
            state.channels.get(&channel_id).and_then(|c| c.guild_id)
        };
        let request = SendMessageRequest {
            content: Some(content),
            nonce: nonce.to_string(),
            message_reference: Some(MessageReference {
                message_id: reply_to.to_string(),
                channel_id: Some(channel_id.to_string()),
                guild_id: guild_id.map(|id| id.to_string()),
                kind: Some(0),
            }),
            allowed_mentions: Some(json!({ "replied_user": mention })),
            flags: 0,
        };
        self.post_message(channel_id, nonce, request, |err| UiEvent::ActionFailed { error: format!("Reply not sent: {err}") });
        true
    }

    /// Optimistic edit: the new content shows now, then `PATCH
    /// .../messages/{id}`; the response replaces the row, a failure restores
    /// the old content. Returns false when the message is not loaded or the
    /// content is unchanged (nothing sent).
    pub fn edit_message(&self, channel_id: Snowflake, message_id: Snowflake, content: String) -> bool {
        let previous = {
            let mut state = self.state();
            let Some(message) = state.messages.message(channel_id, message_id) else { return false };
            if message.content == content || message.state != MessageState::Sent {
                return false;
            }
            let previous = message.content.clone();
            state.apply(&content_patch(channel_id, message_id, &content));
            previous
        };
        let _ = self.events.send(UiEvent::StateChanged);
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let edit = EditMessageRequest { content: Some(content), ..EditMessageRequest::default() };
            let result = rest.edit_message(&channel_id.to_string(), &message_id.to_string(), &edit).await;
            match result {
                Ok(body) if body.get("id").is_some() && body.get("channel_id").is_some() => {
                    lock(&state).apply(&local_dispatch("MESSAGE_UPDATE", body));
                }
                // Edited; MESSAGE_UPDATE carries the server's copy.
                Ok(_) => {}
                Err(err) => {
                    lock(&state).apply(&content_patch(channel_id, message_id, &previous));
                    let _ = events.send(UiEvent::ActionFailed { error: format!("Could not edit the message: {err}") });
                }
            }
            let _ = events.send(UiEvent::StateChanged);
        });
        true
    }

    /// Deletes a message. A pending or failed local message is only dropped
    /// locally. A sent one is removed after `DELETE .../messages/{id}`
    /// succeeds (not before: the state has no way to put a removed message
    /// back in place without touching the window's paging flags).
    /// Flips SUPPRESS_EMBEDS on a sent message (`toggleSuppressEmbeds`):
    /// `PATCH {flags}`, then the returned message (or the gateway's
    /// MESSAGE_UPDATE) updates the state. Nothing changes locally before the
    /// server agrees. Returns false when the message is not loaded or sent.
    pub fn toggle_suppress_embeds(&self, channel_id: Snowflake, message_id: Snowflake) -> bool {
        let flags = {
            let state = self.state();
            let Some(message) = state.messages.message(channel_id, message_id) else { return false };
            if message.state != MessageState::Sent {
                return false;
            }
            toggled_suppress_embeds(message.flags)
        };
        let Ok(flags) = u32::try_from(flags) else {
            let _ = self.events.send(UiEvent::ActionFailed { error: "Message flags are out of range".to_owned() });
            return false;
        };
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.set_message_flags(&channel_id.to_string(), &message_id.to_string(), flags).await {
                Ok(body) if body.get("id").is_some() && body.get("channel_id").is_some() => {
                    lock(&state).apply(&local_dispatch("MESSAGE_UPDATE", body));
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = events.send(UiEvent::ActionFailed { error: format!("Could not change the embeds: {err}") });
                }
            }
            let _ = events.send(UiEvent::StateChanged);
        });
        true
    }

    pub fn delete_message(&self, channel_id: Snowflake, message_id: Snowflake) {
        {
            let mut state = self.state();
            let local = state.messages.message(channel_id, message_id).is_some_and(|m| m.state != MessageState::Sent);
            if local {
                if state.messages.remove_pending(channel_id, message_id) {
                    drop(state);
                    let _ = self.events.send(UiEvent::StateChanged);
                }
                return;
            }
        }
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.delete_message(&channel_id.to_string(), &message_id.to_string()).await {
                Ok(()) => {
                    let delete = json!({ "id": message_id.to_string(), "channel_id": channel_id.to_string() });
                    lock(&state).apply(&local_dispatch("MESSAGE_DELETE", delete));
                }
                Err(err) => {
                    let _ = events.send(UiEvent::ActionFailed { error: format!("Could not delete the message: {err}") });
                }
            }
            let _ = events.send(UiEvent::StateChanged);
        });
    }

    /// The composer changed in `channel_id`: schedules `POST
    /// /channels/{c}/typing` as [`TypingThrottle`] decides (1.5 s delay,
    /// at most once per 8 s). A failed call is only logged.
    pub fn typing_started(&self, typing: &TypingSender, channel_id: Snowflake) {
        let channel = channel_id.to_string();
        let plan = typing.throttle().start(&channel, Instant::now());
        let TypingPlan::SendAfter(delay) = plan else { return };
        let rest = self.rest.clone();
        let typing = typing.clone();
        self.runtime.spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            if !typing.throttle().take_due(&channel) {
                return;
            }
            if let Err(err) = rest.trigger_typing(&channel).await {
                log_typing_error(&err);
            }
        });
    }
}

fn log_typing_error(err: &RestError) {
    tracing::debug!(%err, "typing indicator not sent");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fluxer_client_core::rest::Endpoints;
    use fluxer_client_core::state::{LoadMode, Message};
    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;
    use crate::backend::instance_from_endpoints;

    const CHANNEL: Snowflake = Snowflake(11);

    fn message_id(n: u64) -> Snowflake {
        Snowflake::from_unix_ms(1_763_000_000_000 + n * 1000)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime")
    }

    /// A backend whose REST client has no token: every call fails at once
    /// with `NotAuthenticated`, without touching the network.
    fn backend(rt: &tokio::runtime::Runtime) -> (Backend, UnboundedReceiver<UiEvent>) {
        let endpoints = Endpoints {
            api_code_version: 1,
            api: "https://fluxer.invalid/api".to_owned(),
            gateway: "wss://fluxer.invalid/gateway".to_owned(),
            media: None,
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: None,
            discovery_url: "https://fluxer.invalid/.well-known/fluxer".to_owned(),
            document: json!({}),
        };
        let instance = instance_from_endpoints(endpoints, false).expect("rest client");
        let (backend, events) = Backend::new(rt.handle().clone(), instance.rest, None);
        {
            let mut state = backend.state();
            state.apply(&local_dispatch(
                "READY",
                json!({ "session_id": "session-1", "user": { "id": "100", "username": "me" },
                        "guilds": [], "private_channels": [], "read_states": [] }),
            ));
            let page: Vec<Message> = [(2, "200", "second"), (1, "100", "first")]
                .iter()
                .map(|(n, author, content)| {
                    serde_json::from_value(json!({
                        "id": message_id(*n).to_string(),
                        "channel_id": CHANNEL.to_string(),
                        "author": { "id": author, "username": format!("u{author}") },
                        "content": content,
                        "timestamp": "2026-11-18T10:20:30.000000+00:00",
                        "reactions": [{ "emoji": { "id": null, "name": "👍" }, "count": 2, "me": false }]
                    }))
                    .expect("message fixture")
                })
                .collect();
            state.messages.load(CHANNEL, page, LoadMode::Replace);
        }
        (backend, events)
    }

    fn thumbs() -> PartialEmoji {
        PartialEmoji { id: None, name: "👍".to_owned(), animated: false }
    }

    fn reaction(backend: &Backend, n: u64) -> Option<(u32, bool)> {
        let state = backend.state();
        let message = state.messages.message(CHANNEL, message_id(n))?;
        message.reactions.iter().find(|r| r.emoji == thumbs()).map(|r| (r.count, r.me))
    }

    fn content(backend: &Backend, n: u64) -> Option<String> {
        backend.state().messages.message(CHANNEL, message_id(n)).map(|m| m.content.clone())
    }

    /// Waits for the first `ActionFailed` the spawned task sends.
    fn next_failure(rt: &tokio::runtime::Runtime, events: &mut UnboundedReceiver<UiEvent>) -> String {
        rt.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(event) = events.recv().await {
                    if let UiEvent::ActionFailed { error } = event {
                        return error;
                    }
                }
                String::new()
            })
            .await
            .unwrap_or_default()
        })
    }

    #[test]
    fn reaction_toggle_is_optimistic_and_rolls_back() {
        let rt = runtime();
        let (backend, mut events) = backend(&rt);
        assert!(backend.toggle_reaction(CHANNEL, message_id(2), thumbs()));
        assert_eq!(reaction(&backend, 2), Some((3, true)), "pill updated before the REST call");
        assert_eq!(events.try_recv().ok(), Some(UiEvent::StateChanged));
        let error = next_failure(&rt, &mut events);
        assert!(error.starts_with("Could not add the reaction"), "{error}");
        assert_eq!(reaction(&backend, 2), Some((2, false)), "rolled back after the failure");
    }

    #[test]
    fn removing_own_reaction_rolls_back_too() {
        let rt = runtime();
        let (backend, mut events) = backend(&rt);
        backend.state().apply_local_reaction(CHANNEL, message_id(1), thumbs(), true);
        assert_eq!(reaction(&backend, 1), Some((3, true)));
        assert!(backend.toggle_reaction(CHANNEL, message_id(1), thumbs()));
        assert_eq!(reaction(&backend, 1), Some((2, false)));
        let error = next_failure(&rt, &mut events);
        assert!(error.starts_with("Could not remove the reaction"), "{error}");
        assert_eq!(reaction(&backend, 1), Some((3, true)));
    }

    #[test]
    fn reaction_on_unknown_message_does_nothing() {
        let rt = runtime();
        let (backend, mut events) = backend(&rt);
        assert!(!backend.toggle_reaction(CHANNEL, message_id(9), thumbs()));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn custom_emoji_uses_name_and_id() {
        let custom = PartialEmoji { id: Some(Snowflake(55)), name: "blob".to_owned(), animated: true };
        assert_eq!(reaction_emoji(&custom), ReactionEmoji::custom("blob", "55"));
        assert_eq!(reaction_emoji(&thumbs()), ReactionEmoji::unicode("👍"));
    }

    #[test]
    fn edit_is_optimistic_and_restores_on_failure() {
        let rt = runtime();
        let (backend, mut events) = backend(&rt);
        assert!(!backend.edit_message(CHANNEL, message_id(1), "first".to_owned()), "unchanged content is not sent");
        assert!(backend.edit_message(CHANNEL, message_id(1), "edited".to_owned()));
        assert_eq!(content(&backend, 1).as_deref(), Some("edited"));
        let error = next_failure(&rt, &mut events);
        assert!(error.starts_with("Could not edit"), "{error}");
        assert_eq!(content(&backend, 1).as_deref(), Some("first"));
    }

    #[test]
    fn failed_delete_keeps_the_message() {
        let rt = runtime();
        let (backend, mut events) = backend(&rt);
        backend.delete_message(CHANNEL, message_id(1));
        let error = next_failure(&rt, &mut events);
        assert!(error.starts_with("Could not delete"), "{error}");
        assert_eq!(content(&backend, 1).as_deref(), Some("first"));
    }

    #[test]
    fn failed_reply_marks_the_pending_row() {
        let rt = runtime();
        let (backend, mut events) = backend(&rt);
        assert!(backend.send_reply(CHANNEL, "re".to_owned(), message_id(2), false));
        let error = next_failure(&rt, &mut events);
        assert!(error.starts_with("Reply not sent"), "{error}");
        let state = backend.state();
        let window = state.messages.window(CHANNEL).expect("window");
        let last = window.messages.last().expect("pending row");
        assert_eq!((last.content.as_str(), last.state), ("re", MessageState::Failed));
        drop(state);
        // A failed local row is deleted without REST.
        let id = backend.state().messages.window(CHANNEL).and_then(|w| w.messages.last()).map(|m| m.id);
        if let Some(id) = id {
            backend.delete_message(CHANNEL, id);
        }
        assert_eq!(backend.state().messages.window(CHANNEL).map(|w| w.messages.len()), Some(2));
    }

    #[test]
    fn typing_sender_throttles_per_channel() {
        let rt = runtime();
        let (backend, _events) = backend(&rt);
        let typing = TypingSender::new();
        backend.typing_started(&typing, CHANNEL);
        // A second keystroke while a send is scheduled is throttled.
        assert_eq!(typing.throttle().start(&CHANNEL.to_string(), Instant::now()), TypingPlan::Throttled);
        typing.stopped(CHANNEL);
        assert!(!typing.throttle().take_due(&CHANNEL.to_string()), "stopped before it fired");
    }
}
