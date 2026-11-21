//! Typed client commands: member-list LAZY_REQUEST (op 14),
//! PRESENCE_UPDATE (op 3), and VOICE_STATE_UPDATE (op 4), shaped like
//! `GatewaySocket.ts` (`updateGuildSubscriptions`, `updatePresence`,
//! `updateVoiceStateExplicit`) and `MemberSidebar.ts`
//! (`applyChannelSubscription`). Range math ports
//! `MemberListRangeUtils.ts`.

use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{GatewayError, GatewayHandle, op};

/// `MEMBER_LIST_RANGE_MAX_END` (`GatewayConstants.ts:26`).
pub const MEMBER_LIST_RANGE_MAX_END: u32 = 100_000;
/// `MEMBER_LIST_RANGE_MAX_SPAN`: a range covers at most 100 rows.
pub const MEMBER_LIST_RANGE_MAX_SPAN: u32 = 99;
/// `MEMBER_LIST_RANGE_PAGE_SIZE`.
pub const MEMBER_LIST_RANGE_PAGE_SIZE: u32 = MEMBER_LIST_RANGE_MAX_SPAN + 1;
/// `MEMBER_LIST_RANGE_WINDOW_OVERSCAN_PAGES`.
pub const MEMBER_LIST_RANGE_OVERSCAN_PAGES: u32 = 1;
/// `MEMBER_LIST_RANGE_MAX_RANGES`: ranges per channel subscription.
pub const MEMBER_LIST_RANGE_MAX_RANGES: usize = 10;

/// `normalizeMemberListRanges`: clamp to `0..=MAX_END`, sort, merge
/// overlapping/adjacent ranges, split into chunks of at most 100 rows, and
/// keep at most [`MEMBER_LIST_RANGE_MAX_RANGES`] chunks.
pub fn normalize_member_list_ranges(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut sane: Vec<(u32, u32)> = ranges
        .iter()
        .filter(|(start, _)| *start <= MEMBER_LIST_RANGE_MAX_END)
        .map(|&(start, end)| (start, end.max(start).min(MEMBER_LIST_RANGE_MAX_END)))
        .collect();
    sane.sort_by_key(|&(start, _)| start);
    let mut out = Vec::new();
    let mut current: Option<(u32, u32)> = None;
    for (start, end) in sane {
        current = match current {
            Some((cs, ce)) if start <= ce.saturating_add(1) => Some((cs, ce.max(end))),
            Some((cs, ce)) => {
                push_by_span(&mut out, cs, ce);
                Some((start, end))
            }
            None => Some((start, end)),
        };
    }
    if let Some((cs, ce)) = current {
        push_by_span(&mut out, cs, ce);
    }
    out
}

/// `pushMemberListRangeBySpan`.
fn push_by_span(out: &mut Vec<(u32, u32)>, start: u32, end: u32) {
    let mut chunk_start = start;
    while chunk_start <= end && out.len() < MEMBER_LIST_RANGE_MAX_RANGES {
        let chunk_end = end.min(chunk_start.saturating_add(MEMBER_LIST_RANGE_MAX_SPAN));
        out.push((chunk_start, chunk_end));
        match chunk_end.checked_add(1) {
            Some(next) => chunk_start = next,
            None => break,
        }
    }
}

/// `buildMemberListRangeWindow` for visible rows `first..=last`: the
/// 100-row pages around them plus one overscan page each side, with page 0
/// always kept (the header rows). `total_rows` clamps to the list length
/// (`Some(0)` = nothing to request).
pub fn member_list_page_ranges(first_visible_row: u32, last_visible_row: u32, total_rows: Option<u32>) -> Vec<(u32, u32)> {
    if total_rows == Some(0) {
        return Vec::new();
    }
    let last = last_visible_row.max(first_visible_row);
    let end_index = total_rows.map_or(last, |total| last.min(total - 1));
    let page = |row: u32| row / MEMBER_LIST_RANGE_PAGE_SIZE;
    let mut first_page = page(first_visible_row).saturating_sub(MEMBER_LIST_RANGE_OVERSCAN_PAGES);
    let mut last_page = first_page.max(page(end_index).saturating_add(MEMBER_LIST_RANGE_OVERSCAN_PAGES));
    if let Some(total) = total_rows {
        let max_page = page(total - 1);
        first_page = first_page.min(max_page);
        last_page = last_page.min(max_page);
    }
    let mut ranges = Vec::new();
    if first_page > 0 {
        ranges.push((0, MEMBER_LIST_RANGE_MAX_SPAN));
    }
    for p in first_page..=last_page {
        let start = p.saturating_mul(MEMBER_LIST_RANGE_PAGE_SIZE);
        ranges.push((start, start.saturating_add(MEMBER_LIST_RANGE_MAX_SPAN)));
    }
    normalize_member_list_ranges(&ranges)
}

