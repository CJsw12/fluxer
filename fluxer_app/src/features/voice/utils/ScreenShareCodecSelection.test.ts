// SPDX-License-Identifier: AGPL-3.0-or-later

import {
	computeNegotiatedVideoCodec,
	rankScreenShareCodecs,
	isScreenShareCodecUpgrade,
	type FluxerCodecAdvertisement,
} from '@app/features/voice/utils/ScreenShareCodecSelection';
import {describe, expect, it} from 'vitest';

describe('explicit screen share codec', () => {
	const local: Array<FluxerCodecAdvertisement> = [
		{name: 'H265', type: 'video', payload_type: 105, priority: 4000, encode: true, decode: true},
		{name: 'H264', type: 'video', payload_type: 103, priority: 3000, encode: true, decode: true},
	];
	it('keeps H265 despite unknown or H264-only receivers', () => {
		expect(computeNegotiatedVideoCodec(local, [[local[1]]], 1, ['h265', 'h264'], 'h265').codec).toBe('h265');
	});
	it('retains compatibility negotiation for auto', () => {
		expect(computeNegotiatedVideoCodec(local, [[local[1]]], 1, ['h265', 'h264']).codec).toBe('h264');
	});
	it('rejects unsupported local encoding instead of silently switching', () => {
		expect(() => computeNegotiatedVideoCodec([local[1]], [], 0, ['h265', 'h264'], 'h265')).toThrow(/h265/);
	});
});

describe('isScreenShareCodecUpgrade', () => {
	it('treats an earlier ranked codec than the published one as an upgrade', () => {
		expect(isScreenShareCodecUpgrade(['vp9', 'h264', 'vp8'], 'h264', 'vp9')).toBe(true);
	});

	it('treats a later ranked codec than the published one as a downgrade', () => {
		expect(isScreenShareCodecUpgrade(['vp9', 'h264', 'vp8'], 'vp9', 'h264')).toBe(false);
	});

	it('treats a codec missing from the order as a downgrade', () => {
		expect(isScreenShareCodecUpgrade(['vp9', 'vp8'], 'h264', 'vp9')).toBe(false);
		expect(isScreenShareCodecUpgrade(['vp9', 'vp8'], 'vp9', 'h264')).toBe(false);
	});
});

it('prefers hardware HEVC in auto while preserving a manual codec', () => {
	const profile = {
		browser: 'chromium' as const,
		desktop: true,
		codecs: Object.fromEntries(
			['av1', 'h265', 'h264', 'vp9', 'vp8'].map((codec) => [codec, {allowed: true, supported: true, hardware: true}]),
		),
	} as Parameters<typeof rankScreenShareCodecs>[0]['profile'];
	expect(rankScreenShareCodecs({profile, encoderModeSetting: 'auto', pin: 'auto'}).order[0]).toBe('h265');
	expect(rankScreenShareCodecs({profile, encoderModeSetting: 'auto', pin: 'av1'}).order[0]).toBe('av1');
	profile.codecs.h265.supported = false;
	expect(rankScreenShareCodecs({profile, encoderModeSetting: 'auto', pin: 'auto'}).order[0]).toBe('av1');
});
