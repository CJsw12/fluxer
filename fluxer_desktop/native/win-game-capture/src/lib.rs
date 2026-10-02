// SPDX-License-Identifier: AGPL-3.0-or-later

#![deny(clippy::all)]
#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "windows")]
mod capture_target;
#[cfg(target_os = "windows")]
mod d3d11_device;
#[cfg(any(target_os = "windows", test))]
mod dxgi_capture;
pub mod encoder_attach;
mod fallback;
mod game_capture_abi;
mod gpu_priority;
mod hdr;
#[cfg(target_os = "windows")]
mod nv12_gpu;
mod sources;
#[cfg(any(target_os = "windows", test))]
mod stall;
#[cfg(target_os = "windows")]
mod wgc_capture;

pub use encoder_attach::{EncoderAttachError, EncoderAttachStats, EncoderAttachment};

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{JsValue, Status, ValueType};
use napi_derive::napi;
use parking_lot::{Mutex, RwLock};
use std::ffi::c_void;
use std::sync::Arc;

#[cfg(target_os = "windows")]
use dxgi_capture::DxgiCaptureSession;
use fluxer_encoder_ring::EncoderFrameRate;
#[cfg(target_os = "windows")]
use fluxer_screen_frame_bus::EnqueueOutcome;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
#[cfg(target_os = "windows")]
use wgc_capture::WgcCaptureSession;

const LIFECYCLE_QUEUE_LIMIT: usize = 8;
const START_OPTION_UNSUPPORTED_LIMIT: usize = 4;
#[cfg(target_os = "windows")]
// Two dispatch slots absorb short JS stalls; permits keep the native queue bounded.
const CPU_FRAME_PENDING_LIMIT: usize = 2;

type LifecycleTsfn = Arc<
    ThreadsafeFunction<
        (String, String),
        (),
        (String, String),
        napi::Status,
        false,
        true,
        LIFECYCLE_QUEUE_LIMIT,
    >,
>;

#[cfg(target_os = "windows")]
type CpuFrameTsfn =
    Arc<ThreadsafeFunction<QueuedCpuFrame, (), CpuFrame, napi::Status, false, true, 0>>;

#[cfg(target_os = "windows")]
#[napi(object)]
pub struct CpuFrame {
    pub width: u32,
    pub height: u32,
    #[napi(js_name = "pixelFormat")]
    pub pixel_format: String,
    #[napi(js_name = "timestampUs")]
    pub timestamp_us: i64,
    pub data: Buffer,
}

#[cfg(target_os = "windows")]
struct CpuFramePermit {
    pending: Arc<AtomicUsize>,
}

#[cfg(target_os = "windows")]
impl CpuFramePermit {
    fn try_acquire(pending: &Arc<AtomicUsize>) -> Option<Self> {
        pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < CPU_FRAME_PENDING_LIMIT).then_some(count + 1)
            })
            .ok()
            .map(|_| Self {
                pending: Arc::clone(pending),
            })
    }

    fn convert<T, E>(
        self,
        convert: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<(T, Self), E> {
        convert().map(|value| (value, self))
    }
}

