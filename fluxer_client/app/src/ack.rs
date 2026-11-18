//! Read-ack scheduling for the viewed channel: a candidate waits
//! `ACK_DELAY` before it is sent, and no message id is acked twice.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use fluxer_client_core::state::Snowflake;

pub const ACK_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
pub struct AckScheduler {
    /// Newest message id acked (or sent for ack) per channel.
    sent: BTreeMap<Snowflake, Snowflake>,
    /// Waiting candidate: channel, message and when it becomes due.
    pending: Option<(Snowflake, Snowflake, Instant)>,
}

impl AckScheduler {
    /// Offers `message_id` as the newest message seen in `channel_id`.
    /// Returns the delay to wait before calling [`Self::take_due`] when a
    /// new timer is needed, `None` if one is already running or nothing is
    /// to be acked.
    pub fn offer(&mut self, channel_id: Snowflake, message_id: Snowflake, now: Instant) -> Option<Duration> {
        if self.sent.get(&channel_id).is_some_and(|sent| *sent >= message_id) {
            return None;
        }
        match &mut self.pending {
            // Same channel: keep the deadline, ack the newer message then.
            Some((channel, message, _)) if *channel == channel_id => {
                *message = (*message).max(message_id);
                None
            }
            _ => {
                self.pending = Some((channel_id, message_id, now + ACK_DELAY));
                Some(ACK_DELAY)
            }
        }
    }

    /// Drops the candidate (channel switched, scrolled up).
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// The candidate if its delay has passed; it is recorded as sent.
    pub fn take_due(&mut self, now: Instant) -> Option<(Snowflake, Snowflake)> {
        let (channel, message, due) = self.pending?;
        if now < due {
            return None;
        }
        self.pending = None;
        if self.sent.get(&channel).is_some_and(|sent| *sent >= message) {
            return None;
        }
        self.sent.insert(channel, message);
        Some((channel, message))
    }

    /// Time left until the pending candidate is due.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.pending.map(|(_, _, due)| due.saturating_duration_since(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C1: Snowflake = Snowflake(1);
    const C2: Snowflake = Snowflake(2);

    #[test]
    fn waits_for_the_delay_and_takes_the_newest() {
        let t0 = Instant::now();
        let mut s = AckScheduler::default();
        assert_eq!(s.offer(C1, Snowflake(10), t0), Some(ACK_DELAY));
        assert_eq!(s.offer(C1, Snowflake(11), t0 + Duration::from_millis(500)), None, "timer already running");
        assert_eq!(s.take_due(t0 + Duration::from_millis(999)), None);
        assert_eq!(s.remaining(t0 + Duration::from_millis(999)), Some(Duration::from_millis(1)));
        assert_eq!(s.take_due(t0 + ACK_DELAY), Some((C1, Snowflake(11))));
        assert_eq!(s.take_due(t0 + ACK_DELAY * 2), None);
    }

    #[test]
    fn never_acks_a_message_twice() {
        let t0 = Instant::now();
        let mut s = AckScheduler::default();
        s.offer(C1, Snowflake(10), t0);
        assert_eq!(s.take_due(t0 + ACK_DELAY), Some((C1, Snowflake(10))));
        assert_eq!(s.offer(C1, Snowflake(10), t0 + ACK_DELAY), None);
        assert_eq!(s.offer(C1, Snowflake(9), t0 + ACK_DELAY), None);
        assert_eq!(s.take_due(t0 + ACK_DELAY * 3), None);
        assert!(s.offer(C1, Snowflake(12), t0 + ACK_DELAY).is_some());
    }

    #[test]
    fn switching_channel_or_cancel_drops_the_candidate() {
        let t0 = Instant::now();
        let mut s = AckScheduler::default();
        s.offer(C1, Snowflake(10), t0);
        assert!(s.offer(C2, Snowflake(20), t0).is_some(), "new channel restarts the timer");
        assert_eq!(s.take_due(t0 + ACK_DELAY), Some((C2, Snowflake(20))));
        s.offer(C1, Snowflake(10), t0);
        s.cancel();
        assert_eq!(s.take_due(t0 + ACK_DELAY * 2), None);
        assert!(s.offer(C1, Snowflake(10), t0).is_some(), "cancelled ids can be offered again");
    }
}
