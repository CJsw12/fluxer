//! Pure-Rust voice/video media contracts:
//! - Screen-share codec negotiation ported from
//!   `fluxer_app/src/features/voice/utils/ScreenShareCodecSelection.ts` and
//!   `fluxer_app/src/features/voice/engine/ScreenShareCodecNegotiation.ts`.
//! - Bounded latest-frame video queue and subscription-intent tracking for
//!   GPUI rendering without unbounded frame buffering.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Supported screen-share video codecs in default preference order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    Av1,
    H265,
    H264,
    Vp9,
    Vp8,
}

pub const CODEC_PREFERENCE: [VideoCodec; 5] = [
    VideoCodec::Av1,
    VideoCodec::H265,
    VideoCodec::H264,
    VideoCodec::Vp9,
    VideoCodec::Vp8,
];

pub const LAST_RESORT_VIDEO_CODEC: VideoCodec = VideoCodec::Vp8;

/// Maximum republish attempts allowed for codec renegotiation in a single
/// screen-share session to prevent infinite republish loops.
pub const MAX_CODEC_REPUBLISH_ATTEMPTS: u32 = 3;

impl VideoCodec {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Av1 => "AV1",
            Self::H265 => "H265",
            Self::H264 => "H264",
            Self::Vp9 => "VP9",
            Self::Vp8 => "VP8",
        }
    }

    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "AV1" | "av1" => Some(Self::Av1),
            "H265" | "h265" | "HEVC" | "hevc" => Some(Self::H265),
            "H264" | "h264" => Some(Self::H264),
            "VP9" | "vp9" => Some(Self::Vp9),
            "VP8" | "vp8" => Some(Self::Vp8),
            _ => None,
        }
    }

    /// True for universally supported fallback codecs eligible when one or more
    /// receivers have not reported their capabilities yet (`COMPATIBILITY_CODECS`).
    pub fn is_compatibility_codec(self) -> bool {
        matches!(self, Self::H264 | Self::Vp9 | Self::Vp8)
    }

    pub fn protocol_entry(self) -> (u8, u8, u16) {
        match self {
            Self::Av1 => (101, 102, 5000),
            Self::H265 => (105, 106, 4000),
            Self::H264 => (103, 104, 3000),
            Self::Vp9 => (109, 110, 2000),
            Self::Vp8 => (107, 108, 1000),
        }
    }
}

/// Wire advertisement exchanged on topic `fluxer.rtc.codec-negotiation.v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecAdvertisement {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub payload_type: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtx_payload_type: Option<u8>,
    pub priority: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode: Option<bool>,
}

/// Builds the local `opus` + video codec advertisement list.
pub fn build_codec_advertisements(
    encode_order: &[VideoCodec],
    decode_support: &BTreeSet<VideoCodec>,
) -> Vec<CodecAdvertisement> {
    let mut out = Vec::with_capacity(1 + CODEC_PREFERENCE.len());
    out.push(CodecAdvertisement {
        name: "opus".to_owned(),
        kind: "audio".to_owned(),
        payload_type: 120,
        rtx_payload_type: None,
        priority: 1000,
        encode: Some(true),
        decode: Some(true),
    });
    for codec in CODEC_PREFERENCE {
        let (pt, rtx, priority) = codec.protocol_entry();
        out.push(CodecAdvertisement {
            name: codec.wire_name().to_owned(),
            kind: "video".to_owned(),
            payload_type: pt,
            rtx_payload_type: Some(rtx),
            priority,
            encode: Some(encode_order.contains(&codec)),
            decode: Some(decode_support.contains(&codec)),
        });
    }
    out
}

pub fn decode_set(codecs: &[CodecAdvertisement]) -> BTreeSet<VideoCodec> {
    codecs
        .iter()
        .filter(|c| c.kind == "video" && c.decode == Some(true))
        .filter_map(|c| VideoCodec::from_wire_name(&c.name))
        .collect()
}

pub fn encode_set(codecs: &[CodecAdvertisement]) -> BTreeSet<VideoCodec> {
    codecs
        .iter()
        .filter(|c| c.kind == "video" && c.encode == Some(true))
        .filter_map(|c| VideoCodec::from_wire_name(&c.name))
        .collect()
}

/// True when `next_codec` ranks strictly higher (earlier) than `published_codec`.
pub fn is_codec_upgrade(order: &[VideoCodec], published_codec: VideoCodec, next_codec: VideoCodec) -> bool {
    let Some(published_rank) = order.iter().position(|&c| c == published_codec) else {
        return false;
    };
    let Some(next_rank) = order.iter().position(|&c| c == next_codec) else {
        return false;
    };
    next_rank < published_rank
}

/// Result of negotiating a screen-share video codec across all viewers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecNegotiationResult {
    pub selected: VideoCodec,
    pub candidates: Vec<VideoCodec>,
    pub unknown_participants: usize,
    pub used_fallback: bool,
}

