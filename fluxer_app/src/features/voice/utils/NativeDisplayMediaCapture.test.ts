// @vitest-environment happy-dom
// SPDX-License-Identifier: AGPL-3.0-or-later

import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest';
import {createNativeDisplayMediaStream} from './NativeDisplayMediaCapture';

const state = vi.hoisted(() => ({
	platform: 'win32',
	available: true,
	sourceId: 'window:1234:0' as string | null,
	target: {width: 1920, height: 1080, frameRate: 120, contentHint: 'motion'} as {
		width: number;
		height: number;
		frameRate: number;
		contentHint: string;
	} | null,
	track: {contentHint: '', stop: vi.fn()},
	bridge: vi.fn(),
	consume: vi.fn(),
}));

vi.mock('@app/features/ui/utils/NativeUtils', () => ({getElectronAPI: () => ({platform: state.platform})}));
vi.mock('@app/features/voice/state/DesktopSourceIntent', () => ({
	peekDesktopSourceIntent: () => (state.sourceId ? {sourceId: state.sourceId, includeAudio: false} : null),
	consumeDesktopSourceIntent: state.consume,
}));
vi.mock('@app/features/voice/state/ActiveScreenShareSource', () => ({default: {getTarget: () => state.target}}));
vi.mock('@app/features/voice/utils/native_screen_capture_bridge/shared', () => ({
	getNativeScreenCaptureApi: () => (state.available ? {startCpu: vi.fn()} : null),
}));
vi.mock('@app/features/voice/utils/native_screen_capture_bridge/createCpuFrameBridge', () => ({
	createCpuFrameBridge: state.bridge,
}));

describe('native Windows display capture before audio composition', () => {
	beforeEach(() => {
		state.platform = 'win32';
		state.available = true;
		state.sourceId = 'window:1234:0';
		state.target = {width: 1920, height: 1080, frameRate: 120, contentHint: 'motion'};
		state.bridge.mockReset().mockResolvedValue({track: state.track});
		state.consume.mockReset();
		vi.stubGlobal(
			'MediaStream',
			class {
				constructor(public tracks: Array<unknown>) {}
				getVideoTracks() {
					return this.tracks;
				}
				getAudioTracks() {
					return [];
				}
			},
		);
	});
	afterEach(() => vi.unstubAllGlobals());

	it('captures at 144 FPS while capping generated output at 120 FPS', async () => {
		const stream = await createNativeDisplayMediaStream();
		expect(state.bridge).toHaveBeenCalledWith(
			{
				sourceId: 'window:1234:0',
				sourceKind: 'window',
				width: 1920,
				height: 1080,
				frameRate: 144,
			},
			120,
		);
		expect(stream?.getVideoTracks()).toEqual([state.track]);
		expect(stream?.getAudioTracks()).toEqual([]);
		expect(state.track.contentHint).toBe('motion');
		expect(state.consume).toHaveBeenCalledTimes(1);
	});

	it('uses the same HDR capable path at 60 FPS', async () => {
		state.target!.frameRate = 60;
		state.sourceId = 'screen:42:0';
		await createNativeDisplayMediaStream();
		expect(state.bridge).toHaveBeenCalledWith(expect.objectContaining({sourceKind: 'screen', frameRate: 60}), 60);
	});

	it.each(['linux', 'darwin'])('leaves %s to the existing capture path', async (platform) => {
		state.platform = platform;
		expect(await createNativeDisplayMediaStream()).toBeNull();
		expect(state.bridge).not.toHaveBeenCalled();
		expect(state.consume).not.toHaveBeenCalled();
	});

	it('falls back to Chromium when the selected Windows source has no native API', async () => {
		state.available = false;
		await expect(createNativeDisplayMediaStream()).resolves.toBeNull();
		expect(state.bridge).not.toHaveBeenCalled();
		expect(state.consume).not.toHaveBeenCalled();
	});

	it('does not replace a browser capture without an explicit source intent', async () => {
		state.sourceId = null;
		expect(await createNativeDisplayMediaStream()).toBeNull();
		expect(state.bridge).not.toHaveBeenCalled();
	});

	it('surfaces native capture failure rather than silently advertising 120 on a 60 FPS fallback', async () => {
		state.bridge.mockRejectedValue(new Error('native capture failed'));
		await expect(createNativeDisplayMediaStream()).rejects.toThrow('native capture failed');
	});
});