#[cfg(target_os = "windows")]
impl Drop for CpuFramePermit {
    fn drop(&mut self) {
        self.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(target_os = "windows")]
fn try_convert_cpu_frame<T, E>(
    pending: &Arc<AtomicUsize>,
    convert: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<Option<(T, CpuFramePermit)>, E> {
    let Some(permit) = CpuFramePermit::try_acquire(pending) else {
        return Ok(None);
    };
    permit.convert(convert).map(Some)
}

#[cfg(target_os = "windows")]
struct QueuedCpuFrame {
    frame: CpuFrame,
    _permit: CpuFramePermit,
}

#[cfg(target_os = "windows")]
impl QueuedCpuFrame {
    fn into_js_frame(self) -> CpuFrame {
        let Self { frame, _permit: _ } = self;
        frame
    }
}

#[napi(object)]
#[derive(Clone, Debug)]
pub struct ScreenCaptureRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[napi(object)]
#[derive(Clone, Debug, Default)]
pub struct ScreenCaptureStartOptions {
    #[napi(js_name = "showCursorClicks")]
    pub show_cursor_clicks: Option<bool>,
    #[napi(js_name = "captureRect")]
    pub capture_rect: Option<ScreenCaptureRect>,
    #[napi(js_name = "colorRange")]
    pub color_range: Option<String>,
    #[napi(js_name = "colorSpace")]
    pub color_space: Option<String>,
}

#[napi(object)]
#[derive(Clone, Debug, Default)]
pub struct CaptureStartOptionsDiagnostics {
    #[napi(js_name = "showCursorClicks")]
    pub show_cursor_clicks: Option<bool>,
    #[napi(js_name = "captureRect")]
    pub capture_rect: Option<ScreenCaptureRect>,
    #[napi(js_name = "colorRange")]
    pub color_range: Option<String>,
    #[napi(js_name = "colorSpace")]
    pub color_space: Option<String>,
    #[napi(js_name = "unsupportedOptions")]
    pub unsupported_options: Vec<String>,
}

#[napi(object)]
pub struct CaptureStartResult {
    pub width: u32,
    pub height: u32,
    #[napi(js_name = "frameRate")]
    pub frame_rate: u32,
    #[napi(js_name = "pixelFormat")]
    pub pixel_format: String,
}

#[napi(object)]
pub struct ScreenCaptureSourceDescriptor {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    #[napi(js_name = "targetPid")]
    pub target_pid: Option<u32>,
}

#[napi(object)]
pub struct AvailabilityInfo {
    pub available: bool,
    pub backend: String,
    pub reason: Option<String>,
}

#[napi(object)]
pub struct CaptureDiagnostics {
    pub state: u32,
    #[napi(js_name = "apiType")]
    pub api_type: u32,
    pub transport: u32,
    #[napi(js_name = "fallbackReason")]
    pub fallback_reason: u32,
    #[napi(js_name = "captureFlags")]
    pub capture_flags: u32,
    pub width: u32,
    pub height: u32,
    #[napi(js_name = "dxgiFormat")]
    pub dxgi_format: u32,
    #[napi(js_name = "frameCounter")]
    pub frame_counter: f64,
    #[napi(js_name = "droppedFrameCounter")]
    pub dropped_frame_counter: f64,
    #[napi(js_name = "lastPresentTimestampUs")]
    pub last_present_timestamp_us: f64,
    #[napi(js_name = "lastError")]
    pub last_error: u32,
    #[napi(js_name = "activeStrategy")]
    pub active_strategy: String,
    #[napi(js_name = "lastFallbackReason")]
    pub last_fallback_reason: String,
    #[napi(js_name = "startOptions")]
    pub start_options: CaptureStartOptionsDiagnostics,
    #[napi(js_name = "frameSinkAccepted")]
    pub frame_sink_accepted: f64,
    #[napi(js_name = "frameSinkCoalesced")]
    pub frame_sink_coalesced: f64,
    #[napi(js_name = "frameSinkRejected")]
    pub frame_sink_rejected: f64,
    #[napi(js_name = "mediaFramesDroppedWithoutSink")]
    pub media_frames_dropped_without_sink: f64,
    #[napi(js_name = "cpuFallbackFramesDropped")]
    pub cpu_fallback_frames_dropped: f64,
    #[napi(js_name = "cpuPipeline")]
    pub cpu_pipeline: Option<CpuPipelineDiagnostics>,
}

#[napi(object)]
pub struct CpuPipelineDiagnostics {
    #[napi(js_name = "framesAcquired")]
    pub frames_acquired: f64,
    #[napi(js_name = "framesCoalesced")]
    pub frames_coalesced: f64,
    #[napi(js_name = "permitRejectedFrames")]
    pub permit_rejected_frames: f64,
    #[napi(js_name = "conversionCount")]
    pub conversion_count: f64,
    #[napi(js_name = "conversionTotalMs")]
    pub conversion_total_ms: f64,
    #[napi(js_name = "conversionMaxMs")]
    pub conversion_max_ms: f64,
    #[napi(js_name = "readbackMapCount")]
    pub readback_map_count: f64,
    #[napi(js_name = "readbackMapTotalMs")]
    pub readback_map_total_ms: f64,
    #[napi(js_name = "readbackMapMaxMs")]
    pub readback_map_max_ms: f64,
    #[napi(js_name = "cpuPackCount")]
    pub cpu_pack_count: f64,
    #[napi(js_name = "cpuPackTotalMs")]
    pub cpu_pack_total_ms: f64,
    #[napi(js_name = "cpuPackMaxMs")]
    pub cpu_pack_max_ms: f64,
    #[napi(js_name = "inputWidth")]
    pub input_width: u32,
    #[napi(js_name = "inputHeight")]
    pub input_height: u32,
    #[napi(js_name = "hdrToneMapEnabled")]
    pub hdr_tone_map_enabled: bool,
    #[napi(js_name = "recent5Seconds")]
    pub recent5_seconds: CpuPipelineRecentDiagnostics,
}

#[napi(object)]
pub struct CpuPipelineRecentDiagnostics {
    #[napi(js_name = "windowMs")]
    pub window_ms: f64,
    #[napi(js_name = "bucketMs")]
    pub bucket_ms: f64,
    #[napi(js_name = "framesAcquired")]
    pub frames_acquired: f64,
    #[napi(js_name = "framesCoalesced")]
    pub frames_coalesced: f64,
    #[napi(js_name = "permitRejectedFrames")]
    pub permit_rejected_frames: f64,
    #[napi(js_name = "conversionCount")]
    pub conversion_count: f64,
    #[napi(js_name = "conversionTotalMs")]
    pub conversion_total_ms: f64,
    #[napi(js_name = "conversionMaxMs")]
    pub conversion_max_ms: f64,
    #[napi(js_name = "readbackMapMaxMs")]
    pub readback_map_max_ms: f64,
    #[napi(js_name = "cpuPackMaxMs")]
    pub cpu_pack_max_ms: f64,
    #[napi(js_name = "hdrWhiteQueryCount")]
    pub hdr_white_query_count: f64,
    #[napi(js_name = "hdrWhiteQueryMaxMs")]
    pub hdr_white_query_max_ms: f64,
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct CpuPipelineStageCounters {
    count: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

#[cfg(target_os = "windows")]
impl CpuPipelineStageCounters {
    fn record(&self, duration: std::time::Duration) {
        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
        self.count.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(nanos, Ordering::Relaxed);
        self.max_ns.fetch_max(nanos, Ordering::Relaxed);
    }

    fn snapshot(&self) -> (f64, f64, f64) {
        const NANOS_PER_MILLI: f64 = 1_000_000.0;
        (
            self.count.load(Ordering::Relaxed) as f64,
            self.total_ns.load(Ordering::Relaxed) as f64 / NANOS_PER_MILLI,
            self.max_ns.load(Ordering::Relaxed) as f64 / NANOS_PER_MILLI,
        )
    }

    fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
        self.total_ns.store(0, Ordering::Relaxed);
        self.max_ns.store(0, Ordering::Relaxed);
    }
}

#[cfg(target_os = "windows")]
const CPU_PIPELINE_RECENT_BUCKET: std::time::Duration = std::time::Duration::from_millis(100);
#[cfg(target_os = "windows")]
const CPU_PIPELINE_RECENT_BUCKETS: usize = 51;
#[cfg(target_os = "windows")]
const CPU_PIPELINE_RECENT_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(target_os = "windows")]
#[derive(Default)]
struct CpuPipelineRecentBucket {
    index: u64,
    frames_acquired: u64,
    frames_coalesced: u64,
    permit_rejected_frames: u64,
    conversion_count: u64,
    conversion_total_ns: u64,
    conversion_max_ns: u64,
    readback_map_max_ns: u64,
    cpu_pack_max_ns: u64,
    hdr_white_query_count: u64,
    hdr_white_query_max_ns: u64,
}

#[cfg(target_os = "windows")]
enum CpuPipelineRecentEvent {
    FrameAcquired,
    FrameCoalesced,
    PermitRejected,
    Conversion(std::time::Duration),
    ReadbackMap(std::time::Duration),
    CpuPack(std::time::Duration),
    HdrWhiteQuery(std::time::Duration),
}

#[cfg(target_os = "windows")]
struct CpuPipelineRecentBuckets {
    origin: std::time::Instant,
    buckets: std::collections::VecDeque<CpuPipelineRecentBucket>,
}

#[cfg(target_os = "windows")]
impl Default for CpuPipelineRecentBuckets {
    fn default() -> Self {
        Self {
            origin: std::time::Instant::now(),
            buckets: std::collections::VecDeque::with_capacity(CPU_PIPELINE_RECENT_BUCKETS),
        }
    }
}

#[cfg(target_os = "windows")]
impl CpuPipelineRecentBuckets {
    fn reset(&mut self, now: std::time::Instant) {
        self.origin = now;
        self.buckets.clear();
    }

    fn record(&mut self, now: std::time::Instant, event: CpuPipelineRecentEvent) {
        let index = now
            .saturating_duration_since(self.origin)
            .as_millis()
            .checked_div(CPU_PIPELINE_RECENT_BUCKET.as_millis())
            .unwrap_or(0) as u64;
        self.expire_before(index.saturating_sub((CPU_PIPELINE_RECENT_BUCKETS - 1) as u64));
        if self
            .buckets
            .back()
            .is_none_or(|bucket| bucket.index != index)
        {
            self.buckets.push_back(CpuPipelineRecentBucket {
                index,
                ..CpuPipelineRecentBucket::default()
            });
            while self.buckets.len() > CPU_PIPELINE_RECENT_BUCKETS {
                self.buckets.pop_front();
            }
        }
        let Some(bucket) = self.buckets.back_mut() else {
            return;
        };
        match event {
            CpuPipelineRecentEvent::FrameAcquired => {
                bucket.frames_acquired = bucket.frames_acquired.saturating_add(1);
            }
            CpuPipelineRecentEvent::FrameCoalesced => {
                bucket.frames_coalesced = bucket.frames_coalesced.saturating_add(1);
            }
            CpuPipelineRecentEvent::PermitRejected => {
                bucket.permit_rejected_frames = bucket.permit_rejected_frames.saturating_add(1);
            }
            CpuPipelineRecentEvent::Conversion(duration) => {
                bucket.conversion_count = bucket.conversion_count.saturating_add(1);
                let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
                bucket.conversion_total_ns = bucket.conversion_total_ns.saturating_add(nanos);
                bucket.conversion_max_ns = bucket.conversion_max_ns.max(nanos);
            }
            CpuPipelineRecentEvent::ReadbackMap(duration) => {
                bucket.readback_map_max_ns = bucket
                    .readback_map_max_ns
                    .max(duration.as_nanos().min(u64::MAX as u128) as u64);
            }
            CpuPipelineRecentEvent::CpuPack(duration) => {
                bucket.cpu_pack_max_ns = bucket
                    .cpu_pack_max_ns
                    .max(duration.as_nanos().min(u64::MAX as u128) as u64);
            }
            CpuPipelineRecentEvent::HdrWhiteQuery(duration) => {
                bucket.hdr_white_query_count = bucket.hdr_white_query_count.saturating_add(1);
                bucket.hdr_white_query_max_ns = bucket
                    .hdr_white_query_max_ns
                    .max(duration.as_nanos().min(u64::MAX as u128) as u64);
            }
        }
    }

    fn snapshot(&mut self, now: std::time::Instant) -> CpuPipelineRecentDiagnostics {
        let current_index = now
            .saturating_duration_since(self.origin)
            .as_millis()
            .checked_div(CPU_PIPELINE_RECENT_BUCKET.as_millis())
            .unwrap_or(0) as u64;
        self.expire_before(current_index.saturating_sub((CPU_PIPELINE_RECENT_BUCKETS - 1) as u64));

        let window_ms = (now.saturating_duration_since(self.origin).as_secs_f64() * 1_000.0)
            .clamp(1.0, CPU_PIPELINE_RECENT_WINDOW.as_secs_f64() * 1_000.0);
        let mut result = CpuPipelineRecentDiagnostics {
            window_ms,
            bucket_ms: CPU_PIPELINE_RECENT_BUCKET.as_millis() as f64,
            frames_acquired: 0.0,
            frames_coalesced: 0.0,
            permit_rejected_frames: 0.0,
            conversion_count: 0.0,
            conversion_total_ms: 0.0,
            conversion_max_ms: 0.0,
            readback_map_max_ms: 0.0,
            cpu_pack_max_ms: 0.0,
            hdr_white_query_count: 0.0,
            hdr_white_query_max_ms: 0.0,
        };
        let mut conversion_total_ns = 0u64;
        let mut conversion_max_ns = 0u64;
        let mut readback_map_max_ns = 0u64;
        let mut cpu_pack_max_ns = 0u64;
        let mut hdr_white_query_max_ns = 0u64;
        for bucket in &self.buckets {
            result.frames_acquired += bucket.frames_acquired as f64;
            result.frames_coalesced += bucket.frames_coalesced as f64;
            result.permit_rejected_frames += bucket.permit_rejected_frames as f64;
            result.conversion_count += bucket.conversion_count as f64;
            conversion_total_ns = conversion_total_ns.saturating_add(bucket.conversion_total_ns);
            conversion_max_ns = conversion_max_ns.max(bucket.conversion_max_ns);
            readback_map_max_ns = readback_map_max_ns.max(bucket.readback_map_max_ns);
            cpu_pack_max_ns = cpu_pack_max_ns.max(bucket.cpu_pack_max_ns);
            result.hdr_white_query_count += bucket.hdr_white_query_count as f64;
            hdr_white_query_max_ns = hdr_white_query_max_ns.max(bucket.hdr_white_query_max_ns);
        }
        const NANOS_PER_MILLI: f64 = 1_000_000.0;
        result.conversion_total_ms = conversion_total_ns as f64 / NANOS_PER_MILLI;
        result.conversion_max_ms = conversion_max_ns as f64 / NANOS_PER_MILLI;
        result.readback_map_max_ms = readback_map_max_ns as f64 / NANOS_PER_MILLI;
        result.cpu_pack_max_ms = cpu_pack_max_ns as f64 / NANOS_PER_MILLI;
        result.hdr_white_query_max_ms = hdr_white_query_max_ns as f64 / NANOS_PER_MILLI;
        result
    }

    fn expire_before(&mut self, oldest_index: u64) {
        while self
            .buckets
            .front()
            .is_some_and(|bucket| bucket.index < oldest_index)
        {
            self.buckets.pop_front();
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct CpuPipelineStats {
    frames_acquired: AtomicU64,
    frames_coalesced: AtomicU64,
    permit_rejected_frames: AtomicU64,
    conversion: CpuPipelineStageCounters,
    readback_map: CpuPipelineStageCounters,
    cpu_pack: CpuPipelineStageCounters,
    recent: Mutex<CpuPipelineRecentBuckets>,
    input_width: AtomicU64,
    input_height: AtomicU64,
    hdr_tone_map_enabled: AtomicBool,
}

#[cfg(target_os = "windows")]
impl CpuPipelineStats {
    fn record_acquired(&self) {
        self.record_acquired_at(std::time::Instant::now());
    }

    fn record_acquired_at(&self, now: std::time::Instant) {
        self.frames_acquired.fetch_add(1, Ordering::Relaxed);
        self.record_recent_at(now, CpuPipelineRecentEvent::FrameAcquired);
    }

    fn record_coalesced(&self) {
        self.record_coalesced_at(std::time::Instant::now());
    }

    fn record_coalesced_at(&self, now: std::time::Instant) {
        self.frames_coalesced.fetch_add(1, Ordering::Relaxed);
        self.record_recent_at(now, CpuPipelineRecentEvent::FrameCoalesced);
    }

    fn record_permit_rejected(&self) {
        self.record_permit_rejected_at(std::time::Instant::now());
    }

    fn record_permit_rejected_at(&self, now: std::time::Instant) {
        self.permit_rejected_frames.fetch_add(1, Ordering::Relaxed);
        self.record_recent_at(now, CpuPipelineRecentEvent::PermitRejected);
    }

    fn record_conversion(&self, duration: std::time::Duration) {
        self.record_conversion_at(duration, std::time::Instant::now());
    }

    fn record_conversion_at(&self, duration: std::time::Duration, now: std::time::Instant) {
        self.conversion.record(duration);
        self.record_recent_at(now, CpuPipelineRecentEvent::Conversion(duration));
    }

    fn record_readback_map(&self, duration: std::time::Duration) {
        self.record_readback_map_at(duration, std::time::Instant::now());
    }

    fn record_readback_map_at(&self, duration: std::time::Duration, now: std::time::Instant) {
        self.readback_map.record(duration);
        self.record_recent_at(now, CpuPipelineRecentEvent::ReadbackMap(duration));
    }

    fn record_cpu_pack(&self, duration: std::time::Duration) {
        self.record_cpu_pack_at(duration, std::time::Instant::now());
    }

    fn record_cpu_pack_at(&self, duration: std::time::Duration, now: std::time::Instant) {
        self.cpu_pack.record(duration);
        self.record_recent_at(now, CpuPipelineRecentEvent::CpuPack(duration));
    }

    fn record_hdr_white_query(&self, duration: std::time::Duration) {
        self.record_hdr_white_query_at(duration, std::time::Instant::now());
    }

    fn record_hdr_white_query_at(&self, duration: std::time::Duration, now: std::time::Instant) {
        self.record_recent_at(now, CpuPipelineRecentEvent::HdrWhiteQuery(duration));
    }

    fn record_recent_at(&self, now: std::time::Instant, event: CpuPipelineRecentEvent) {
        self.recent.lock().record(now, event);
    }

    fn configure(&self, width: u32, height: u32, hdr_tone_map_enabled: bool) {
        self.input_width.store(width as u64, Ordering::Relaxed);
        self.input_height.store(height as u64, Ordering::Relaxed);
        self.hdr_tone_map_enabled
            .store(hdr_tone_map_enabled, Ordering::Relaxed);
    }

    fn reset(&self) {
        self.reset_at(std::time::Instant::now());
    }

    fn reset_at(&self, now: std::time::Instant) {
        self.frames_acquired.store(0, Ordering::Relaxed);
        self.frames_coalesced.store(0, Ordering::Relaxed);
        self.permit_rejected_frames.store(0, Ordering::Relaxed);
        self.conversion.reset();
        self.readback_map.reset();
        self.cpu_pack.reset();
        self.recent.lock().reset(now);
        self.configure(0, 0, false);
    }

    fn snapshot(&self) -> CpuPipelineDiagnostics {
        self.snapshot_at(std::time::Instant::now())
    }

    fn snapshot_at(&self, now: std::time::Instant) -> CpuPipelineDiagnostics {
        let (conversion_count, conversion_total_ms, conversion_max_ms) = self.conversion.snapshot();
        let (readback_map_count, readback_map_total_ms, readback_map_max_ms) =
            self.readback_map.snapshot();
        let (cpu_pack_count, cpu_pack_total_ms, cpu_pack_max_ms) = self.cpu_pack.snapshot();
        CpuPipelineDiagnostics {
            frames_acquired: self.frames_acquired.load(Ordering::Relaxed) as f64,
            frames_coalesced: self.frames_coalesced.load(Ordering::Relaxed) as f64,
            permit_rejected_frames: self.permit_rejected_frames.load(Ordering::Relaxed) as f64,
            conversion_count,
            conversion_total_ms,
            conversion_max_ms,
            readback_map_count,
            readback_map_total_ms,
            readback_map_max_ms,
            cpu_pack_count,
            cpu_pack_total_ms,
            cpu_pack_max_ms,
            input_width: self.input_width.load(Ordering::Relaxed) as u32,
            input_height: self.input_height.load(Ordering::Relaxed) as u32,
            hdr_tone_map_enabled: self.hdr_tone_map_enabled.load(Ordering::Relaxed),
            recent5_seconds: self.recent.lock().snapshot(now),
        }
    }
}

#[napi(object)]
pub struct EncoderAttachDiagnostics {
    pub attached: bool,
    pub width: u32,
    pub height: u32,
    pub capacity: u32,
    #[napi(js_name = "framesSubmitted")]
    pub frames_submitted: f64,
    #[napi(js_name = "framesDropped")]
    pub frames_dropped: f64,
    #[napi(js_name = "ringFullEvents")]
    pub ring_full_events: f64,
    #[napi(js_name = "failedBlits")]
    pub failed_blits: f64,
}

#[napi(object)]
pub struct FrameSinkDiagnostics {
    pub accepted: f64,
    pub coalesced: f64,
    pub rejected: f64,
    #[napi(js_name = "mediaFramesDroppedWithoutSink")]
    pub media_frames_dropped_without_sink: f64,
    #[napi(js_name = "cpuFallbackFramesDropped")]
    pub cpu_fallback_frames_dropped: f64,
}

pub struct CaptureInner {
    pub lifecycle_tsfn: Mutex<Option<LifecycleTsfn>>,
    #[cfg(target_os = "windows")]
    pub(crate) cpu_frame_tsfn: Mutex<Option<CpuFrameTsfn>>,
    #[cfg(target_os = "windows")]
    pub cpu_frame_pending: Arc<AtomicUsize>,
    #[cfg(target_os = "windows")]
    cpu_pipeline: CpuPipelineStats,
    #[cfg(target_os = "windows")]
    pub session: Mutex<Option<DxgiCaptureSession>>,
    #[cfg(target_os = "windows")]
    pub(crate) wgc_session: Mutex<Option<WgcCaptureSession>>,
    pub running: std::sync::atomic::AtomicBool,
    pub fallback: Mutex<Option<fallback::FallbackTracker>>,
    pub capture_id: Mutex<Option<String>>,
    pub start_options: Mutex<CaptureStartOptionsDiagnostics>,
    pub encoder_attachment: RwLock<Option<Arc<EncoderAttachment>>>,
    pub native_frame_sink:
        Mutex<Option<Arc<fluxer_screen_frame_bus::NativeScreenFrameSinkHandleRef>>>,
    #[cfg(target_os = "windows")]
    pub frame_sink_accepted: AtomicU64,
    #[cfg(target_os = "windows")]
    pub frame_sink_coalesced: AtomicU64,
    #[cfg(target_os = "windows")]
    pub frame_sink_rejected: AtomicU64,
    #[cfg(target_os = "windows")]
    pub media_frames_dropped_without_sink: AtomicU64,
    #[cfg(target_os = "windows")]
    pub cpu_fallback_frames_dropped: AtomicU64,
    #[cfg(target_os = "windows")]
    pub frame_sink_backpressure_emitted: AtomicBool,
    #[cfg(target_os = "windows")]
    pub frame_sink_missing_emitted: AtomicBool,
    #[cfg(target_os = "windows")]
    pub cpu_fallback_emitted: AtomicBool,
}

pub fn emit_lifecycle(inner: &CaptureInner, event_type: &str, message: &str) {
    let guard = inner.lifecycle_tsfn.lock();
    if let Some(tsfn) = guard.as_ref() {
        let _ = tsfn.call(
            (event_type.to_string(), message.to_string()),
            ThreadsafeFunctionCallMode::NonBlocking,
        );
    }
}

#[cfg(target_os = "windows")]
struct BusSharedTexture {
    handle: u64,
    width: u32,
    height: u32,
    dxgi_format: u32,
    timestamp_us: i64,
}

#[cfg(target_os = "windows")]
impl BusSharedTexture {
    fn into_bus_desc(self) -> fluxer_screen_frame_bus::SharedTextureDesc {
        fluxer_screen_frame_bus::SharedTextureDesc {
            handle: self.handle,
            width: self.width,
            height: self.height,
            dxgi_format: self.dxgi_format,
            timestamp_us: self.timestamp_us,
        }
    }
}

#[derive(Clone, Copy)]
struct FrameSinkCounterSnapshot {
    accepted: u64,
    coalesced: u64,
    rejected: u64,
    dropped_without_sink: u64,
    cpu_fallback_dropped: u64,
}

#[cfg(target_os = "windows")]
fn frame_sink_counter_snapshot(inner: &CaptureInner) -> FrameSinkCounterSnapshot {
    FrameSinkCounterSnapshot {
        accepted: inner.frame_sink_accepted.load(Ordering::Acquire),
        coalesced: inner.frame_sink_coalesced.load(Ordering::Acquire),
        rejected: inner.frame_sink_rejected.load(Ordering::Acquire),
        dropped_without_sink: inner
            .media_frames_dropped_without_sink
            .load(Ordering::Acquire),
        cpu_fallback_dropped: inner.cpu_fallback_frames_dropped.load(Ordering::Acquire),
    }
}

#[cfg(target_os = "windows")]
fn frame_sink_diagnostics_from(snapshot: FrameSinkCounterSnapshot) -> FrameSinkDiagnostics {
    FrameSinkDiagnostics {
        accepted: snapshot.accepted as f64,
        coalesced: snapshot.coalesced as f64,
        rejected: snapshot.rejected as f64,
        media_frames_dropped_without_sink: snapshot.dropped_without_sink as f64,
        cpu_fallback_frames_dropped: snapshot.cpu_fallback_dropped as f64,
    }
}

#[cfg(target_os = "windows")]
pub(crate) enum FrameSinkRef {
    Native(Arc<fluxer_screen_frame_bus::NativeScreenFrameSinkHandleRef>),
    Bus(Arc<dyn fluxer_screen_frame_bus::ScreenFrameSink>),
}

#[cfg(target_os = "windows")]
pub(crate) fn resolve_frame_sink(
    inner: &CaptureInner,
    capture_id: Option<&str>,
) -> Option<FrameSinkRef> {
    if let Some(sink) = native_frame_sink_for(inner) {
        return Some(FrameSinkRef::Native(sink));
    }
    let capture_id = capture_id?;
    fluxer_screen_frame_bus::get_sink(capture_id).map(FrameSinkRef::Bus)
}

#[cfg(target_os = "windows")]
pub(crate) fn emit_shared_texture_frame(
    inner: &CaptureInner,
    sink: &FrameSinkRef,
    handle: u64,
    width: u32,
    height: u32,
    dxgi_format: u32,
    timestamp_us: i64,
) -> bool {
    assert!(handle != 0, "shared texture handle is non-zero");
    assert!(width > 0, "shared texture width is positive");
    assert!(height > 0, "shared texture height is positive");
    let desc = BusSharedTexture {
        handle,
        width,
        height,
        dxgi_format,
        timestamp_us,
    }
    .into_bus_desc();
    let outcome = match sink {
        FrameSinkRef::Native(sink) => sink.enqueue_shared_texture(desc),
        FrameSinkRef::Bus(sink) => {
            sink.enqueue(fluxer_screen_frame_bus::ScreenFrame::SharedTexture(desc))
        }
    };
    record_frame_sink_outcome(inner, outcome);
    frame_sink_outcome_delivered(outcome)
}

#[cfg(target_os = "windows")]
fn frame_sink_outcome_delivered(outcome: EnqueueOutcome) -> bool {
    !matches!(outcome, EnqueueOutcome::Rejected)
}

#[cfg(target_os = "windows")]
fn record_frame_sink_outcome(inner: &CaptureInner, outcome: EnqueueOutcome) {
    match outcome {
        EnqueueOutcome::Accepted => {
            inner.frame_sink_accepted.fetch_add(1, Ordering::AcqRel);
        }
        EnqueueOutcome::Coalesced => {
            inner.frame_sink_coalesced.fetch_add(1, Ordering::AcqRel);
            emit_frame_sink_backpressure_once(
                inner,
                "Windows shared texture frame coalesced by native frame sink",
            );
        }
        EnqueueOutcome::Rejected => {
            inner.frame_sink_rejected.fetch_add(1, Ordering::AcqRel);
            emit_frame_sink_backpressure_once(
                inner,
                "Windows shared texture frame rejected by native frame sink",
            );
        }
    }
}

#[cfg(target_os = "windows")]
fn emit_frame_sink_backpressure_once(inner: &CaptureInner, message: &'static str) {
    if inner
        .frame_sink_backpressure_emitted
        .swap(true, Ordering::AcqRel)
    {
        return;
    }
    emit_lifecycle(inner, "diagnostic", message);
}

#[cfg(target_os = "windows")]
pub(crate) fn note_media_frame_without_sink(inner: &CaptureInner, message: &'static str) {
    inner
        .media_frames_dropped_without_sink
        .fetch_add(1, Ordering::AcqRel);
    if inner
        .frame_sink_missing_emitted
        .swap(true, Ordering::AcqRel)
    {
        return;
    }
    emit_lifecycle(inner, "diagnostic", message);
}

#[cfg(target_os = "windows")]
fn native_frame_sink_for(
    inner: &CaptureInner,
) -> Option<Arc<fluxer_screen_frame_bus::NativeScreenFrameSinkHandleRef>> {
    inner.native_frame_sink.lock().as_ref().cloned()
}

pub fn observe_fallback(
    inner: &CaptureInner,
    signature: fallback::FailureSignature,
) -> Option<fallback::FallbackDecision> {
    let decision = {
        let mut guard = inner.fallback.lock();
        guard.as_mut().map(|tracker| tracker.observe(signature))
    };
    if let Some(decision) = decision.as_ref() {
        let (kind, message) = fallback::decision_lifecycle(decision);
        emit_lifecycle(inner, kind, &message);
    }
    decision
}

#[napi]
pub struct ScreenCapture {
    inner: Arc<CaptureInner>,
}

#[napi]
impl ScreenCapture {
    #[allow(clippy::new_without_default)]
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CaptureInner {
                lifecycle_tsfn: Mutex::new(None),
                #[cfg(target_os = "windows")]
                cpu_frame_tsfn: Mutex::new(None),
                #[cfg(target_os = "windows")]
                cpu_frame_pending: Arc::new(AtomicUsize::new(0)),
                #[cfg(target_os = "windows")]
                cpu_pipeline: CpuPipelineStats::default(),
                #[cfg(target_os = "windows")]
                session: Mutex::new(None),
                #[cfg(target_os = "windows")]
                wgc_session: Mutex::new(None),
                running: std::sync::atomic::AtomicBool::new(false),
                fallback: Mutex::new(None),
                capture_id: Mutex::new(None),
                start_options: Mutex::new(CaptureStartOptionsDiagnostics::default()),
                encoder_attachment: RwLock::new(None),
                native_frame_sink: Mutex::new(None),
                #[cfg(target_os = "windows")]
                frame_sink_accepted: AtomicU64::new(0),
                #[cfg(target_os = "windows")]
                frame_sink_coalesced: AtomicU64::new(0),
                #[cfg(target_os = "windows")]
                frame_sink_rejected: AtomicU64::new(0),
                #[cfg(target_os = "windows")]
                media_frames_dropped_without_sink: AtomicU64::new(0),
                #[cfg(target_os = "windows")]
                cpu_fallback_frames_dropped: AtomicU64::new(0),
                #[cfg(target_os = "windows")]
                frame_sink_backpressure_emitted: AtomicBool::new(false),
                #[cfg(target_os = "windows")]
                frame_sink_missing_emitted: AtomicBool::new(false),
                #[cfg(target_os = "windows")]
                cpu_fallback_emitted: AtomicBool::new(false),
            }),
        }
    }

    #[napi(js_name = "setLifecycleCallback")]
    pub fn set_lifecycle_callback(&self, callback: Function<(String, String), ()>) -> Result<()> {
        let tsfn: LifecycleTsfn = callback
            .build_threadsafe_function::<(String, String)>()
            .weak::<true>()
            .callee_handled::<false>()
            .max_queue_size::<LIFECYCLE_QUEUE_LIMIT>()
            .build()
            .map(Arc::new)?;
        let mut guard = self.inner.lifecycle_tsfn.lock();
        *guard = Some(tsfn);
        Ok(())
    }

    #[cfg(target_os = "windows")]
    #[napi(js_name = "setCpuFrameCallback")]
    pub fn set_cpu_frame_callback(&self, callback: Option<Function<CpuFrame, ()>>) -> Result<()> {
        if self.inner.running.load(Ordering::Acquire) {
            return Err(napi::Error::from_reason(
                "CPU frame callback must be set before capture starts",
            ));
        }
        let tsfn = callback
            .map(|callback| {
                callback
                    .build_threadsafe_function::<QueuedCpuFrame>()
                    .weak::<true>()
                    .callee_handled::<false>()
                    .max_queue_size::<0>()
                    .build_callback(|context| Ok(context.value.into_js_frame()))
                    .map(Arc::new)
            })
            .transpose()?;
        *self.inner.cpu_frame_tsfn.lock() = tsfn;
        Ok(())
    }

    #[napi(js_name = "setFrameSinkHandle")]
    pub fn set_frame_sink_handle(&self, frame_sink_handle: Unknown<'_>) -> Result<()> {
        let sink = retain_native_frame_sink_handle(frame_sink_handle)?;
        let mut guard = self.inner.native_frame_sink.lock();
        *guard = Some(sink);
        Ok(())
    }

    #[napi]
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &self,
        source_id: String,
        source_kind: String,
        width: Option<u32>,
        height: Option<u32>,
        frame_rate: Option<u32>,
        capture_id: Option<String>,
        start_options: Option<ScreenCaptureStartOptions>,
    ) -> Result<CaptureStartResult> {
        let start_options = record_start_options(&self.inner, start_options)?;
        let normalized_capture_id = capture_id
            .map(|raw| raw.trim().to_string())
            .filter(|trimmed| !trimmed.is_empty());
        {
            let mut guard = self.inner.capture_id.lock();
            *guard = normalized_capture_id;
        }
        #[cfg(target_os = "windows")]
        {
            self.start_windows(
                source_id,
                source_kind,
                width,
                height,
                frame_rate,
                start_options,
            )
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (
                source_id,
                source_kind,
                width,
                height,
                frame_rate,
                start_options,
            );
            Err(napi::Error::from_reason(
                "native game capture only supported on Windows",
            ))
        }
    }

    #[napi(js_name = "getDiagnostics")]
    pub fn get_diagnostics(&self) -> Option<CaptureDiagnostics> {
        let snapshot = {
            let guard = self.inner.fallback.lock();
            guard.as_ref().map(|tracker| tracker.snapshot())
        }?;

        #[cfg(target_os = "windows")]
        {
            let frame_sink = frame_sink_counter_snapshot(&self.inner);
            let cpu_pipeline = self
                .inner
                .cpu_frame_tsfn
                .lock()
                .as_ref()
                .map(|_| self.inner.cpu_pipeline.snapshot());
            Some(strategy_only_diagnostics(
                &snapshot,
                current_start_options(&self.inner),
                frame_sink,
                cpu_pipeline,
            ))
        }

        #[cfg(not(target_os = "windows"))]
        Some(strategy_only_diagnostics(
            &snapshot,
            current_start_options(&self.inner),
            FrameSinkCounterSnapshot {
                accepted: 0,
                coalesced: 0,
                rejected: 0,
                dropped_without_sink: 0,
                cpu_fallback_dropped: 0,
            },
            None,
        ))
    }

    #[napi(js_name = "getFrameSinkDiagnostics")]
    pub fn get_frame_sink_diagnostics(&self) -> FrameSinkDiagnostics {
        #[cfg(target_os = "windows")]
        {
            frame_sink_diagnostics_from(frame_sink_counter_snapshot(&self.inner))
        }
        #[cfg(not(target_os = "windows"))]
        {
            FrameSinkDiagnostics {
                accepted: 0.0,
                coalesced: 0.0,
                rejected: 0.0,
                media_frames_dropped_without_sink: 0.0,
                cpu_fallback_frames_dropped: 0.0,
            }
        }
    }

    #[napi]
    pub fn stop(&self) -> Result<()> {
        self.inner
            .running
            .store(false, std::sync::atomic::Ordering::Release);
        self.inner.capture_id.lock().take();
        self.inner.native_frame_sink.lock().take();
        #[cfg(target_os = "windows")]
        self.inner.cpu_frame_tsfn.lock().take();
        if let Some(attachment) = self.inner.encoder_attachment.write().take() {
            attachment.detach();
        }
        #[cfg(target_os = "windows")]
        {
            let mut guard = self.inner.session.lock();
            *guard = None;
            let mut wgc_guard = self.inner.wgc_session.lock();
            *wgc_guard = None;
        }
        {
            let mut fallback_guard = self.inner.fallback.lock();
            *fallback_guard = None;
        }
        Ok(())
    }

    #[napi(js_name = "attachEncoder")]
    pub fn attach_encoder(&self, width: u32, height: u32, frame_rate: Option<u32>) -> Result<()> {
        if width == 0 || height == 0 {
            return Err(napi::Error::new(
                Status::InvalidArg,
                "ScreenCapture.attachEncoder requires positive dimensions",
            ));
        }
        let frame_rate = EncoderFrameRate::from_fps(frame_rate.unwrap_or(30));
        let attachment = EncoderAttachment::try_new_with_frame_rate(width, height, frame_rate)
            .map_err(|e| {
                napi::Error::new(Status::GenericFailure, format!("attachEncoder failed: {e}"))
            })?;
        *self.inner.encoder_attachment.write() = Some(attachment);
        emit_lifecycle(
            &self.inner,
            "diagnostic",
            &format!(
                "encoder ring attached: {width}x{height}@{}fps, capacity=8",
                frame_rate.numerator
            ),
        );
        Ok(())
    }

    #[napi(js_name = "detachEncoder")]
    pub fn detach_encoder(&self) -> Result<()> {
        if let Some(attachment) = self.inner.encoder_attachment.write().take() {
            attachment.detach();
        }
        emit_lifecycle(&self.inner, "diagnostic", "encoder ring detached");
        Ok(())
    }

    #[napi(js_name = "isEncoderAttached")]
    pub fn is_encoder_attached(&self) -> bool {
        self.inner
            .encoder_attachment
            .read()
            .as_ref()
            .map(|attachment| attachment.is_attached())
            .unwrap_or(false)
    }

    #[napi(js_name = "encoderRingFullCount")]
    pub fn encoder_ring_full_count(&self) -> u32 {
        self.inner
            .encoder_attachment
            .read()
            .as_ref()
            .map(|attachment| attachment.stats().ring_full_events.min(u32::MAX as u64) as u32)
            .unwrap_or(0)
    }

    #[napi(js_name = "getEncoderAttachDiagnostics")]
    pub fn get_encoder_attach_diagnostics(&self) -> Option<EncoderAttachDiagnostics> {
        let guard = self.inner.encoder_attachment.read();
        let attachment = guard.as_ref()?;
        let stats = attachment.stats();
        Some(EncoderAttachDiagnostics {
            attached: attachment.is_attached(),
            width: attachment.width(),
            height: attachment.height(),
            capacity: attachment.capacity().min(u32::MAX as usize) as u32,
            frames_submitted: stats.frames_submitted as f64,
            frames_dropped: stats.frames_dropped as f64,
            ring_full_events: stats.ring_full_events as f64,
            failed_blits: stats.failed_blits as f64,
        })
    }
}

fn record_start_options(
    inner: &CaptureInner,
    options: Option<ScreenCaptureStartOptions>,
) -> Result<CaptureStartOptionsDiagnostics> {
    let state = build_start_option_diagnostics(options.unwrap_or_default())?;
    if !state.unsupported_options.is_empty() {
        emit_lifecycle(
            inner,
            "diagnostic",
            &format!(
                "Windows capture start options currently unsupported: {}",
                state.unsupported_options.join(", ")
            ),
        );
    }
    let mut guard = inner.start_options.lock();
    *guard = state.clone();
    Ok(state)
}

fn current_start_options(inner: &CaptureInner) -> CaptureStartOptionsDiagnostics {
    inner.start_options.lock().clone()
}

fn build_start_option_diagnostics(
    options: ScreenCaptureStartOptions,
) -> Result<CaptureStartOptionsDiagnostics> {
    validate_capture_rect(options.capture_rect.as_ref())?;
    validate_enum_option(
        options.color_range.as_deref(),
        "colorRange",
        &["full", "limited"],
    )?;
    validate_enum_option(
        options.color_space.as_deref(),
        "colorSpace",
        &["rec709", "srgb"],
    )?;

    let mut unsupported_options = Vec::with_capacity(START_OPTION_UNSUPPORTED_LIMIT);
    if options.show_cursor_clicks.is_some() {
        unsupported_options.push("showCursorClicks".to_string());
    }
    if options.capture_rect.is_some() {
        unsupported_options.push("captureRect".to_string());
    }
    if options.color_range.is_some() {
        unsupported_options.push("colorRange".to_string());
    }
    if options.color_space.is_some() {
        unsupported_options.push("colorSpace".to_string());
    }
    assert!(
        unsupported_options.len() <= START_OPTION_UNSUPPORTED_LIMIT,
        "unsupported start-option list bounded"
    );

    Ok(CaptureStartOptionsDiagnostics {
        show_cursor_clicks: options.show_cursor_clicks,
        capture_rect: options.capture_rect,
        color_range: options.color_range,
        color_space: options.color_space,
        unsupported_options,
    })
}

fn validate_capture_rect(rect: Option<&ScreenCaptureRect>) -> Result<()> {
    let Some(rect) = rect else {
        return Ok(());
    };
    if !rect.x.is_finite() || !rect.y.is_finite() {
        return Err(napi::Error::new(
            Status::InvalidArg,
            "captureRect x/y must be finite numbers",
        ));
    }
    if !rect.width.is_finite() || !rect.height.is_finite() {
        return Err(napi::Error::new(
            Status::InvalidArg,
            "captureRect width/height must be finite numbers",
        ));
    }
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(napi::Error::new(
            Status::InvalidArg,
            "captureRect requires positive width and height",
        ));
    }
    Ok(())
}