/// Computes the negotiated video codec matching `computeNegotiatedVideoCodec` in
/// `ScreenShareCodecSelection.ts`:
/// - Excludes non-compatibility codecs (H.265, AV1) whenever any viewer has not
///   reported codec support (`unknown_participants > 0`).
/// - Requires every known remote viewer to advertise `decode: true`.
/// - Falls back to the first common compatibility codec, or `VP8` as last resort.
pub fn negotiate_screen_share_codec(
    local_codecs: &[CodecAdvertisement],
    remote_codecs: &[Vec<CodecAdvertisement>],
    unknown_participants: usize,
    preference: &[VideoCodec],
    published_codec: Option<VideoCodec>,
) -> CodecNegotiationResult {
    let local_encode = encode_set(local_codecs);
    let remote_decode: Vec<BTreeSet<VideoCodec>> = remote_codecs.iter().map(|r| decode_set(r)).collect();

    let candidates: Vec<VideoCodec> = preference
        .iter()
        .copied()
        .filter(|&codec| {
            if !local_encode.contains(&codec) {
                return false;
            }
            if unknown_participants > 0 && !codec.is_compatibility_codec() {
                return false;
            }
            remote_decode.iter().all(|decode| decode.contains(&codec))
        })
        .collect();

    let (raw_selected, used_fallback) = if let Some(&best) = candidates.first() {
        (best, false)
    } else {
        let fallback = CODEC_PREFERENCE
            .iter()
            .copied()
            .find(|&codec| {
                codec.is_compatibility_codec()
                    && local_encode.contains(&codec)
                    && remote_decode.iter().all(|decode| decode.contains(&codec))
            })
            .unwrap_or(LAST_RESORT_VIDEO_CODEC);
        (fallback, true)
    };

    // Match TS policy: do not disrupt an active stream for a pure upward upgrade.
    let selected = match published_codec {
        Some(current) if is_codec_upgrade(preference, current, raw_selected) => current,
        _ => raw_selected,
    };

    CodecNegotiationResult {
        selected,
        candidates,
        unknown_participants,
        used_fallback,
    }
}

/// Tracks requested, negotiated, and actually rendered codecs separately, and
/// caps codec-change republish attempts per screen-share session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenShareCodecTracker {
    pub requested: Option<VideoCodec>,
    pub negotiated: Option<VideoCodec>,
    pub rendered: Option<VideoCodec>,
    pub republish_count: u32,
    pub max_republishes: u32,
}

impl Default for ScreenShareCodecTracker {
    fn default() -> Self {
        Self::new(MAX_CODEC_REPUBLISH_ATTEMPTS)
    }
}

impl ScreenShareCodecTracker {
    pub fn new(max_republishes: u32) -> Self {
        Self {
            requested: None,
            negotiated: None,
            rendered: None,
            republish_count: 0,
            max_republishes,
        }
    }

    pub fn set_requested(&mut self, codec: VideoCodec) {
        self.requested = Some(codec);
    }

    /// Records a newly negotiated codec. Returns `Ok(true)` if a republish is
    /// required and within budget, `Ok(false)` if no change is needed, or
    /// `Err` if the republish budget is exhausted.
    pub fn apply_negotiated(&mut self, next: VideoCodec) -> Result<bool, &'static str> {
        match self.negotiated {
            None => {
                self.negotiated = Some(next);
                Ok(true)
            }
            Some(current) if current == next => Ok(false),
            Some(_) => {
                if self.republish_count >= self.max_republishes {
                    return Err("screen share codec republish limit reached");
                }
                self.republish_count += 1;
                self.negotiated = Some(next);
                Ok(true)
            }
        }
    }

    pub fn record_rendered(&mut self, codec: VideoCodec) {
        self.rendered = Some(codec);
    }

    pub fn reset(&mut self) {
        self.requested = None;
        self.negotiated = None;
        self.rendered = None;
        self.republish_count = 0;
    }
}

/// Decoded video frame handed from a native receiver stream to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedVideoFrame {
    pub width: u32,
    pub height: u32,
    pub timestamp_us: i64,
    pub codec: Option<VideoCodec>,
    /// Owned RGBA/BGRA/NV12 bytes for the frame.
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
struct LatestSlotInner {
    frame: Option<DecodedVideoFrame>,
    generation: u64,
    dropped_frames: u64,
    closed: bool,
}

/// Capacity-1 latest-frame queue: pushes always overwrite any unconsumed older
/// frame so memory stays strictly bounded to a single frame per track.
#[derive(Debug, Clone, Default)]
pub struct LatestVideoFrameQueue {
    inner: Arc<Mutex<LatestSlotInner>>,
}

