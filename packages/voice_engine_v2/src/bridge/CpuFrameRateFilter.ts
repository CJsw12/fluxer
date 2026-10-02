// SPDX-License-Identifier: AGPL-3.0-or-later

const CAPTURE_STALL_GAP_US = 250_000;

export function createCpuFrameRateFilter(
	captureFrameRate: number,
	outputFrameRate: number,
): (timestampUs: number) => boolean {
	let previousTimestampUs: number | undefined;
	let frameCredit = 0;
	return (timestampUs) => {
		if (previousTimestampUs === undefined) {
			previousTimestampUs = timestampUs;
			return true;
		}
		if (timestampUs <= previousTimestampUs) return false;
		const elapsedUs = timestampUs - previousTimestampUs;
		const elapsedCredit = (elapsedUs * outputFrameRate) / 1_000_000;
		previousTimestampUs = timestampUs;
		if (outputFrameRate >= captureFrameRate) return true;
		// A stall starts a new cadence; unused time must not produce a catch-up burst.
		if (elapsedUs >= CAPTURE_STALL_GAP_US) {
			frameCredit = 0;
			return true;
		}
		frameCredit += elapsedCredit;
		if (frameCredit < 1) return false;
		// Keep at most two spare frames of budget for close pairs; a stall still clears all credit.
		frameCredit = Math.min(frameCredit - 1, 2);
		return true;
	};
}
