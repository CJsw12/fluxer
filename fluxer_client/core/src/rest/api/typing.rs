//! `POST /channels/{c}/typing` and the client-side send throttle
//! (`RollingTypingSender.ts`, `TypingSendThrottle.ts`).

use std::time::{Duration, Instant};

use reqwest::Method;
use serde::Deserialize;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// 200 body of `POST /channels/{c}/typing` when slowmode applies
/// (`ChannelService.getTypingCooldown`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct TypingCooldown {
    #[serde(default)]
    pub message_send_cooldown_ms: Option<u64>,
    #[serde(default)]
    pub thread_create_cooldown_ms: Option<u64>,
}

impl RestClient {
    /// `POST /channels/{c}/typing`. `None` on 204; the slowmode cooldowns on
    /// 200. Pace calls with [`TypingThrottle`].
    pub async fn trigger_typing(&self, channel_id: &str) -> Result<Option<TypingCooldown>, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        let path = format!("/channels/{channel_id}/typing");
        let body = self.request(Method::POST, &path, &[], None, Auth::Required).await?;
        if body.is_null() {
            return Ok(None);
        }
        serde_json::from_value(body)
            .map(Some)
            .map_err(|e| RestError::Decode(format!("typing cooldown: {e}")))
    }
}

/// `TypingSendThrottle.ts` delays.
pub const TYPING_SEND_DELAY: Duration = Duration::from_millis(1500);
pub const TYPING_THROTTLE: Duration = Duration::from_secs(8);
pub const TYPING_IMMEDIATE_SEND_IDLE: Duration = Duration::from_secs(16);

/// What to do after a composer keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypingPlan {
    /// A send is already scheduled or one went out under 8 s ago.
    Throttled,
    /// Wait this long, then call [`TypingThrottle::take_due`] and, if it
    /// returns true, [`RestClient::trigger_typing`].
    SendAfter(Duration),
}

/// Port of `planTypingSend` + `RollingTypingSender`: one slot for the
/// channel being typed in. Switching channels drops the old slot. The
/// caller owns the timer; this only decides.
#[derive(Debug, Default)]
pub struct TypingThrottle {
    slot: Option<TypingSlot>,
}

#[derive(Debug)]
struct TypingSlot {
    channel_id: String,
    /// When the last send was scheduled.
    prev_send: Instant,
    /// A scheduled send has not fired yet.
    pending: bool,
}

impl TypingThrottle {
    pub fn new() -> Self {
        Self::default()
    }

    /// The composer changed in `channel_id` at `now`.
    pub fn start(&mut self, channel_id: &str, now: Instant) -> TypingPlan {
        let delay = match &self.slot {
            Some(slot) if slot.channel_id == channel_id => {
                if slot.pending || slot.prev_send + TYPING_THROTTLE > now {
                    return TypingPlan::Throttled;
                }
                if slot.prev_send + TYPING_IMMEDIATE_SEND_IDLE <= now { Duration::ZERO } else { TYPING_SEND_DELAY }
            }
            _ => TYPING_SEND_DELAY,
        };
        self.slot = Some(TypingSlot { channel_id: channel_id.to_owned(), prev_send: now, pending: true });
        TypingPlan::SendAfter(delay)
    }

    /// The scheduled delay elapsed: true if the send should still go out
    /// (not stopped, not superseded by another channel).
    pub fn take_due(&mut self, channel_id: &str) -> bool {
        match &mut self.slot {
            Some(slot) if slot.channel_id == channel_id && slot.pending => {
                slot.pending = false;
                true
            }
            _ => false,
        }
    }

    /// The composer was cleared before the scheduled send fired.
    pub fn stop(&mut self, channel_id: &str) {
        if self.slot.as_ref().is_some_and(|slot| slot.channel_id == channel_id && slot.pending) {
            self.slot = None;
        }
    }

    /// Our own message was sent: the next keystroke starts a new slot.
    pub fn message_sent(&mut self) {
        self.slot = None;
    }
}
