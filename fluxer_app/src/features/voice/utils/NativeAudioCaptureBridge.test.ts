// SPDX-License-Identifier: AGPL-3.0-or-later

import {afterEach, describe, expect, it, vi} from 'vitest';
import {
	armNativeAudioForNextCapture,
	armNativeSystemAudioForNextCapture,
	captureNativeAudioTrackForWindowPid,
	getLastNativeAudioArmFailure,
	resetNativeAudioCaptureBridgeForTests,
} from '@app/features/voice/utils/NativeAudioCaptureBridge';
import type {ElectronAPI, NativeAudioApi} from '@app/types/electron.d';

const TARGET_PID = 4242;
const SOURCE_ID = 'window:fixture:0';

function installElectronApi(nativeAudio: NativeAudioApi): void {
	vi.stubGlobal('window', {electron: {platform: 'win32', nativeAudio} as ElectronAPI});
	vi.stubGlobal('navigator', {
		mediaDevices: {
			getDisplayMedia: vi.fn(async () => ({}) as unknown as MediaStream),
		},
	});
}

function createNativeAudioApi(overrides: Partial<NativeAudioApi> = {}): NativeAudioApi {
	return {
		getAvailability: vi.fn(async () => ({
			available: true,
			backend: 'windows-wasapi-loopback' as const,
			capabilities: {process: true, system: true, systemExcludesSelf: true},
		})),
		listAudibleApplications: vi.fn(async () => []),
		resolveAudioRootPidForSource: vi.fn(async () => TARGET_PID),
		start: vi.fn(async () => ({captureId: 'capture-1', sampleRate: 48000, channels: 2})),
		setRule: vi.fn(async () => true),
		stop: vi.fn(async () => undefined),
		getRoutingGraph: vi.fn(async () => ({ok: true, graphs: [], availability: {available: true}})),
		onFrame: vi.fn(() => () => undefined),
		onEnd: vi.fn(() => () => undefined),
		...overrides,
	};
}

describe('NativeAudioCaptureBridge Windows process capture', () => {
	afterEach(() => {
		resetNativeAudioCaptureBridgeForTests();
		vi.unstubAllGlobals();
	});

	it('fails the selected-window arm when process loopback fails instead of starting system capture', async () => {
		const start = vi
			.fn<NativeAudioApi['start']>()
			.mockRejectedValueOnce(new Error('ProcessLoopback activation failed'))
			.mockResolvedValueOnce({captureId: 'system-capture', sampleRate: 48000, channels: 2});
		const nativeAudio = createNativeAudioApi({start});
		installElectronApi(nativeAudio);

		await expect(armNativeAudioForNextCapture(SOURCE_ID)).resolves.toBe(false);

		expect(nativeAudio.resolveAudioRootPidForSource).toHaveBeenCalledWith(SOURCE_ID);
		expect(start).toHaveBeenCalledTimes(1);
		expect(start).toHaveBeenCalledWith({targetPid: TARGET_PID, includeProcessTree: true});
		expect(getLastNativeAudioArmFailure()).toMatchObject({
			platform: 'win32',
			sourceId: SOURCE_ID,
			sourceMode: 'specific',
			reason: 'native-audio-start-failed',
			detail: 'ProcessLoopback activation failed',
		});
	});

	it('keeps the explicit Windows system-audio route available', async () => {
		const start = vi.fn<NativeAudioApi['start']>(async () => ({
			captureId: 'system-capture',
			sampleRate: 48000,
			channels: 2,
		}));
		installElectronApi(createNativeAudioApi({start}));

		await expect(armNativeSystemAudioForNextCapture()).resolves.toBe(true);

		expect(start).toHaveBeenCalledTimes(1);
		expect(start).toHaveBeenCalledWith({includeProcessTree: false, winCaptureScope: 'system'});
	});

	it('returns a selected-window track failure without retrying as system capture', async () => {
		const start = vi.fn<NativeAudioApi['start']>().mockRejectedValue(new Error('Selected process exited'));
		installElectronApi(createNativeAudioApi({start}));

		await expect(captureNativeAudioTrackForWindowPid(TARGET_PID)).resolves.toBeNull();

		expect(start).toHaveBeenCalledTimes(1);
		expect(start).toHaveBeenCalledWith({targetPid: TARGET_PID, includeProcessTree: true});
		expect(getLastNativeAudioArmFailure()).toMatchObject({
			platform: 'win32',
			sourceMode: 'specific',
			reason: 'native-audio-start-failed',
			detail: 'Selected process exited',
		});
	});
});