fn validate_enum_option(value: Option<&str>, name: &str, allowed: &[&str]) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(napi::Error::new(
        Status::InvalidArg,
        format!("invalid {name}: {value}"),
    ))
}

fn retain_native_frame_sink_handle(
    value: Unknown<'_>,
) -> Result<Arc<fluxer_screen_frame_bus::NativeScreenFrameSinkHandleRef>> {
    if value.get_type()? != ValueType::External {
        return Err(napi::Error::new(
            Status::InvalidArg,
            "ScreenCapture.setFrameSinkHandle expects a native external frame sink handle",
        ));
    }

    let raw_value = value.value();
    let mut data: *mut c_void = std::ptr::null_mut();
    let status =
        unsafe { napi::sys::napi_get_value_external(raw_value.env, raw_value.value, &mut data) };
    if status != napi::sys::Status::napi_ok || data.is_null() {
        return Err(napi::Error::new(
            Status::InvalidArg,
            "ScreenCapture.setFrameSinkHandle received an empty native external frame sink handle",
        ));
    }

    let handle = unsafe {
        data.cast::<fluxer_screen_frame_bus::NativeScreenFrameSinkHandle>()
            .as_ref()
    }
    .and_then(fluxer_screen_frame_bus::NativeScreenFrameSinkHandle::retain_ref)
    .ok_or_else(|| {
        napi::Error::new(
            Status::InvalidArg,
            "ScreenCapture.setFrameSinkHandle received an invalid native frame sink handle",
        )
    })?;

    Ok(Arc::new(handle))
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.inner.native_frame_sink.lock().take();
        #[cfg(target_os = "windows")]
        self.inner.cpu_frame_tsfn.lock().take();
    }
}

