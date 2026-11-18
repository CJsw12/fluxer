//! Desktop notifications for incoming messages (`Notification.ts`
//! `handleMessageCreate`, `NotificationUtils.ts`, `SoundUtils.ts`).
//!
//! The decision runs on the tokio side right after a `MESSAGE_CREATE` is
//! applied ([`notification_for`], using the core's `should_notify` with the
//! window focus and viewed channel the UI keeps in [`NotifyFocus`]); the UI
//! gets a [`Notification`] and [`Notifier`] shows the toast and plays the
//! sound through the [`Toaster`] / [`SoundPlayer`] seams (tests use fakes).
//!
//! Not ported: streamer mode, "a modal is open", the mature-content gate,
//! per-sound toggles, clearing shown toasts on focus/select/ack (notify-rust
//! has no remove API on Windows), notification avatars.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use fluxer_client_core::state::{ChannelKind, ClientState, Message, NotificationKind, Snowflake, should_notify};
use serde_json::Value;

use crate::ui::l10n;
use crate::ui::reply::{excerpt, member_name};
use crate::view_model::channel_name;

/// The OS seams; never compiled into tests so none can show a toast or
/// play a sound.
#[cfg(not(test))]
pub mod os;

/// Longest toast body, in characters.
const BODY_CHARS: usize = 200;
/// Message ids remembered so a replayed `MESSAGE_CREATE` does not notify twice.
const RECENT_IDS: usize = 200;

/// Window focus and the channel on screen, written by the UI thread and
/// read by the gateway task for every `MESSAGE_CREATE`.
#[derive(Debug, Default)]
pub struct NotifyFocus {
    focused: AtomicBool,
    /// Viewed channel id; 0 = none.
    viewing: AtomicU64,
}

impl NotifyFocus {
    pub fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Relaxed);
    }

    pub fn set_viewing(&self, channel_id: Option<Snowflake>) {
        self.viewing.store(channel_id.map_or(0, |id| id.0), Ordering::Relaxed);
    }

    pub fn focused(&self) -> bool {
        self.focused.load(Ordering::Relaxed)
    }

    pub fn viewing(&self) -> Option<Snowflake> {
        match self.viewing.load(Ordering::Relaxed) {
            0 => None,
            id => Some(Snowflake(id)),
        }
    }
}

/// What to show for one incoming message.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub kind: NotificationKind,
    pub guild_id: Option<Snowflake>,
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    pub title: String,
    pub body: String,
}

/// Decides on a `MESSAGE_CREATE` payload `d` that was just applied to
/// `state`. `None`: no sound and no toast.
pub fn notification_for(state: &ClientState, d: &Value, focus: &NotifyFocus, now_ms: i64) -> Option<Notification> {
    let message: Message = match serde_json::from_value(d.clone()) {
        Ok(message) => message,
        Err(err) => {
            tracing::debug!(%err, "MESSAGE_CREATE did not parse for notifications");
            return None;
        }
    };
    let kind = should_notify(state, &message, now_ms, focus.focused(), focus.viewing())?;
    let guild_id = state.channels.get(&message.channel_id).and_then(|c| c.guild_id);
    Some(Notification {
        kind,
        guild_id,
        channel_id: message.channel_id,
        message_id: message.id,
        title: toast_title(state, &message),
        body: toast_body(state, guild_id, &message),
    })
}

/// `author #channel (guild)` in guilds, `author (group)` in group DMs and
/// the author alone in DMs. The author is the member nick when known.
pub fn toast_title(state: &ClientState, message: &Message) -> String {
    let Some(channel) = state.channels.get(&message.channel_id) else {
        return message.author.display_name().to_owned();
    };
    let author = member_name(state, channel.guild_id, &message.author);
    match (channel.kind, channel.guild_id) {
        (ChannelKind::Dm, _) => author,
        (ChannelKind::GroupDm, _) => format!("{author} ({})", channel_name(channel)),
        (_, Some(guild_id)) => {
            let name = channel.name.clone().unwrap_or_default();
            match state.guilds.get(&guild_id) {
                Some(guild) => format!("{author} #{name} ({})", guild.name),
                None => format!("{author} #{name}"),
            }
        }
        (_, None) => author,
    }
}

/// One-line plain-text excerpt: mentions, channels, roles and custom
/// emoji in readable form; "Sent an attachment" for a file-only message.
pub fn toast_body(state: &ClientState, guild_id: Option<Snowflake>, message: &Message) -> String {
    let text = plain_text(state, guild_id, message);
    let body = excerpt(&text, BODY_CHARS);
    if !body.is_empty() {
        return body;
    }
    match message.attachments.len() {
        0 if !message.embeds.is_empty() => l10n::tr_args(
            "{length, plural, one {# embed} other {# embeds}}",
            &[("length", &message.embeds.len().to_string())],
        ),
        0 => String::new(),
        1 => l10n::tr("Sent an attachment"),
        n => format!("Sent {n} attachments"),
    }
}

