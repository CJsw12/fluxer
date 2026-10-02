// SPDX-License-Identifier: AGPL-3.0-or-later

import {patchTrackStopForCleanup} from '@app/features/voice/utils/native_audio_capture_bridge/shared';
import {createCpuFrameRateFilter} from '@fluxer/voice_engine_v2/src/bridge/CpuFrameRateFilter';
import {
	getGeneratorVideoCtor,
	getNativeScreenCaptureApi,
	getVideoFrameCtor,
	markNativeScreenShareTrack,
	type NativeScreenBridgeHandle,
} from '@app/features/voice/utils/native_screen_capture_bridge/shared';
import type {NativeScreenCaptureCpuStartOptions} from '@app/types/electron.d';

export {createCpuFrameRateFilter} from '@fluxer/voice_engine_v2/src/bridge/CpuFrameRateFilter';

interface CpuFrameBridgeStats {
	active: boolean;
	captureId: string | null;
	startedAtMs: number;
	lastFrameAtMs: number | null;
	requestedCaptureFrameRate: number;
	requestedOutputFrameRate: number;
	rateFilterLocation: 'preload' | 'renderer';
	framesReceived: number;
	framesForwarded: number;
	framesDroppedForRate: number;
	framesDroppedForBackpressure: number;
	invalidFrames: number;
	writeErrors: number;
	maxWriteDurationMs: number;
	shortGapFrameCount: number;
	captureStallCount: number;
	maxFrameTimestampGapMs: number;
}

interface RecentFrameBucket {
	atMs: number;
	received: number;
	forwarded: number;
	rateDrops: number;
	backpressureDrops: number;
	shortGaps: number;
	maxGapMs: number;
	shortArrivalGaps: number;
	maxArrivalGapMs: number;
}
const CAPTURE_STALL_GAP_US = 250_000;
let latestRecentBuckets: Array<RecentFrameBucket> = [];
let latestStats: CpuFrameBridgeStats | null = null;
export function getCpuFrameBridgeStats() {
	if (!latestStats) return null;
	const now = Date.now();
	const buckets = latestRecentBuckets.filter((bucket) => bucket.atMs > now - 5000);
	const total = buckets.reduce(
		(sum, bucket) => ({
			received: sum.received + bucket.received,
			forwarded: sum.forwarded + bucket.forwarded,
			rateDrops: sum.rateDrops + bucket.rateDrops,
			backpressureDrops: sum.backpressureDrops + bucket.backpressureDrops,
			shortGaps: sum.shortGaps + bucket.shortGaps,
			maxGapMs: Math.max(sum.maxGapMs, bucket.maxGapMs),
			shortArrivalGaps: sum.shortArrivalGaps + bucket.shortArrivalGaps,
			maxArrivalGapMs: Math.max(sum.maxArrivalGapMs, bucket.maxArrivalGapMs),
		}),
		{
			received: 0,
			forwarded: 0,
			rateDrops: 0,
			backpressureDrops: 0,
			shortGaps: 0,
			maxGapMs: 0,
			shortArrivalGaps: 0,
			maxArrivalGapMs: 0,
		},
	);
	const windowMs = Math.max(1, Math.min(5000, now - latestStats.startedAtMs));
	return {
		...latestStats,
		recent5Seconds: {
			...total,
			windowMs,
			bucketMs: 100,
			receivedFps: (total.received * 1000) / windowMs,
			forwardedFps: (total.forwarded * 1000) / windowMs,
		},
	};
}