#[cfg(target_os = "windows")]
impl ScreenCapture {
    #[allow(clippy::too_many_arguments)]
    fn start_windows(
        &self,
        source_id: String,
        source_kind: String,
        width: Option<u32>,
        height: Option<u32>,
        frame_rate: Option<u32>,
        _start_options: CaptureStartOptionsDiagnostics,
    ) -> Result<CaptureStartResult> {
        use std::sync::atomic::Ordering;

        if self.inner.running.load(Ordering::Acquire) {
            return Err(napi::Error::from_reason("Capture already running"));
        }
        self.inner.cpu_pipeline.reset();

        let target_frame_rate = frame_rate.unwrap_or(30).clamp(1, 144);

        let frame_interval =
            std::time::Duration::from_nanos(1_000_000_000 / target_frame_rate as u64);

        if source_kind == "screen" {
            let monitor = wgc_capture::parse_monitor_source_id(&source_id, &source_kind)
                .ok_or_else(|| {
                    napi::Error::from_reason(format!("Invalid source: {source_kind}:{source_id}"))
                })?;
            if !wgc_capture::wgc_capture_supported() {
                return Err(napi::Error::from_reason(
                    "Windows Graphics Capture is unavailable for screen capture",
                ));
            }
            let session = WgcCaptureSession::new_monitor(monitor, width, height).map_err(|e| {
                napi::Error::from_reason(format!("Failed to create WGC screen capture: {e}"))
            })?;
            return self.start_windows_wgc_session(session, target_frame_rate);
        }

        let hwnd = if source_kind == "game" {
            let target = capture_target::resolve_game_capture_target(&source_id, &source_kind)
                .map_err(|e| {
                    napi::Error::from_reason(format!("Failed to resolve game capture target: {e}"))
                })?;
            windows::Win32::Foundation::HWND(target as *mut _)
        } else {
            dxgi_capture::parse_window_source_id(&source_id, &source_kind).ok_or_else(|| {
                napi::Error::from_reason(format!("Invalid source: {source_kind}:{source_id}"))
            })?
        };

        if let Some(result) = self.try_start_windows_wgc(hwnd, width, height, target_frame_rate)? {
            return Ok(result);
        }

        if self.inner.cpu_frame_tsfn.lock().is_some() {
            return Err(napi::Error::from_reason(
                "CPU frame callback requires Windows Graphics Capture",
            ));
        }

        let session = DxgiCaptureSession::new(hwnd, width, height)
            .map_err(|e| napi::Error::from_reason(format!("Failed to create DXGI capture: {e}")))?;

        let capture_width = session.capture_width();
        let capture_height = session.capture_height();

        {
            let mut guard = self.inner.session.lock();
            *guard = Some(session);
        }
        {
            let mut guard = self.inner.fallback.lock();
            *guard = Some(fallback::FallbackTracker::new(
                fallback::CaptureStrategy::DxgiDuplication,
            ));
        }

        self.inner.running.store(true, Ordering::Release);

        let inner = Arc::clone(&self.inner);

        std::thread::Builder::new()
            .name("dxgi-capture".into())
            .spawn(move || {
                dxgi_capture::capture_loop(&inner, frame_interval);
            })
            .map_err(|e| {
                napi::Error::from_reason(format!("Failed to spawn capture thread: {e}"))
            })?;

        Ok(CaptureStartResult {
            width: capture_width,
            height: capture_height,
            frame_rate: target_frame_rate,
            pixel_format: "bgra".to_string(),
        })
    }

