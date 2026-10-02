// SPDX-License-Identifier: AGPL-3.0-or-later

import {randomUUID} from 'node:crypto';
import type {
	NativeScreenCaptureApi,
	NativeScreenCaptureCpuFrame,
	NativeScreenCaptureDiagnostics,
	NativeScreenCaptureStartResult,
} from '@electron/common/Types';
import {createCpuFrameRateFilter} from '@fluxer/voice_engine_v2/src/bridge/CpuFrameRateFilter';
import type {ScreenCapture} from '@fluxer/win-game-capture';
import {ipcRenderer} from 'electron';

type CpuCapture = ScreenCapture & {
	setCpuFrameCallback: (callback: ((frame: NativeScreenCaptureCpuFrame) => void) | null) => void;
};

interface CpuCaptureSession {
	captureId: string;
	onEnd: (reason: string) => void;
	capture?: CpuCapture;
	onError?: (error: Error) => void;
	onClosed?: () => void;
	ended: boolean;
	cancelled: boolean;
	endReason: string;
	cpuFrameDispatch: {
		received: number;
		forwarded: number;
		rateDropped: number;
		maxDispatchDurationMs: number;
	};
	stopping?: Promise<void>;
}

export function getNativeScreenCpuDiagnostics(captureId: string): NativeScreenCaptureDiagnostics | null {
	const session = captures.get(captureId);
	if (!session?.capture || session.ended || session.cancelled) return null;
	return {...session.capture.getDiagnostics(), captureId, cpuFrameDispatch: {...session.cpuFrameDispatch}};
}

const MAX_CPU_CAPTURES = 2;
const captures = new Map<string, CpuCaptureSession>();

function discardSession(session: CpuCaptureSession): void {
	if (captures.get(session.captureId) === session) captures.delete(session.captureId);
	session.ended = true;
	session.capture?.setCpuFrameCallback(null);
	if (session.capture && session.onError) session.capture.removeListener('error', session.onError);
	if (session.capture && session.onClosed) session.capture.removeListener('closed', session.onClosed);
}

function endSession(session: CpuCaptureSession, reason: string): void {
	if (session.ended) return;
	discardSession(session);
	session.onEnd(reason);
}

function stopSession(session: CpuCaptureSession, reason: string = 'closed'): Promise<void> {
	session.cancelled = true;
	if (session.ended) return Promise.resolve();
	if (session.stopping) return session.stopping;
	if (!session.capture) {
		discardSession(session);
		return Promise.resolve();
	}
	session.stopping = Promise.resolve()
		.then(() => session.capture!.stop())
		.finally(() => endSession(session, reason));
	return session.stopping;
}

function cleanupBeforeUnload(): void {
	for (const session of [...captures.values()]) {
		void stopSession(session).catch(() => undefined);
	}
}

if (typeof window !== 'undefined') {
	window.addEventListener('beforeunload', cleanupBeforeUnload);
}

export const nativeScreenCpuApi: Pick<NativeScreenCaptureApi, 'startCpu' | 'stopCpu' | 'cpuFrameRateFiltering'> = {
	cpuFrameRateFiltering: true,
	async startCpu(options, onFrame, onEnd): Promise<NativeScreenCaptureStartResult> {
		options = {...options};
		if (process.platform !== 'win32') throw new Error('Windows screen capture is unavailable');
		if (!/^(window|screen):[^:]+:[01]$/.test(options.sourceId)) throw new Error('Invalid screen source');
		if (options.sourceKind !== 'window' && options.sourceKind !== 'screen')
			throw new Error('Invalid screen source kind');
		if (!Number.isInteger(options.width) || options.width < 16 || options.width > 8192) {
			throw new Error('Invalid screen width');
		}
		if (!Number.isInteger(options.height) || options.height < 16 || options.height > 8192) {
			throw new Error('Invalid screen height');
		}
		if (!Number.isInteger(options.frameRate) || options.frameRate < 1 || options.frameRate > 144) {
			throw new Error('Invalid screen frame rate');
		}
		if (
			options.outputFrameRate !== undefined &&
			(!Number.isInteger(options.outputFrameRate) || options.outputFrameRate < 1 || options.outputFrameRate > options.frameRate)
		) {
			throw new Error('Invalid screen output frame rate');
		}
		if (captures.size >= MAX_CPU_CAPTURES) throw new Error('Maximum CPU screen captures reached');

		const {outputFrameRate, ...captureOptions} = options;
		const shouldForwardFrame = createCpuFrameRateFilter(options.frameRate, outputFrameRate ?? options.frameRate);
		const captureId = randomUUID();
		const session: CpuCaptureSession = {
			captureId,
			onEnd,
			ended: false,
			cancelled: false,
			endReason: 'closed',
			cpuFrameDispatch: {received: 0, forwarded: 0, rateDropped: 0, maxDispatchDurationMs: 0},
		};
		captures.set(captureId, session);
		try {
			const authorized = await ipcRenderer.invoke('native-screen-capture:authorize-cpu-start', options);
			if (session.cancelled || session.ended || captures.get(captureId) !== session) {
				throw new Error('Native screen capture startup was cancelled');
			}
			if (authorized !== true) throw new Error('Native Windows screen capture is not authorized');

			const {ScreenCapture} = await import('@fluxer/win-game-capture');
			if (session.cancelled || session.ended || captures.get(captureId) !== session) {
				throw new Error('Native screen capture startup was cancelled');
			}
			const capture = new ScreenCapture({...captureOptions, captureId}) as CpuCapture;
			session.capture = capture;
			session.onError = (error) => {
				session.endReason = error.message;
				void stopSession(session, session.endReason).catch(() => endSession(session, session.endReason));
			};
			session.onClosed = () => endSession(session, session.endReason);
			capture.on('error', session.onError);
			capture.on('closed', session.onClosed);
			capture.setCpuFrameCallback((frame) => {
				if (session.ended || session.cancelled) return;
				session.cpuFrameDispatch.received += 1;
				if (!shouldForwardFrame(frame.timestampUs)) {
					session.cpuFrameDispatch.rateDropped += 1;
					return;
				}
				const dispatchStartedAtMs = performance.now();
				session.cpuFrameDispatch.forwarded += 1;
				try {
					onFrame(frame);
				} finally {
					session.cpuFrameDispatch.maxDispatchDurationMs = Math.max(
						session.cpuFrameDispatch.maxDispatchDurationMs,
						performance.now() - dispatchStartedAtMs,
					);
				}
			});

			const result = await capture.start();
			if (!result) throw new Error('Screen capture returned no format');
			if (session.cancelled || session.ended || captures.get(captureId) !== session) {
				await stopSession(session).catch(() => undefined);
				throw new Error('Native screen capture ended before startup completed');
			}
			return {...result, captureId};
		} catch (error) {
			if (session.capture) {
				await stopSession(session, session.endReason).catch(() => undefined);
			} else {
				discardSession(session);
			}
			throw error;
		}
	},
	async stopCpu(captureId): Promise<void> {
		const session = captures.get(captureId);
		if (!session) return;
		await stopSession(session);
	},
};
