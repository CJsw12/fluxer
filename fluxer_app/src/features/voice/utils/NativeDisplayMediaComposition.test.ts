// @vitest-environment happy-dom
// SPDX-License-Identifier: AGPL-3.0-or-later

import {afterEach, describe, expect, it, vi} from 'vitest';

const state = vi.hoisted(() => ({
	events: [] as string[],
	originalCapture: vi.fn(),
	createNativeDisplayMediaStream: vi.fn(),
	createGeneratorBridge: vi.fn(),
	nativeAudioApi: {
		getAvailability: vi.fn(),
		resolveAudioRootPidForSource: vi.fn(),
		start: vi.fn(),
		stop: vi.fn(),
		onFrame: vi.fn(),
		onEnd: vi.fn(),
	},
}));

vi.mock('@app/features/ui/utils/NativeUtils', () => ({
	getElectronAPI: () => ({platform: 'win32', nativeAudio: state.nativeAudioApi}),
}));

vi.mock('@app/features/voice/utils/NativeDisplayMediaCapture', () => ({
	createNativeDisplayMediaStream: state.createNativeDisplayMediaStream,
}));

vi.mock('@app/features/voice/utils/native_audio_capture_bridge/createGeneratorBridge', () => ({
	createGeneratorBridge: state.createGeneratorBridge,
}));

vi.mock('@app/features/voice/utils/native_audio_capture_bridge/createScriptProcessorBridge', () => ({
	createScriptProcessorBridge: vi.fn(),
}));

class FakeTrack extends EventTarget {
	readonly kind: 'audio' | 'video';
	readyState: MediaStreamTrackState = 'live';
	stop = vi.fn(() => {
		this.readyState = 'ended';
		this.dispatchEvent(new Event('ended'));
	});

	constructor(kind: 'audio' | 'video') {
		super();
		this.kind = kind;
	}

	override addEventListener(
		type: string,
		listener: EventListenerOrEventListenerObject | null,
		options?: boolean | AddEventListenerOptions,
	): void {
		if (type === 'ended') state.events.push(`${this.kind}-lifecycle-attached`);
		super.addEventListener(type, listener, options);
	}
}

class FakeStream {
	private tracks: FakeTrack[];

	constructor(tracks: FakeTrack[]) {
		this.tracks = [...tracks];
	}

	getTracks(): FakeTrack[] {
		return [...this.tracks];
	}

	getVideoTracks(): FakeTrack[] {
		return this.tracks.filter((track) => track.kind === 'video');
	}

	getAudioTracks(): FakeTrack[] {
		return this.tracks.filter((track) => track.kind === 'audio');
	}

	addTrack(track: FakeTrack): void {
		state.events.push(`stream-add-${track.kind}`);
		this.tracks.push(track);
	}

	removeTrack(track: FakeTrack): void {
		this.tracks = this.tracks.filter((candidate) => candidate !== track);
	}
}

describe('native display media and native audio composition', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it('creates native video first, attaches live native audio, and cleans audio up when video stops', async () => {
		const videoTrack = new FakeTrack('video');
		const audioTrack = new FakeTrack('audio');
		const stream = new FakeStream([videoTrack]);
		state.events.length = 0;
		state.nativeAudioApi.getAvailability.mockReset().mockResolvedValue({
			available: true,
			backend: 'test',
			capabilities: {process: true},
		});
		state.nativeAudioApi.resolveAudioRootPidForSource.mockReset().mockResolvedValue(4321);
		state.nativeAudioApi.start.mockReset().mockImplementation(async () => {
			state.events.push('native-audio-capture-started');
			return {captureId: 'native-audio-1', sampleRate: 48000, channels: 2};
		});
		state.nativeAudioApi.stop.mockReset().mockResolvedValue(undefined);
		state.createNativeDisplayMediaStream.mockReset().mockImplementation(async () => {
			state.events.push('native-video-created');
			return stream;
		});
		const cleanup = vi.fn(async (stopRemote: boolean, endDetail?: string) => {
			state.events.push(`audio-bridge-cleanup:${endDetail ?? 'unknown'}`);
			if (stopRemote) await state.nativeAudioApi.stop('native-audio-1');
			audioTrack.stop();
		});
		state.createGeneratorBridge.mockReset().mockImplementation(async () => {
			state.events.push('audio-bridge-created');
			return {track: audioTrack, cleanup};
		});

		vi.stubGlobal('MediaStreamTrackGenerator', class {});
		vi.stubGlobal('AudioData', class {});
		vi.stubGlobal('navigator', {
			mediaDevices: {
				getDisplayMedia: state.originalCapture.mockReset().mockResolvedValue(new FakeStream([])),
			},
		});

		const {armNativeAudioForNextCapture, installDesktopDisplayMediaCapture} = await import('./NativeAudioCaptureBridge');
		expect(await armNativeAudioForNextCapture('window:4321:0')).toBe(true);
		installDesktopDisplayMediaCapture();

		const composed = await navigator.mediaDevices.getDisplayMedia({audio: true, video: true});

		expect(composed).toBe(stream);
		expect(state.originalCapture).not.toHaveBeenCalled();
		expect(stream.getVideoTracks()).toEqual([videoTrack]);
		expect(stream.getAudioTracks()).toEqual([audioTrack]);
		expect(state.events.indexOf('native-video-created')).toBeLessThan(state.events.indexOf('audio-bridge-created'));
		expect(state.events.indexOf('native-video-created')).toBeLessThan(state.events.indexOf('video-lifecycle-attached'));
		expect(cleanup).not.toHaveBeenCalled();
		expect(audioTrack.readyState).toBe('live');
		expect(state.nativeAudioApi.stop).not.toHaveBeenCalled();

		videoTrack.stop();
		await vi.waitFor(() => expect(cleanup).toHaveBeenCalledWith(true, 'caller-stopped'));
		expect(audioTrack.stop).toHaveBeenCalledTimes(1);
		expect(state.nativeAudioApi.stop).toHaveBeenCalledExactlyOnceWith('native-audio-1');
	});
});