    fn try_start_windows_wgc(
        &self,
        hwnd: windows::Win32::Foundation::HWND,
        width: Option<u32>,
        height: Option<u32>,
        target_frame_rate: u32,
    ) -> Result<Option<CaptureStartResult>> {
        assert!(target_frame_rate >= 1, "frame rate at least 1");
        assert!(target_frame_rate <= 144, "frame rate at most 144");
        if !wgc_capture::wgc_capture_supported() {
            return Ok(None);
        }
        let session = match WgcCaptureSession::new(hwnd, width, height) {
            Ok(session) => session,
            Err(e) => {
                emit_lifecycle(
                    &self.inner,
                    "diagnostic",
                    &format!("WGC window capture unavailable; using DXGI duplication: {e}"),
                );
                return Ok(None);
            }
        };
        self.start_windows_wgc_session(session, target_frame_rate)
            .map(Some)
    }

    fn start_windows_wgc_session(
        &self,
        session: WgcCaptureSession,
        target_frame_rate: u32,
    ) -> Result<CaptureStartResult> {
        assert!(target_frame_rate >= 1, "frame rate at least 1");
        assert!(target_frame_rate <= 144, "frame rate at most 144");
        let capture_width = session.capture_width();
        let capture_height = session.capture_height();

        {
            let mut guard = self.inner.wgc_session.lock();
            *guard = Some(session);
        }
        {
            let mut guard = self.inner.fallback.lock();
            *guard = Some(fallback::FallbackTracker::new(
                fallback::CaptureStrategy::Wgc,
            ));
        }

        self.inner.running.store(true, Ordering::Release);

        let inner = Arc::clone(&self.inner);
        let frame_interval =
            std::time::Duration::from_nanos(1_000_000_000 / target_frame_rate as u64);

        std::thread::Builder::new()
            .name("wgc-capture".into())
            .spawn(move || {
                wgc_capture::capture_loop(&inner, frame_interval);
            })
            .map_err(|e| {
                napi::Error::from_reason(format!("Failed to spawn WGC capture thread: {e}"))
            })?;

        Ok(CaptureStartResult {
            width: capture_width,
            height: capture_height,
            frame_rate: target_frame_rate,
            pixel_format: if self.inner.cpu_frame_tsfn.lock().is_some() {
                "nv12".to_string()
            } else {
                "bgra".to_string()
            },
        })
    }
}

