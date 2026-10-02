// SPDX-License-Identifier: AGPL-3.0-or-later

import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest';

const state = vi.hoisted(() => ({
	startCpu: vi.fn(),
	stopCpu: vi.fn(),
	cpuFrameRateFiltering: false,
	startOptions: null as Record<string, unknown> | null,
	writeFrame: vi.fn(),
	onFrame: null as
		| ((frame: {pixelFormat: string; width: number; height: number; data: Uint8Array; timestampUs: number}) => void)
		| null,
	writtenTimestamps: [] as Array<number>,
	colorSpace: undefined as VideoColorSpaceInit | undefined,
}));

vi.mock('@app/features/voice/utils/native_audio_capture_bridge/shared', () => ({
	patchTrackStopForCleanup: () => () => {},
}));
vi.mock('@app/features/voice/utils/native_screen_capture_bridge/shared', () => ({
	getGeneratorVideoCtor: () =>
		class {
			writable = {
				getWriter: () => ({
					desiredSize: 1,
					write: (frame: {timestamp: number}) => state.writeFrame(frame),
					close: async () => {},
				}),
			};
			stop() {}
		},
	getNativeScreenCaptureApi: () => ({
		startCpu: state.startCpu,
		stopCpu: state.stopCpu,
		cpuFrameRateFiltering: state.cpuFrameRateFiltering,
	}),
	getVideoFrameCtor: () =>
		class {
			timestamp: number;
			constructor(_data: Uint8Array, init: {timestamp: number; colorSpace?: VideoColorSpaceInit}) {
				this.timestamp = init.timestamp;
				state.colorSpace = init.colorSpace;
			}
			close() {}
		},
	markNativeScreenShareTrack: vi.fn(),
}));

import {createCpuFrameBridge, createCpuFrameRateFilter, getCpuFrameBridgeStats} from './createCpuFrameBridge';

async function runCapture(
	captureRate: number,
	outputRate: number,
	timestamps: Array<number>,
	preloadFilterActive = false,
): Promise<Array<number>> {
	state.writtenTimestamps = [];
	state.cpuFrameRateFiltering = preloadFilterActive;
	const bridge = await createCpuFrameBridge(
		{
			sourceId: 'window:1234:0',
			sourceKind: 'window',
			width: 16,
			height: 16,
			frameRate: captureRate,
		},
		outputRate,
	);
	for (const timestampUs of timestamps) {
		state.onFrame?.({
			pixelFormat: 'nv12',
			width: 16,
			height: 16,
			data: new Uint8Array((16 * 16 * 3) / 2),
			timestampUs,
		});
		await Promise.resolve();
		await Promise.resolve();
	}
	await bridge.cleanup();
	return [...state.writtenTimestamps];
}

function makeJitteredTimestamps(frameCount: number, frameRate: number): Array<number> {
	const jitterUs = [0, 350, -250, 500, -400, 100];
	return Array.from(
		{length: frameCount},
		(_, index) => Math.round((index * 1_000_000) / frameRate) + jitterUs[index % jitterUs.length],
	);
}