/// The LAZY_REQUEST `subscriptions` value for one channel's member list:
/// `{guild_id: {active: true, sync?: true, member_list_channels:
/// {channel_id: [[start, end], ...]}}}`. `ranges` are normalized; an empty
/// list releases the channel's subscription. Send `sync: true` on the first
/// subscription to a guild in this session (`syncedMemberListGuildIds`).
pub fn member_list_subscription(guild_id: &str, channel_id: &str, ranges: &[(u32, u32)], sync: bool) -> Value {
    let ranges: Vec<[u32; 2]> = normalize_member_list_ranges(ranges)
        .into_iter()
        .map(|(start, end)| [start, end])
        .collect();
    let mut channels = Map::new();
    channels.insert(channel_id.to_owned(), json!(ranges));
    let mut guild = Map::new();
    guild.insert("active".to_owned(), Value::Bool(true));
    if sync {
        guild.insert("sync".to_owned(), Value::Bool(true));
    }
    guild.insert("member_list_channels".to_owned(), Value::Object(channels));
    let mut subscriptions = Map::new();
    subscriptions.insert(guild_id.to_owned(), Value::Object(guild));
    Value::Object(subscriptions)
}

/// Own presence for PRESENCE_UPDATE (op 3). `custom_status: None` leaves
/// the custom status out (unchanged); `Some(Value::Null)` clears it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PresenceUpdate {
    /// online, idle, dnd or invisible.
    pub status: String,
    pub afk: bool,
    pub mobile: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_status: Option<Value>,
}

const PRESENCE_STATUSES: [&str; 4] = ["online", "idle", "dnd", "invisible"];

fn validate_snowflake(id: &str) -> Result<(), GatewayError> {
    if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(GatewayError::InvalidArgument("guild/channel id must be a snowflake"));
    }
    Ok(())
}

/// Payload for VOICE_STATE_UPDATE (op 4), matching `GatewayVoiceStateUpdateParams`
/// and `buildVoiceStatePayload` in `GatewaySocket.ts`.
///
/// `guild_id`, `channel_id`, and `connection_id` are always serialized (as JSON
/// `null` when `None`) because the server distinguishes `null` leave/DM states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VoiceStateUpdate {
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub self_video: bool,
    pub self_stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewer_stream_keys: Option<Vec<String>>,
    pub connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p2p: Option<bool>,
    pub is_mobile: bool,
}

impl VoiceStateUpdate {
    /// Joins (or moves to) a guild or DM voice channel with all self flags off.
    pub fn join(guild_id: Option<String>, channel_id: impl Into<String>, connection_id: Option<String>) -> Self {
        Self {
            guild_id,
            channel_id: Some(channel_id.into()),
            self_mute: false,
            self_deaf: false,
            self_video: false,
            self_stream: false,
            viewer_stream_keys: None,
            connection_id,
            p2p: None,
            is_mobile: false,
        }
    }

    /// Leaves a guild voice channel (`guild_id: Some(..)`), a DM call
    /// (`guild_id: None, connection_id: Some(..)`), or disconnects all voice
    /// sessions (`guild_id: None, connection_id: None`) by sending
    /// `channel_id: null`.
    pub fn leave(guild_id: Option<String>, connection_id: Option<String>) -> Self {
        Self {
            guild_id,
            channel_id: None,
            self_mute: false,
            self_deaf: false,
            self_video: false,
            self_stream: false,
            viewer_stream_keys: None,
            connection_id,
            p2p: None,
            is_mobile: false,
        }
    }
}

