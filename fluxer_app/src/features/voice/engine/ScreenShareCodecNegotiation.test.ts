// SPDX-License-Identifier: AGPL-3.0-or-later

import {
	createScreenShareCodecNegotiationSnapshot,
	transitionScreenShareCodecNegotiationSnapshot,
} from '@app/features/voice/engine/ScreenShareCodecNegotiation';
import {
	buildScreenShareCodecAdvertisements,
	type FluxerCodecAdvertisement,
} from '@app/features/voice/utils/ScreenShareCodecSelection';
import type {VideoCodec} from 'livekit-client';
import {describe, expect, it, vi} from 'vitest';

vi.mock('@app/features/devtools/utils/DesktopTroubleshootingUtils', () => ({
	getDesktopTroubleshootingSettings: vi.fn(),
}));
vi.mock('@app/features/platform/utils/AppLogger', () => ({
	Logger: class {
		debug(): void {}
		info(): void {}
		warn(): void {}
	},
}));
vi.mock('@app/features/voice/engine/VoiceMediaEngineBridge', () => ({
	getVoiceConnectionContextFromMediaEngine: vi.fn(() => null),
	getVoiceStateByConnectionIdFromMediaEngine: vi.fn(() => null),
}));
vi.mock('@app/features/voice/engine/VoiceStreamWatchState', () => ({
	getStreamKeyForParticipantIdentity: vi.fn(() => null),
}));
vi.mock('@app/features/voice/engine/VoiceTrackPublicationUtils', () => ({
	getLocalScreenShareVideoPublications: vi.fn(() => []),
	isLiveLocalTrackPublication: vi.fn(() => false),
}));
vi.mock('@app/features/voice/state/ScreenShareDeliveryRollout', () => ({
	default: {enabled: false},
}));
vi.mock('@app/features/voice/state/VoiceSettings', () => ({
	default: {
		getPreferredScreenShareCodec: () => 'auto',
		getScreenShareEncoderMode: () => 'auto',
	},
}));
vi.mock('@app/features/voice/utils/CodecCapabilityDetector', () => ({
	buildScreenShareCodecProfile: vi.fn(),
	isVideoCodecAllowedForPublish: vi.fn(() => true),
	selectNativeScreenCaptureScreenShareCodec: vi.fn(() => 'vp8'),
	selectOptimalScreenShareCodec: vi.fn(() => 'vp8'),
}));
vi.mock('@app/features/voice/utils/GpuEncoderCapabilities', () => ({
	loadGpuEncoderReport: vi.fn(),
}));
vi.mock('@app/features/voice/utils/NativeHardwareEncoderCapabilities', () => ({
	loadNativeHardwareEncoderCapabilities: vi.fn(),
}));
vi.mock('@app/features/voice/utils/VideoDecoderCapabilities', () => ({
	getScreenShareDecodeFailures: vi.fn(() => []),
	getVideoDecoderExclusionsSync: vi.fn(() => []),
	loadVideoDecoderExclusions: vi.fn(),
}));
vi.mock('@app/features/voice/utils/VoiceParticipantIdentity', () => ({
	parseVoiceParticipantIdentity: vi.fn(() => ({connectionId: ''})),
}));
vi.mock('livekit-client', () => ({
	RoomEvent: {
		DataReceived: 'dataReceived',
		ParticipantConnected: 'participantConnected',
		ParticipantDisconnected: 'participantDisconnected',
		Reconnected: 'reconnected',
	},
}));

const codecPreference: ReadonlyArray<VideoCodec> = ['h265', 'h264', 'vp8'];
const localCodecs: Array<FluxerCodecAdvertisement> = buildScreenShareCodecAdvertisements(codecPreference, {
	av1: false,
	h265: false,
	h264: true,
	vp9: false,
	vp8: true,
});
const h264OnlyReceiver: Array<FluxerCodecAdvertisement> = buildScreenShareCodecAdvertisements([], {
	av1: false,
	h265: false,
	h264: true,
	vp9: false,
	vp8: false,
});

function evaluate(input: {
	remoteCodecs?: ReadonlyArray<ReadonlyArray<FluxerCodecAdvertisement>>;
	unknownParticipants?: number;
	publishedCodec?: VideoCodec | null;
	requestedCodec?: VideoCodec | null;
	codecPreference?: ReadonlyArray<VideoCodec>;
}) {
	return transitionScreenShareCodecNegotiationSnapshot(createScreenShareCodecNegotiationSnapshot(), {
		type: 'negotiation.evaluate',
		localCodecs,
		remoteCodecs: input.remoteCodecs ?? [],
		unknownParticipants: input.unknownParticipants ?? 0,
		reason: 'manual',
		codecPreference: input.codecPreference ?? codecPreference,
		publishedCodec: input.publishedCodec ?? null,
		requestedCodec: input.requestedCodec ?? null,
	}).context.selection;
}

describe('screen share codec negotiation', () => {
	it('uses an explicitly requested H.265 codec when a known H.264-only and an unknown receiver are present', () => {
		expect(
			evaluate({
				remoteCodecs: [h264OnlyReceiver],
				unknownParticipants: 1,
				requestedCodec: 'h265',
			})?.codec,
		).toBe('h265');
	});

	it('does not keep the published H.264 codec over an explicit H.265 request', () => {
		expect(
			evaluate({
				publishedCodec: 'h264',
				requestedCodec: 'h265',
			})?.codec,
		).toBe('h265');
	});

	it('keeps receiver compatibility selection in auto mode', () => {
		expect(
			evaluate({
				remoteCodecs: [h264OnlyReceiver],
				codecPreference: codecPreference,
			})?.codec,
		).toBe('h264');
	});
});