describe('CPU frame bridge output pacing', () => {
	beforeEach(() => {
		state.onFrame = null;
		state.writeFrame
			.mockReset()
			.mockImplementation(async (frame: {timestamp: number}) => state.writtenTimestamps.push(frame.timestamp));
		state.colorSpace = undefined;
		state.startCpu.mockReset().mockImplementation(async (_options, onFrame) => {
			state.startOptions = _options;
			state.onFrame = onFrame;
			return {captureId: 'capture-1', width: 16, height: 16, frameRate: 144, pixelFormat: 'nv12'};
		});
		state.stopCpu.mockReset().mockResolvedValue(undefined);
		state.cpuFrameRateFiltering = false;
		state.startOptions = null;
	});
	afterEach(() => vi.clearAllMocks());

	it('keeps overlapping captures out of the latest bridge window', async () => {
		const options = {sourceId: 'window:1234:0', sourceKind: 'window' as const, width: 16, height: 16, frameRate: 120};
		const oldBridge = await createCpuFrameBridge(options, 120);
		const oldOnFrame = state.onFrame;
		const newBridge = await createCpuFrameBridge(options, 120);
		try {
			const frame = {pixelFormat: 'nv12', width: 16, height: 16, data: new Uint8Array(384), timestampUs: 1000};
			oldOnFrame?.(frame);
			state.onFrame?.(frame);
			await Promise.resolve();
			await Promise.resolve();
			await Promise.resolve();
			expect(getCpuFrameBridgeStats()?.framesReceived).toBe(1);
			expect(getCpuFrameBridgeStats()?.recent5Seconds.received).toBe(1);
			expect(getCpuFrameBridgeStats()?.recent5Seconds.forwarded).toBe(1);
		} finally {
			await oldBridge.cleanup();
			await newBridge.cleanup();
		}
	});

	it('reports only recent five seconds and expires older frames', async () => {
		let now = 0;
		const clock = vi.spyOn(Date, 'now').mockImplementation(() => now);
		const bridge = await createCpuFrameBridge(
			{sourceId: 'window:1234:0', sourceKind: 'window', width: 16, height: 16, frameRate: 120},
			120,
		);
		try {
			for (const time of [1000, 6000]) {
				now = time;
				state.onFrame?.({
					pixelFormat: 'nv12',
					width: 16,
					height: 16,
					data: new Uint8Array(384),
					timestampUs: time * 1000,
				});
				await Promise.resolve();
				await Promise.resolve();
				await Promise.resolve();
			}
			expect(getCpuFrameBridgeStats()?.framesReceived).toBe(2);
			expect(getCpuFrameBridgeStats()?.recent5Seconds.received).toBe(1);
			expect(getCpuFrameBridgeStats()?.recent5Seconds.forwarded).toBe(1);
			now = 11001;
			expect(getCpuFrameBridgeStats()?.recent5Seconds.received).toBe(0);
		} finally {
			await bridge.cleanup();
			clock.mockRestore();
		}
	});

	it('separates delayed JS arrivals from regular source frame timestamps', async () => {
		let now = 0;
		const clock = vi.spyOn(performance, 'now').mockImplementation(() => now);
		const bridge = await createCpuFrameBridge(
			{sourceId: 'window:1234:0', sourceKind: 'window', width: 16, height: 16, frameRate: 120},
			120,
		);
		try {
			for (const [arrivalMs, timestampUs] of [
				[0, 0],
				[40, 8333],
				[41, 16667],
			]) {
				now = arrivalMs;
				state.onFrame?.({pixelFormat: 'nv12', width: 16, height: 16, data: new Uint8Array(384), timestampUs});
				await Promise.resolve();
				await Promise.resolve();
				await Promise.resolve();
			}
			const recent = getCpuFrameBridgeStats()?.recent5Seconds;
			expect(recent?.maxArrivalGapMs).toBe(40);
			expect(recent?.shortArrivalGaps).toBe(1);
			expect(recent?.maxGapMs).toBeCloseTo(8.334);
			expect(recent?.shortGaps).toBe(0);
		} finally {
			await bridge.cleanup();
			clock.mockRestore();
		}
	});

	it('labels native G22 NV12 with its sRGB transfer and limited Rec.709 matrix', async () => {
		await runCapture(120, 120, [0]);
		expect(state.colorSpace).toEqual({
			primaries: 'bt709',
			transfer: 'iec61966-2-1',
			matrix: 'bt709',
			fullRange: false,
		});
	});

	it('keeps monotonic timestamps without thinning again after preload filtering', async () => {
		const output = await runCapture(144, 120, [0, 1000, 1000, 2000], true);

		expect(output).toEqual([0, 1000, 2000]);
		expect(state.startOptions).toMatchObject({frameRate: 144, outputFrameRate: 120});
		expect(getCpuFrameBridgeStats()?.rateFilterLocation).toBe('preload');
	});

	it('keeps jittered 144 FPS capture near the requested 120 FPS output rate', async () => {
		const output = await runCapture(144, 120, makeJitteredTimestamps(289, 144));
		const outputRate = ((output.length - 1) * 1_000_000) / (output.at(-1)! - output[0]!);

		expect(state.startOptions).not.toHaveProperty('outputFrameRate');
		expect(getCpuFrameBridgeStats()?.rateFilterLocation).toBe('renderer');
		expect(outputRate).toBeGreaterThanOrEqual(119.5);
		expect(outputRate).toBeLessThanOrEqual(120.5);
	});

	it('does not catch up after a capture stall', async () => {
		const output = await runCapture(144, 120, [0, 1_000_000, 1_000_100, 1_006_944, 1_008_400]);

		expect(output).toEqual([0, 1_000_000, 1_008_400]);
	});

	it('bounds spare credit below the stall boundary and clears it at the boundary', () => {
		for (const gapUs of [249_000, 250_000]) {
			const filter = createCpuFrameRateFilter(144, 120);
			filter(0);
			const forwarded = Array.from({length: 20}, (_, index) => gapUs + index * 100).filter(filter);
			// This budget can admit at most two extra NEW frames; no buffered frames are replayed.
			expect(forwarded.length).toBe(gapUs < 250_000 ? 3 : 1);
		}
	});

	it('preserves near-120 delivery with uneven frame intervals', async () => {
		const timestamps = Array.from({length: 241}, (_, index) => index * 8333 + (index % 2 ? -3000 : 0));
		const output = await runCapture(144, 120, timestamps);
		expect(output.length).toBeGreaterThanOrEqual(239);
		expect(output.length).toBeLessThanOrEqual(241);
	});

	it('keeps a slower native stream instead of discarding its accumulated frame budget', async () => {
		const timestamps = Array.from({length: 201}, (_, index) => index * 10_000 + (index % 2 ? -4000 : 0));
		const output = await runCapture(144, 120, timestamps);
		expect(output.length).toBeGreaterThanOrEqual(199);
	});

	it('does not treat ordinary coalesced native gaps as a stopped capture', async () => {
		const timestamps = [0];
		for (let index = 0; index < 200; index++) {
			for (const gapUs of [6944, 6944, 20_833]) timestamps.push(timestamps.at(-1)! + gapUs);
		}
		const output = await runCapture(144, 120, timestamps);
		// This is only 86 FPS on average; short coalescing gaps must not cause recurring rate drops.
		expect(output.length).toBeGreaterThanOrEqual(timestamps.length - 2);
	});

	it('rejects non-increasing timestamps without spending the next valid frame budget', () => {
		const filter = createCpuFrameRateFilter(144, 120);
		expect([0, 8400, 8400, 8000, 16_800].map(filter)).toEqual([true, true, false, false, true]);
	});

	it('rejects non-increasing timestamps when capture does not need thinning', () => {
		for (const captureRate of [30, 60]) {
			const filter = createCpuFrameRateFilter(captureRate, 60);
			expect([0, 100, 100, 90, 200].map(filter)).toEqual([true, true, false, false, true]);
		}
	});

	it('bounds the rate after uneven delivery fills the spare frame budget', async () => {
		const slow = Array.from({length: 101}, (_, index) => index * 10_000);
		const fast = Array.from({length: 1001}, (_, index) => 1_000_000 + (index + 1) * 1000);
		const output = await runCapture(144, 120, [...slow, ...fast]);
		const fastOutput = output.filter((timestamp) => timestamp > 1_000_000);
		expect(fastOutput.length).toBeGreaterThanOrEqual(119);
		expect(fastOutput.length).toBeLessThanOrEqual(122);
	});

	it('keeps near-120 output when native delivery arrives in close pairs', async () => {
		const timestamps = [0];
		for (let index = 0; index < 400; index++) {
			timestamps.push(timestamps.at(-1)! + 15_000, timestamps.at(-1)! + 15_100);
		}
		const output = await runCapture(144, 120, timestamps);
		const rate = ((output.length - 1) * 1_000_000) / timestamps.at(-1)!;
		expect(rate).toBeGreaterThanOrEqual(119.5);
		expect(rate).toBeLessThanOrEqual(120.5);
	});

	it('reports rate drops without retaining frame history', async () => {
		await runCapture(144, 120, [0, 100, 8400]);
		expect(getCpuFrameBridgeStats()).toMatchObject({
			active: false,
			framesReceived: 3,
			framesForwarded: 2,
			framesDroppedForRate: 1,
			writeErrors: 0,
		});
	});

	it('reports short capture gaps separately from long stalls', async () => {
		await runCapture(144, 120, [0, 20_000, 40_000, 340_000]);
		expect(getCpuFrameBridgeStats()).toMatchObject({
			shortGapFrameCount: 2,
			captureStallCount: 1,
			maxFrameTimestampGapMs: 300,
		});
	});

	it('drops new frames while a write is pending instead of queueing a catch-up burst', async () => {
		let finishWrite!: () => void;
		state.writeFrame.mockImplementation(
			() =>
				new Promise<void>((resolve) => {
					finishWrite = resolve;
				}),
		);
		const bridge = await createCpuFrameBridge(
			{sourceId: 'window:1234:0', sourceKind: 'window', width: 16, height: 16, frameRate: 144},
			120,
		);
		for (const timestampUs of [0, 8400, 16800]) {
			state.onFrame?.({pixelFormat: 'nv12', width: 16, height: 16, data: new Uint8Array(384), timestampUs});
		}
		expect(state.writeFrame).toHaveBeenCalledTimes(1);
		expect(getCpuFrameBridgeStats()?.framesDroppedForBackpressure).toBe(2);
		finishWrite();
		await Promise.resolve();
		await Promise.resolve();
		await bridge.cleanup();
		expect(getCpuFrameBridgeStats()?.framesForwarded).toBe(1);
	});

	it('reports failed writes without counting them as forwarded frames', async () => {
		state.writeFrame.mockRejectedValue(new Error('closed'));
		await runCapture(120, 120, [0]);
		expect(getCpuFrameBridgeStats()).toMatchObject({writeErrors: 1, framesForwarded: 0});
	});

	it('preserves the requested rate when capture already runs at 60 FPS', async () => {
		const timestamps = makeJitteredTimestamps(121, 60);
		const output = await runCapture(60, 60, timestamps);

		expect(output).toEqual(timestamps);
	});
});