/// `hideSpoilers` of the plain-text renderer: every `||…||` span
/// (non-greedy, may span lines) becomes `█████`, so a spoiler never shows
/// in the OS notification centre.
pub fn hide_spoilers(content: &str) -> String {
    const HIDDEN: &str = "█████";
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(open) = rest.find("||") {
        let body = &rest[open + 2..];
        let Some(close) = body.find("||") else { break };
        out.push_str(&rest[..open]);
        if close == 0 {
            // `||||` is not a spoiler.
            out.push_str("||||");
        } else {
            out.push_str(HIDDEN);
        }
        rest = &body[close + 2..];
    }
    out.push_str(rest);
    out
}

/// `content` with spoilers hidden and `<@id>`, `<@!id>`, `<@&id>`, `<#id>`
/// and `<:name:id>` tokens replaced by `@name`, `@role`, `#channel` and
/// `:name:`.
fn plain_text(state: &ClientState, guild_id: Option<Snowflake>, message: &Message) -> String {
    let hidden = hide_spoilers(&message.content);
    let content = &hidden;
    let mut out = String::with_capacity(content.len());
    let mut rest = content.as_str();
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            out.push_str(&rest[start..]);
            return out;
        };
        match resolve_token(state, guild_id, message, &after[..end]) {
            Some(text) => out.push_str(&text),
            None => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn resolve_token(state: &ClientState, guild_id: Option<Snowflake>, message: &Message, token: &str) -> Option<String> {
    let id = |s: &str| s.parse::<u64>().ok().map(Snowflake);
    if let Some(role) = token.strip_prefix("@&") {
        let role = id(role)?;
        let name = guild_id
            .and_then(|g| state.guilds.get(&g))
            .and_then(|g| g.roles.get(&role))
            .map_or_else(|| l10n::tr("deleted-role"), |r| r.name.clone());
        return Some(format!("@{name}"));
    }
    if let Some(user) = token.strip_prefix('@') {
        let user = id(user.trim_start_matches('!'))?;
        let name = message
            .mentions
            .iter()
            .find(|u| u.id == user)
            .map_or_else(|| l10n::tr("unknown user"), |u| member_name(state, guild_id, u));
        return Some(format!("@{name}"));
    }
    if let Some(channel) = token.strip_prefix('#') {
        let channel = id(channel)?;
        let name = state.channels.get(&channel).map_or_else(|| l10n::tr("deleted-channel"), channel_name);
        return Some(format!("#{name}"));
    }
    // `:name:id` / `a:name:id` custom emoji.
    let emoji = token.strip_prefix("a:").or_else(|| token.strip_prefix(':'))?;
    let (name, emoji_id) = emoji.split_once(':')?;
    id(emoji_id)?;
    Some(format!(":{name}:"))
}

/// The two bundled sounds (`SoundUtils.ts`; DMs reuse the message sound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Message,
    SameChannel,
}

/// A toast to show; clicking it opens `channel_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub guild_id: Option<Snowflake>,
    pub channel_id: Snowflake,
    pub title: String,
    pub body: String,
}

/// Shows desktop toasts (`os::OsToaster` in the app).
pub trait Toaster {
    fn show(&self, toast: Toast);
}

/// Plays a notification sound (`os::RodioPlayer` in the app).
pub trait SoundPlayer {
    fn play(&self, sound: Sound);
}

/// Shows nothing (tests, and before the OS seams are set).
#[cfg(test)]
pub struct Silent;

#[cfg(test)]
impl Toaster for Silent {
    fn show(&self, _: Toast) {}
}

#[cfg(test)]
impl SoundPlayer for Silent {
    fn play(&self, _: Sound) {}
}

/// What [`Notifier::handle`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Disabled, or the message was already notified.
    Nothing,
    SoundOnly,
    Toast,
}

/// Shows notifications through the OS seams, with the in-memory toggle and
/// the notified-id dedupe.
pub struct Notifier {
    toaster: Box<dyn Toaster>,
    sounds: Box<dyn SoundPlayer>,
    /// The header's bell toggle and settings switch: toasts (and sounds when enabled).
    pub enabled: bool,
    /// Separate settings switch gating notification sounds.
    pub sounds_enabled: bool,
    recent: VecDeque<Snowflake>,
}

impl Notifier {
    pub fn new(toaster: Box<dyn Toaster>, sounds: Box<dyn SoundPlayer>) -> Self {
        Self { toaster, sounds, enabled: true, sounds_enabled: true, recent: VecDeque::new() }
    }

    /// Plays the sound and shows the toast `n` asks for.
    pub fn handle(&mut self, n: &Notification) -> Outcome {
        if !self.enabled || self.recent.contains(&n.message_id) {
            return Outcome::Nothing;
        }
        if n.kind == NotificationKind::SameChannelSound && !self.sounds_enabled {
            return Outcome::Nothing;
        }
        if self.recent.len() == RECENT_IDS {
            self.recent.pop_front();
        }
        self.recent.push_back(n.message_id);
        if n.kind == NotificationKind::SameChannelSound {
            self.sounds.play(Sound::SameChannel);
            return Outcome::SoundOnly;
        }
        if self.sounds_enabled {
            self.sounds.play(Sound::Message);
        }
        self.toaster.show(Toast {
            guild_id: n.guild_id,
            channel_id: n.channel_id,
            title: n.title.clone(),
            body: n.body.clone(),
        });
        Outcome::Toast
    }
}

#[cfg(test)]
pub(crate) mod tests;