impl LatestVideoFrameQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `frame` as the latest frame, dropping any unconsumed previous frame.
    /// Returns `false` if the queue has been closed.
    pub fn push(&self, frame: DecodedVideoFrame) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.closed {
            return false;
        }
        if guard.frame.replace(frame).is_some() {
            guard.dropped_frames = guard.dropped_frames.saturating_add(1);
        }
        guard.generation = guard.generation.saturating_add(1);
        true
    }

    /// Takes the latest frame if its generation is newer than `last_generation`.
    pub fn take_if_newer(&self, last_generation: u64) -> Option<(u64, DecodedVideoFrame)> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.generation <= last_generation {
            return None;
        }
        let frame = guard.frame.take()?;
        Some((guard.generation, frame))
    }

    pub fn dropped_frames(&self) -> u64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).dropped_frames
    }

    pub fn close(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.closed = true;
        guard.frame = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_set_of(codecs: &[VideoCodec]) -> BTreeSet<VideoCodec> {
        codecs.iter().copied().collect()
    }

    #[test]
    fn h265_selected_when_all_viewers_report_h265_decode() {
        let local = build_codec_advertisements(
            &[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8],
            &decode_set_of(&[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8]),
        );
        let remote_a = build_codec_advertisements(
            &[VideoCodec::H264],
            &decode_set_of(&[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8]),
        );
        let remote_b = build_codec_advertisements(
            &[VideoCodec::Vp8],
            &decode_set_of(&[VideoCodec::H265, VideoCodec::Vp8]),
        );
        let res = negotiate_screen_share_codec(
            &local,
            &[remote_a, remote_b],
            0,
            &[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8],
            None,
        );
        assert_eq!(res.selected, VideoCodec::H265);
        assert!(!res.used_fallback);
    }

    #[test]
    fn falls_back_from_h265_for_unsupported_or_unreported_receivers() {
        let local = build_codec_advertisements(
            &[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8],
            &decode_set_of(&[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8]),
        );
        let remote_h265 = build_codec_advertisements(
            &[],
            &decode_set_of(&[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8]),
        );
        let remote_no_h265 = build_codec_advertisements(
            &[],
            &decode_set_of(&[VideoCodec::H264, VideoCodec::Vp8]),
        );

        // One viewer lacks H.265 -> selects common H.264.
        let res = negotiate_screen_share_codec(
            &local,
            &[remote_h265.clone(), remote_no_h265],
            0,
            &[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8],
            None,
        );
        assert_eq!(res.selected, VideoCodec::H264);

        // All known viewers support H.265, but 1 participant has not reported yet -> H.265 excluded.
        let res_unknown = negotiate_screen_share_codec(
            &local,
            &[remote_h265],
            1,
            &[VideoCodec::H265, VideoCodec::H264, VideoCodec::Vp8],
            None,
        );
        assert_eq!(res_unknown.selected, VideoCodec::H264);
        assert_eq!(res_unknown.unknown_participants, 1);
    }

    #[test]
    fn codec_tracker_separates_requested_negotiated_rendered_and_caps_republishes() {
        let mut tracker = ScreenShareCodecTracker::new(2);
        tracker.set_requested(VideoCodec::H265);
        assert_eq!(tracker.apply_negotiated(VideoCodec::H264), Ok(true));
        tracker.record_rendered(VideoCodec::H264);
        assert_eq!(tracker.requested, Some(VideoCodec::H265));
        assert_eq!(tracker.negotiated, Some(VideoCodec::H264));
        assert_eq!(tracker.rendered, Some(VideoCodec::H264));

        // Same codec is a no-op and does not consume republish budget.
        assert_eq!(tracker.apply_negotiated(VideoCodec::H264), Ok(false));
        assert_eq!(tracker.republish_count, 0);

        // Two downgrades/changes succeed; the third exceeds budget.
        assert_eq!(tracker.apply_negotiated(VideoCodec::Vp8), Ok(true));
        assert_eq!(tracker.apply_negotiated(VideoCodec::H264), Ok(true));
        assert!(tracker.apply_negotiated(VideoCodec::Vp8).is_err());
    }

    #[test]
    fn latest_video_frame_queue_overwrites_stale_frames_and_clears_on_close() {
        let queue = LatestVideoFrameQueue::new();
        let make = |ts: i64| DecodedVideoFrame {
            width: 640,
            height: 360,
            timestamp_us: ts,
            codec: Some(VideoCodec::H265),
            data: vec![1, 2, 3, 4],
        };
        assert!(queue.push(make(100)));
        assert!(queue.push(make(200)));
        assert!(queue.push(make(300)));
        assert_eq!(queue.dropped_frames(), 2);

        let (generation, frame) = queue.take_if_newer(0).expect("latest frame");
        assert_eq!(generation, 3);
        assert_eq!(frame.timestamp_us, 300);
        assert!(queue.take_if_newer(generation).is_none());

        queue.close();
        assert!(!queue.push(make(400)));
        assert!(queue.take_if_newer(0).is_none());
    }
}
