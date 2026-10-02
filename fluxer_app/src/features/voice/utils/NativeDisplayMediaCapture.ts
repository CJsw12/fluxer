// SPDX-License-Identifier: AGPL-3.0-or-later

import {getElectronAPI} from '@app/features/ui/utils/NativeUtils';
import ActiveScreenShareSource from '@app/features/voice/state/ActiveScreenShareSource';
import {consumeDesktopSourceIntent, peekDesktopSourceIntent} from '@app/features/voice/state/DesktopSourceIntent';
import {createCpuFrameBridge} from '@app/features/voice/utils/native_screen_capture_bridge/createCpuFrameBridge';
import {getNativeScreenCaptureApi} from '@app/features/voice/utils/native_screen_capture_bridge/shared';

export async function createNativeDisplayMediaStream(): Promise<MediaStream | null> {
	if (getElectronAPI()?.platform !== 'win32') return null;
	const intent = peekDesktopSourceIntent();
	const target = ActiveScreenShareSource.getTarget();
	if (!intent || !target || !/^(window|screen):[^:]+:[01]$/.test(intent.sourceId)) return null;
	if (!getNativeScreenCaptureApi()?.startCpu) return null;
	consumeDesktopSourceIntent();
	const bridge = await createCpuFrameBridge(
		{
			sourceId: intent.sourceId,
			sourceKind: intent.sourceId.startsWith('window:') ? 'window' : 'screen',
			width: target.width,
			height: target.height,
			frameRate: target.frameRate === 120 ? 144 : target.frameRate,
		},
		target.frameRate,
	);
	if (target.contentHint) bridge.track.contentHint = target.contentHint;
	return new MediaStream([bridge.track]);
}
