// SPDX-License-Identifier: AGPL-3.0-or-later

import {beforeEach, describe, expect, it, vi} from 'vitest';

const state = vi.hoisted(() => ({
	storage: new Map<string, string>(),
	mediaEngine: {
		room: null as object | null,
		refreshMicrophoneFromSettings: () => undefined,
		refreshCameraBackgroundFromSettings: () => undefined,
		refreshCameraCaptureFromSettings: () => undefined,
		refreshScreenShareCodecNegotiationFromSettings: () => undefined,
		setScreenShareAudioMuted: () => undefined,
		applyAllLocalAudioPreferences: () => undefined,
		applyLocalInputVolume: () => undefined,
	},
}));

vi.mock('@app/features/platform/state/PersistentStorage', () => ({
	default: {
		getItem: (key: string) => state.storage.get(key) ?? null,
		setItem: (key: string, value: string) => state.storage.set(key, value),
	},
}));

vi.mock('@app/features/platform/utils/MobXPersistence', () => ({
	makePersistent: async (store: Record<string, unknown>, storageKey: string) => {
		const raw = state.storage.get(storageKey);
		if (raw) Object.assign(store, JSON.parse(raw));
	},
}));

vi.mock('@app/features/platform/utils/AppLogger', () => ({
	Logger: class {
		warn(): void {}
	},
}));

vi.mock('@app/features/voice/state/ScreenShareDeliveryRollout', () => ({default: {enabled: false}}));
vi.mock('@app/features/voice/utils/VideoQualityEntitlement', () => ({hasHigherVideoQuality: () => false}));
vi.mock('@app/features/voice/utils/VoiceBackgroundAvailability', () => ({areVoiceBackgroundsAvailable: () => false}));
vi.mock('@app/features/voice/utils/VoiceProcessingProfile', () => ({
	DEFAULT_VOICE_PROCESSING_MODE: 'voice',
	getActiveInputDeviceLabel: () => null,
}));
vi.mock('@app/features/voice/utils/VoiceVolumeUtils', () => ({clampVoiceVolumePercent: (value: number) => value}));
vi.mock('@app/features/voice/engine/MediaEngineFacade', () => ({default: state.mediaEngine}));

async function loadSettings() {
	return (await import('@app/features/voice/state/VoiceSettings')).default;
}

describe('screen share max bitrate setting', () => {
	beforeEach(() => {
		vi.resetModules();
		state.storage.clear();
	});

	it('defaults to auto and clamps integer overrides to 1–20 Mbps', async () => {
		const settings = await loadSettings();
		expect(settings.getScreenShareMaxBitrateMbps()).toBeNull();

		for (const [value, expected] of [
			[1, 1],
			[12, 12],
			[20, 20],
			[50, 20],
		] as const) {
			settings.updateSettings({screenShareMaxBitrateMbps: value});
			expect(settings.getScreenShareMaxBitrateMbps()).toBe(expected);
		}
	});

	it('uses auto for invalid update values', async () => {
		const settings = await loadSettings();
		settings.updateSettings({screenShareMaxBitrateMbps: 12});

		for (const value of [0, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) {
			settings.updateSettings({screenShareMaxBitrateMbps: value});
			expect(settings.getScreenShareMaxBitrateMbps()).toBeNull();
		}
	});

	it('normalizes unknown persisted values to auto before hydration', async () => {
		state.storage.set('VoiceSettings', JSON.stringify({screenShareMaxBitrateMbpsPrefV2: '12'}));

		const settings = await loadSettings();

		expect(settings.getScreenShareMaxBitrateMbps()).toBeNull();
		expect(JSON.parse(state.storage.get('VoiceSettings')!).screenShareMaxBitrateMbpsPrefV2).toBeNull();
	});

	it('hydrates a valid persisted override', async () => {
		state.storage.set('VoiceSettings', JSON.stringify({screenShareMaxBitrateMbpsPrefV2: 24}));

		const settings = await loadSettings();

		expect(settings.getScreenShareMaxBitrateMbps()).toBe(20);
		expect(JSON.parse(state.storage.get('VoiceSettings')!).screenShareMaxBitrateMbpsPrefV2).toBe(20);
	});

	it('exposes a command to update or clear the override', async () => {
		const settings = await loadSettings();
		const {setScreenShareMaxBitrateMbps} = await import('@app/features/voice/commands/VoiceSettingsCommands');

		setScreenShareMaxBitrateMbps(24);
		expect(settings.getScreenShareMaxBitrateMbps()).toBe(20);

		setScreenShareMaxBitrateMbps(null);
		expect(settings.getScreenShareMaxBitrateMbps()).toBeNull();
	});
});