impl GatewayHandle {
    /// LAZY_REQUEST for one channel's member list; see
    /// [`member_list_subscription`]. Rejects non-snowflake ids so they
    /// cannot reach the server as map keys.
    pub fn subscribe_member_list(
        &self,
        guild_id: &str,
        channel_id: &str,
        ranges: &[(u32, u32)],
        sync: bool,
    ) -> Result<(), GatewayError> {
        validate_snowflake(guild_id)?;
        validate_snowflake(channel_id)?;
        self.lazy_request(member_list_subscription(guild_id, channel_id, ranges, sync))
    }

    /// PRESENCE_UPDATE (op 3) with a typed body; remembered for IDENTIFY
    /// and re-sent after RESUMED like [`GatewayHandle::update_presence`].
    pub fn set_presence(&self, presence: &PresenceUpdate) -> Result<(), GatewayError> {
        if !PRESENCE_STATUSES.contains(&presence.status.as_str()) {
            return Err(GatewayError::InvalidArgument("status must be online, idle, dnd or invisible"));
        }
        self.update_presence(serde_json::to_value(presence)?)
    }

    /// VOICE_STATE_UPDATE (op 4) for joining, updating self state in, or
    /// leaving (`channel_id: None`) a voice channel.
    pub fn update_voice_state(&self, state: &VoiceStateUpdate) -> Result<(), GatewayError> {
        if let Some(guild_id) = state.guild_id.as_deref() {
            validate_snowflake(guild_id)?;
        }
        if let Some(channel_id) = state.channel_id.as_deref() {
            validate_snowflake(channel_id)?;
        }
        if let Some(connection_id) = state.connection_id.as_deref()
            && connection_id.is_empty()
        {
            return Err(GatewayError::InvalidArgument("connection_id must not be empty"));
        }
        self.send_raw(op::VOICE_STATE_UPDATE, serde_json::to_value(state)?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tokio::sync::{mpsc, watch};

    use super::*;
    use crate::gateway::Command;

    fn handle() -> (GatewayHandle, mpsc::Receiver<Command>) {
        let (commands, rx) = mpsc::channel(4);
        let (shutdown, _) = watch::channel(false);
        (GatewayHandle { commands, shutdown: Arc::new(shutdown) }, rx)
    }

    #[test]
    fn ranges_are_clamped_merged_and_split_by_span() {
        assert_eq!(normalize_member_list_ranges(&[]), vec![]);
        assert_eq!(normalize_member_list_ranges(&[(0, 199)]), vec![(0, 99), (100, 199)]);
        assert_eq!(normalize_member_list_ranges(&[(100, 150), (0, 99), (151, 160)]), vec![(0, 99), (100, 160)]);
        assert_eq!(normalize_member_list_ranges(&[(300, 310), (0, 10)]), vec![(0, 10), (300, 310)]);
        // end < start is raised to start; start past MAX_END is dropped.
        assert_eq!(normalize_member_list_ranges(&[(50, 10), (200_000, 200_001)]), vec![(50, 50)]);
        assert_eq!(normalize_member_list_ranges(&[(99_990, 200_000)]), vec![(99_990, 100_000)]);
        // At most 10 chunks.
        assert_eq!(normalize_member_list_ranges(&[(0, 5_000)]).len(), MEMBER_LIST_RANGE_MAX_RANGES);
    }

    #[test]
    fn page_ranges_follow_build_member_list_range_window() {
        assert_eq!(member_list_page_ranges(0, 30, None), vec![(0, 99), (100, 199)]);
        assert_eq!(member_list_page_ranges(250, 270, None), vec![(0, 99), (100, 199), (200, 299), (300, 399)]);
        assert_eq!(member_list_page_ranges(0, 30, Some(40)), vec![(0, 99)]);
        assert_eq!(member_list_page_ranges(0, 30, Some(0)), vec![]);
    }

    #[test]
    fn subscription_matches_the_ts_payload() {
        assert_eq!(
            member_list_subscription("1", "2", &[(0, 99)], true),
            json!({"1": {"active": true, "sync": true, "member_list_channels": {"2": [[0, 99]]}}})
        );
        assert_eq!(
            member_list_subscription("1", "2", &[], false),
            json!({"1": {"active": true, "member_list_channels": {"2": []}}})
        );
    }

    #[test]
    fn handle_sends_lazy_request_and_presence() {
        let (gateway, mut rx) = handle();
        gateway.subscribe_member_list("10", "20", &[(0, 150)], false).unwrap();
        match rx.try_recv() {
            Ok(Command::Send(frame)) => {
                let frame: Value = serde_json::from_str(&frame).unwrap();
                assert_eq!(
                    frame,
                    json!({"op": 14, "d": {"subscriptions": {"10": {"active": true,
                        "member_list_channels": {"20": [[0, 99], [100, 150]]}}}}})
                );
            }
            _ => panic!("expected a LAZY_REQUEST frame"),
        }
        assert!(matches!(
            gateway.subscribe_member_list("10", "x\"}", &[], false),
            Err(GatewayError::InvalidArgument(_))
        ));

        let presence = PresenceUpdate { status: "idle".into(), afk: true, mobile: false, custom_status: None };
        gateway.set_presence(&presence).unwrap();
        match rx.try_recv() {
            Ok(Command::Presence(d)) => assert_eq!(d, json!({"status": "idle", "afk": true, "mobile": false})),
            _ => panic!("expected a presence command"),
        }
        let cleared = PresenceUpdate { custom_status: Some(Value::Null), ..presence };
        gateway.set_presence(&cleared).unwrap();
        match rx.try_recv() {
            Ok(Command::Presence(d)) => assert_eq!(d["custom_status"], Value::Null),
            _ => panic!("expected a presence command"),
        }
        let bad = PresenceUpdate { status: "away".into(), afk: false, mobile: false, custom_status: None };
        assert!(matches!(gateway.set_presence(&bad), Err(GatewayError::InvalidArgument(_))));
    }

    #[test]
    fn handle_validates_and_encodes_voice_state_update() {
        let (gateway, mut rx) = handle();
        let mut join = VoiceStateUpdate::join(Some("42".into()), "100", Some("conn-1".into()));
        join.self_mute = true;
        join.viewer_stream_keys = Some(vec!["stream:1".into()]);
        join.p2p = Some(false);
        gateway.update_voice_state(&join).unwrap();
        match rx.try_recv() {
            Ok(Command::Send(frame)) => {
                let frame: Value = serde_json::from_str(&frame).unwrap();
                assert_eq!(
                    frame,
                    json!({
                        "op": 4,
                        "d": {
                            "guild_id": "42",
                            "channel_id": "100",
                            "self_mute": true,
                            "self_deaf": false,
                            "self_video": false,
                            "self_stream": false,
                            "viewer_stream_keys": ["stream:1"],
                            "connection_id": "conn-1",
                            "p2p": false,
                            "is_mobile": false
                        }
                    })
                );
            }
            _ => panic!("expected a VOICE_STATE_UPDATE frame"),
        }

        let leave = VoiceStateUpdate::leave(None, None);
        gateway.update_voice_state(&leave).unwrap();
        match rx.try_recv() {
            Ok(Command::Send(frame)) => {
                let frame: Value = serde_json::from_str(&frame).unwrap();
                assert_eq!(
                    frame,
                    json!({
                        "op": 4,
                        "d": {
                            "guild_id": null,
                            "channel_id": null,
                            "self_mute": false,
                            "self_deaf": false,
                            "self_video": false,
                            "self_stream": false,
                            "connection_id": null,
                            "is_mobile": false
                        }
                    })
                );
                let obj = frame["d"].as_object().unwrap();
                assert!(!obj.contains_key("viewer_stream_keys"));
                assert!(!obj.contains_key("p2p"));
            }
            _ => panic!("expected a VOICE_STATE_UPDATE leave frame"),
        }

        let bad_guild = VoiceStateUpdate::join(Some("abc".into()), "100", None);
        assert!(matches!(
            gateway.update_voice_state(&bad_guild),
            Err(GatewayError::InvalidArgument(_))
        ));
        let bad_channel = VoiceStateUpdate::join(Some("42".into()), "", None);
        assert!(matches!(
            gateway.update_voice_state(&bad_channel),
            Err(GatewayError::InvalidArgument(_))
        ));
        let bad_conn = VoiceStateUpdate::leave(Some("42".into()), Some(String::new()));
        assert!(matches!(
            gateway.update_voice_state(&bad_conn),
            Err(GatewayError::InvalidArgument(_))
        ));
    }
}