export async function createCpuFrameBridge(
	options: NativeScreenCaptureCpuStartOptions,
	outputFrameRate: number,
): Promise<NativeScreenBridgeHandle> {
	const api = getNativeScreenCaptureApi();
	const Generator = getGeneratorVideoCtor();
	const VideoFrame = getVideoFrameCtor();
	if (!api?.startCpu || !api.stopCpu || !Generator || !VideoFrame) {
		throw new Error('120 FPS native video capture is unavailable');
	}
	const preloadFilterActive = api.cpuFrameRateFiltering === true;
	const startOptions = {...options};
	delete startOptions.outputFrameRate;
	if (preloadFilterActive) startOptions.outputFrameRate = outputFrameRate;
	const track = new Generator({kind: 'video'});
	const writer = track.writable.getWriter();
	const shouldForwardFrame = createCpuFrameRateFilter(
		preloadFilterActive ? outputFrameRate : options.frameRate,
		outputFrameRate,
	);
	const stats: CpuFrameBridgeStats = {
		active: true,
		captureId: null,
		startedAtMs: Date.now(),
		lastFrameAtMs: null,
		requestedCaptureFrameRate: options.frameRate,
		requestedOutputFrameRate: outputFrameRate,
		rateFilterLocation: preloadFilterActive ? 'preload' : 'renderer',
		framesReceived: 0,
		framesForwarded: 0,
		framesDroppedForRate: 0,
		framesDroppedForBackpressure: 0,
		invalidFrames: 0,
		writeErrors: 0,
		maxWriteDurationMs: 0,
		shortGapFrameCount: 0,
		captureStallCount: 0,
		maxFrameTimestampGapMs: 0,
	};
	latestStats = stats;
	const recentBuckets: Array<RecentFrameBucket> = [];
	latestRecentBuckets = recentBuckets;
	let captureId: string | null = null;
	let stopped = false;
	let writing = false;
	let previousTimestampUs: number | null = null;
	let previousArrivalMs: number | null = null;
	const cleanup = async (stopRemote = true): Promise<void> => {
		if (stopped) return;
		stopped = true;
		stats.active = false;
		if (stopRemote && captureId) await api.stopCpu(captureId).catch(() => undefined);
		try {
			await writer.close();
		} catch {}
		try {
			track.stop();
		} catch {}
	};
	const restoreStop = patchTrackStopForCleanup(track, () => {
		void cleanup();
	});
	try {
		const started = await api.startCpu(
			startOptions,
			(frame) => {
				if (stopped) return;
				stats.framesReceived += 1;
				stats.lastFrameAtMs = Date.now();
				const atMs = Math.floor(stats.lastFrameAtMs / 100) * 100;
				let bucket = recentBuckets.at(-1);
				if (!bucket || bucket.atMs !== atMs) {
					bucket = {
						atMs,
						received: 0,
						forwarded: 0,
						rateDrops: 0,
						backpressureDrops: 0,
						shortGaps: 0,
						maxGapMs: 0,
						shortArrivalGaps: 0,
						maxArrivalGapMs: 0,
					};
					recentBuckets.push(bucket);
					if (recentBuckets.length > 51) recentBuckets.shift();
				}
				const frameBucket = bucket;
				frameBucket.received += 1;
				const arrivalMs = performance.now();
				if (previousArrivalMs !== null) {
					const gapMs = arrivalMs - previousArrivalMs;
					frameBucket.maxArrivalGapMs = Math.max(frameBucket.maxArrivalGapMs, gapMs);
					if (gapMs * outputFrameRate >= 2000 && gapMs < CAPTURE_STALL_GAP_US / 1000) frameBucket.shortArrivalGaps += 1;
				}
				previousArrivalMs = arrivalMs;
				if (previousTimestampUs !== null && frame.timestampUs > previousTimestampUs) {
					const gapUs = frame.timestampUs - previousTimestampUs;
					frameBucket.maxGapMs = Math.max(frameBucket.maxGapMs, gapUs / 1000);
					stats.maxFrameTimestampGapMs = Math.max(stats.maxFrameTimestampGapMs, gapUs / 1000);
					if (gapUs >= CAPTURE_STALL_GAP_US) stats.captureStallCount += 1;
					else if (gapUs * outputFrameRate >= 2_000_000) {
						stats.shortGapFrameCount += 1;
						frameBucket.shortGaps += 1;
					}
				}
				if (previousTimestampUs === null || frame.timestampUs > previousTimestampUs)
					previousTimestampUs = frame.timestampUs;
				if (writing || writer.desiredSize === null || writer.desiredSize <= 0) {
					stats.framesDroppedForBackpressure += 1;
					frameBucket.backpressureDrops += 1;
					return;
				}
				if (
					frame.pixelFormat !== 'nv12' ||
					frame.width <= 0 ||
					frame.height <= 0 ||
					frame.data.byteLength !== (frame.width * frame.height * 3) / 2
				) {
					stats.invalidFrames += 1;
					return;
				}
				if (!shouldForwardFrame(frame.timestampUs)) {
					stats.framesDroppedForRate += 1;
					frameBucket.rateDrops += 1;
					return;
				}
				let videoFrame: InstanceType<typeof VideoFrame>;
				try {
					videoFrame = new VideoFrame(frame.data as Uint8Array<ArrayBuffer>, {
						format: 'NV12',
						codedWidth: frame.width,
						codedHeight: frame.height,
						timestamp: frame.timestampUs,
						// Native G22 NV12 carries sRGB transfer with a limited-range Rec.709 YUV matrix.
						colorSpace: {primaries: 'bt709', transfer: 'iec61966-2-1', matrix: 'bt709', fullRange: false},
					});
				} catch {
					stats.invalidFrames += 1;
					return;
				}
				writing = true;
				const writeStartedAt = performance.now();
				let writeFailed = false;
				void writer
					.write(videoFrame)
					.catch(() => {
						writeFailed = true;
						stats.writeErrors += 1;
					})
					.finally(() => {
						if (!writeFailed) {
							stats.framesForwarded += 1;
							frameBucket.forwarded += 1;
						}
						stats.maxWriteDurationMs = Math.max(stats.maxWriteDurationMs, performance.now() - writeStartedAt);
						videoFrame.close();
						writing = false;
					});
			},
			() => {
				void cleanup(false);
			},
		);
		captureId = started.captureId;
		stats.captureId = captureId;
		if (stopped) {
			await api.stopCpu(captureId);
			throw new Error('Native screen capture ended during startup');
		}
		markNativeScreenShareTrack(track);
		return {
			track,
			cleanup: async (stopRemote = true) => {
				restoreStop();
				await cleanup(stopRemote);
			},
		};
	} catch (error) {
		restoreStop();
		await cleanup();
		throw error;
	}
}