fn strategy_only_diagnostics(
    snapshot: &fallback::FallbackSnapshot,
    start_options: CaptureStartOptionsDiagnostics,
    frame_sink: FrameSinkCounterSnapshot,
    cpu_pipeline: Option<CpuPipelineDiagnostics>,
) -> CaptureDiagnostics {
    CaptureDiagnostics {
        state: 0,
        api_type: 0,
        transport: 0,
        fallback_reason: 0,
        capture_flags: 0,
        width: 0,
        height: 0,
        dxgi_format: 0,
        frame_counter: 0.0,
        dropped_frame_counter: 0.0,
        last_present_timestamp_us: 0.0,
        last_error: 0,
        active_strategy: snapshot.active_strategy.clone(),
        last_fallback_reason: snapshot.last_fallback_reason.clone(),
        start_options,
        frame_sink_accepted: frame_sink.accepted as f64,
        frame_sink_coalesced: frame_sink.coalesced as f64,
        frame_sink_rejected: frame_sink.rejected as f64,
        media_frames_dropped_without_sink: frame_sink.dropped_without_sink as f64,
        cpu_fallback_frames_dropped: frame_sink.cpu_fallback_dropped as f64,
        cpu_pipeline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_options_are_kept_as_explicit_unsupported_state() {
        let state = build_start_option_diagnostics(ScreenCaptureStartOptions {
            show_cursor_clicks: Some(true),
            capture_rect: Some(ScreenCaptureRect {
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 200.0,
            }),
            color_range: Some("full".to_string()),
            color_space: Some("rec709".to_string()),
        })
        .expect("valid options");

        assert_eq!(state.show_cursor_clicks, Some(true));
        assert_eq!(state.color_range.as_deref(), Some("full"));
        assert_eq!(state.color_space.as_deref(), Some("rec709"));
        assert_eq!(
            state.unsupported_options,
            vec![
                "showCursorClicks".to_string(),
                "captureRect".to_string(),
                "colorRange".to_string(),
                "colorSpace".to_string(),
            ]
        );
    }

    #[test]
    fn capture_rect_requires_positive_dimensions() {
        let err = build_start_option_diagnostics(ScreenCaptureStartOptions {
            capture_rect: Some(ScreenCaptureRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 10.0,
            }),
            ..ScreenCaptureStartOptions::default()
        })
        .err();
        assert!(err.is_some(), "invalid captureRect is rejected");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_pipeline_diagnostics_aggregate_stage_timings_and_reset() {
        let stats = CpuPipelineStats::default();
        stats.record_acquired();
        stats.record_acquired();
        stats.record_coalesced();
        stats.record_permit_rejected();
        stats.configure(1920, 1080, true);
        stats.record_conversion(std::time::Duration::from_millis(2));
        stats.record_conversion(std::time::Duration::from_millis(5));
        stats.record_readback_map(std::time::Duration::from_millis(3));
        stats.record_cpu_pack(std::time::Duration::from_micros(500));

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.frames_acquired, 2.0);
        assert_eq!(snapshot.frames_coalesced, 1.0);
        assert_eq!(snapshot.permit_rejected_frames, 1.0);
        assert_eq!(snapshot.conversion_count, 2.0);
        assert_eq!(snapshot.conversion_total_ms, 7.0);
        assert_eq!(snapshot.conversion_max_ms, 5.0);
        assert_eq!(snapshot.readback_map_count, 1.0);
        assert_eq!(snapshot.readback_map_total_ms, 3.0);
        assert_eq!(snapshot.cpu_pack_count, 1.0);
        assert_eq!(snapshot.cpu_pack_total_ms, 0.5);
        assert_eq!(snapshot.input_width, 1920);
        assert_eq!(snapshot.input_height, 1080);
        assert!(snapshot.hdr_tone_map_enabled);

        stats.reset();
        let reset = stats.snapshot();
        assert_eq!(reset.frames_acquired, 0.0);
        assert_eq!(reset.conversion_count, 0.0);
        assert_eq!(reset.conversion_total_ms, 0.0);
        assert_eq!(reset.input_width, 0);
        assert!(!reset.hdr_tone_map_enabled);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_pipeline_recent_diagnostics_expire_by_bucket() {
        let stats = CpuPipelineStats::default();
        let start = std::time::Instant::now();
        stats.reset_at(start);
        stats.record_acquired_at(start);
        stats.record_acquired_at(start + std::time::Duration::from_millis(99));
        stats.record_coalesced_at(start + std::time::Duration::from_millis(100));
        stats.record_conversion_at(
            std::time::Duration::from_millis(4),
            start + std::time::Duration::from_millis(100),
        );
        stats.record_conversion_at(
            std::time::Duration::from_millis(9),
            start + std::time::Duration::from_millis(150),
        );
        stats.record_readback_map_at(
            std::time::Duration::from_millis(6),
            start + std::time::Duration::from_millis(150),
        );
        stats.record_cpu_pack_at(
            std::time::Duration::from_micros(500),
            start + std::time::Duration::from_millis(150),
        );
        stats.record_permit_rejected_at(start + std::time::Duration::from_millis(200));
        stats.record_hdr_white_query_at(
            std::time::Duration::from_millis(25),
            start + std::time::Duration::from_millis(500),
        );

        let current = stats.snapshot_at(start + std::time::Duration::from_secs(5));
        let recent = current.recent5_seconds;
        assert_eq!(recent.window_ms, 5_000.0);
        assert_eq!(recent.bucket_ms, 100.0);
        assert_eq!(recent.frames_acquired, 2.0);
        assert_eq!(recent.frames_coalesced, 1.0);
        assert_eq!(recent.permit_rejected_frames, 1.0);
        assert_eq!(recent.conversion_count, 2.0);
        assert_eq!(recent.conversion_total_ms, 13.0);
        assert_eq!(recent.conversion_max_ms, 9.0);
        assert_eq!(recent.readback_map_max_ms, 6.0);
        assert_eq!(recent.cpu_pack_max_ms, 0.5);
        assert_eq!(recent.hdr_white_query_count, 1.0);
        assert_eq!(recent.hdr_white_query_max_ms, 25.0);

        let expired = stats.snapshot_at(start + std::time::Duration::from_millis(5_100));
        assert_eq!(expired.recent5_seconds.frames_acquired, 0.0);
        assert_eq!(expired.recent5_seconds.frames_coalesced, 1.0);
        assert_eq!(expired.recent5_seconds.conversion_count, 2.0);

        let idle = stats.snapshot_at(start + std::time::Duration::from_secs(10));
        assert_eq!(idle.recent5_seconds.frames_coalesced, 0.0);
        assert_eq!(idle.recent5_seconds.permit_rejected_frames, 0.0);
        assert_eq!(idle.recent5_seconds.conversion_count, 0.0);
        assert_eq!(idle.recent5_seconds.conversion_total_ms, 0.0);
        assert_eq!(idle.recent5_seconds.readback_map_max_ms, 0.0);
        assert_eq!(idle.recent5_seconds.cpu_pack_max_ms, 0.0);
        assert_eq!(idle.recent5_seconds.hdr_white_query_count, 0.0);
        assert_eq!(idle.recent5_seconds.hdr_white_query_max_ms, 0.0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_pipeline_recent_diagnostics_reset_for_new_capture() {
        let stats = CpuPipelineStats::default();
        let start = std::time::Instant::now();
        stats.reset_at(start);
        stats.record_acquired_at(start);
        stats.record_hdr_white_query_at(
            std::time::Duration::from_millis(30),
            start + std::time::Duration::from_millis(100),
        );

        let next_capture = start + std::time::Duration::from_secs(1);
        stats.reset_at(next_capture);
        let snapshot = stats.snapshot_at(next_capture);
        assert_eq!(snapshot.recent5_seconds.window_ms, 1.0);
        assert_eq!(snapshot.recent5_seconds.frames_acquired, 0.0);
        assert_eq!(snapshot.recent5_seconds.hdr_white_query_count, 0.0);
        assert_eq!(snapshot.recent5_seconds.hdr_white_query_max_ms, 0.0);
        assert_eq!(
            stats
                .snapshot_at(next_capture + std::time::Duration::from_secs(1))
                .recent5_seconds
                .window_ms,
            1_000.0,
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_pipeline_recent_diagnostics_keep_at_most_51_buckets() {
        let stats = CpuPipelineStats::default();
        let start = std::time::Instant::now();
        stats.reset_at(start);
        for bucket in 0..60 {
            stats.record_acquired_at(start + std::time::Duration::from_millis(bucket * 100));
        }

        let recent = stats.snapshot_at(start + std::time::Duration::from_millis(5_900));
        assert_eq!(stats.recent.lock().buckets.len(), 51);
        assert_eq!(recent.recent5_seconds.frames_acquired, 51.0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_frame_pending_gate_allows_two_frames_and_skips_work_when_full() {
        let pending = Arc::new(AtomicUsize::new(0));
        let accepted = try_convert_cpu_frame(&pending, || Ok::<_, &'static str>(7)).unwrap();
        let Some((frame, permit)) = accepted else {
            panic!("first CPU frame should be accepted");
        };
        assert_eq!(frame, 7);

        let second = try_convert_cpu_frame(&pending, || Ok::<_, &'static str>(8))
            .unwrap()
            .expect("a second bounded slot absorbs a short JS dispatch delay");

        let mut rejected_conversion_ran = false;
        let rejected = try_convert_cpu_frame(&pending, || {
            rejected_conversion_ran = true;
            Ok::<_, &'static str>(9)
        })
        .unwrap();
        assert!(rejected.is_none());
        assert!(
            !rejected_conversion_ran,
            "rejected CPU frames must skip readback work"
        );

        drop(permit);
        assert!(
            try_convert_cpu_frame(&pending, || Ok::<_, &'static str>(9))
                .unwrap()
                .is_some()
        );
        drop(second);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_frame_conversion_error_releases_pending_permit() {
        let pending = Arc::new(AtomicUsize::new(0));
        let result: std::result::Result<Option<((), CpuFramePermit)>, &'static str> =
            try_convert_cpu_frame(&pending, || Err::<(), _>("readback failed"));

        assert!(result.is_err());
        assert_eq!(pending.load(Ordering::Acquire), 0);
        assert!(
            try_convert_cpu_frame(&pending, || Ok::<_, &'static str>(()))
                .unwrap()
                .is_some()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_frame_error_releases_only_its_own_slot() {
        let pending = Arc::new(AtomicUsize::new(0));
        let held = CpuFramePermit::try_acquire(&pending).unwrap();
        let failed = try_convert_cpu_frame(&pending, || Err::<(), _>("readback failed"));
        assert!(failed.is_err());
        assert_eq!(pending.load(Ordering::Acquire), 1);
        drop(held);
        assert_eq!(pending.load(Ordering::Acquire), 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_frame_dispatch_bound_holds_under_concurrent_acquisition() {
        let pending = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(std::sync::Barrier::new(9));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let pending = Arc::clone(&pending);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let permit = CpuFramePermit::try_acquire(&pending);
                    barrier.wait();
                    barrier.wait();
                    drop(permit);
                })
            })
            .collect();
        barrier.wait();
        assert_eq!(pending.load(Ordering::Acquire), CPU_FRAME_PENDING_LIMIT);
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(pending.load(Ordering::Acquire), 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cpu_frame_callback_conversion_releases_pending_permit() {
        let pending = Arc::new(AtomicUsize::new(0));
        let (_, permit) = try_convert_cpu_frame(&pending, || Ok::<_, &'static str>(()))
            .unwrap()
            .expect("frame permit");
        let payload = QueuedCpuFrame {
            frame: CpuFrame {
                width: 2,
                height: 2,
                pixel_format: "nv12".to_string(),
                timestamp_us: 1,
                data: vec![0; 6].into(),
            },
            _permit: permit,
        };

        let frame = payload.into_js_frame();
        assert_eq!(frame.width, 2);
        assert_eq!(pending.load(Ordering::Acquire), 0);
    }
}

#[napi(js_name = "isSupported")]
pub fn is_supported() -> bool {
    cfg!(target_os = "windows")
}

#[napi(js_name = "getAvailability")]
pub fn get_availability() -> AvailabilityInfo {
    AvailabilityInfo {
        available: cfg!(target_os = "windows"),
        backend: "windows-game-capture".to_string(),
        reason: if cfg!(target_os = "windows") {
            None
        } else {
            Some("unsupported-platform".to_string())
        },
    }
}

#[napi(js_name = "listSources")]
pub fn list_sources() -> Result<Vec<ScreenCaptureSourceDescriptor>> {
    Ok(sources::list_sources())
}

#[napi(js_name = "elevateGpuSchedulingPriority")]
pub fn elevate_gpu_scheduling_priority(
    process_id: Option<u32>,
    priority_class: Option<String>,
) -> Result<()> {
    gpu_priority::elevate(process_id, priority_class).map_err(napi::Error::from_reason)
}

#[napi(js_name = "restoreGpuSchedulingPriority")]
pub fn restore_gpu_scheduling_priority(process_id: Option<u32>) -> Result<()> {
    gpu_priority::restore(process_id).map_err(napi::Error::from_reason)
}
