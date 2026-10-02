// SPDX-License-Identifier: AGPL-3.0-or-later

import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {describe, test} from 'node:test';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';

const require = createRequire(import.meta.url);
const esbuild = require('esbuild');
const sourcePath = fileURLToPath(new URL('./NativeScreenCpu.ts', import.meta.url));
const source = readFileSync(sourcePath, 'utf8').replace(
	"await import('@fluxer/win-game-capture')",
	"await Promise.resolve(require('@fluxer/win-game-capture'))",
);
const transformedSource = esbuild.transformSync(source, {
	loader: 'ts',
	format: 'cjs',
	platform: 'node',
	target: 'node20',
}).code;
const frameRateFilterPath = fileURLToPath(
	new URL('../../../packages/voice_engine_v2/src/bridge/CpuFrameRateFilter.ts', import.meta.url),
);
const frameRateFilterModule = {exports: {}};
vm.runInNewContext(
	esbuild.transformSync(readFileSync(frameRateFilterPath, 'utf8'), {
		loader: 'ts',
		format: 'cjs',
		platform: 'node',
		target: 'node20',
	}).code,
	{exports: frameRateFilterModule.exports, module: frameRateFilterModule},
	{filename: frameRateFilterPath},
);

function deferred() {
	let resolve;
	let reject;
	const promise = new Promise((resolvePromise, rejectPromise) => {
		resolve = resolvePromise;
		reject = rejectPromise;
	});
	return {promise, resolve, reject};
}

function makeHarness({
	platform = 'win32',
	authorize = async () => true,
	start = async () => ({
		width: 1280,
		height: 720,
		frameRate: 120,
		pixelFormat: 'nv12',
	}),
} = {}) {
	const calls = {events: [], authorization: [], addonImports: 0};
	const captures = [];
	const listeners = new Map();
	const window = {
		addEventListener(type, listener) {
			const registered = listeners.get(type) ?? new Set();
			registered.add(listener);
			listeners.set(type, registered);
		},
		dispatch(type) {
			for (const listener of listeners.get(type) ?? []) listener();
		},
	};

	class FakeCapture extends EventEmitter {
		constructor(options) {
			super();
			this.options = options;
			this.stopCount = 0;
			this.startCalled = false;
			this.cpuFrameCallback = null;
			captures.push(this);
		}

		setCpuFrameCallback(callback) {
			this.cpuFrameCallback = callback;
		}

		start() {
			this.startCalled = true;
			return start(this);
		}

		async stop() {
			this.stopCount += 1;
			this.emit('closed');
		}
	}

	const ipcRenderer = {
		invoke(channel, options) {
			calls.events.push('authorize');
			calls.authorization.push({channel, options});
			return Promise.resolve(authorize(options));
		},
	};
	function requireStub(specifier) {
		if (specifier === 'node:crypto') return {randomUUID: () => `cpu-capture-${calls.authorization.length}`};
		if (specifier === 'electron') return {ipcRenderer};
		if (specifier === '@fluxer/voice_engine_v2/src/bridge/CpuFrameRateFilter') return frameRateFilterModule.exports;
		if (specifier === '@fluxer/win-game-capture') {
			calls.events.push('addon-import');
			calls.addonImports += 1;
			return {ScreenCapture: FakeCapture};
		}
		throw new Error(`Unexpected import: ${specifier}`);
	}

	const module = {exports: {}};
	const context = vm.createContext({
		console,
		exports: module.exports,
		module,
		process: {platform},
		performance,
		require: requireStub,
		window,
	});
	vm.runInContext(transformedSource, context, {filename: sourcePath});
	return {
		api: module.exports.nativeScreenCpuApi,
		getDiagnostics: module.exports.getNativeScreenCpuDiagnostics,
		calls,
		captures,
		window,
	};
}

const validOptions = () => ({sourceId: 'screen:42:0', sourceKind: 'screen', width: 1280, height: 720, frameRate: 120});

async function waitFor(predicate) {
	for (let attempt = 0; attempt < 30; attempt += 1) {
		if (predicate()) return;
		await new Promise((resolve) => setTimeout(resolve, 0));
	}
	assert.fail('Timed out waiting for native CPU capture state');
}

describe('NativeScreenCpu preload lifecycle', () => {
	test('uses the validated options snapshot after asynchronous authorization', async () => {
		const authorization = deferred();
		const harness = makeHarness({authorize: () => authorization.promise});
		const options = validOptions();
		assert.equal(harness.api.cpuFrameRateFiltering, true);
		const started = harness.api.startCpu(
			options,
			() => undefined,
			() => undefined,
		);
		options.width = 8193;
		options.sourceId = 'screen:999:0';
		authorization.resolve(true);
		const result = await started;
		assert.equal(harness.calls.authorization[0].options.width, 1280);
		assert.equal(harness.captures[0].options.width, 1280);
		assert.equal(harness.captures[0].options.sourceId, 'screen:42:0');
		await harness.api.stopCpu(result.captureId);
	});

	test('rejects an invalid output rate before authorization', async () => {
		for (const outputFrameRate of [0, 60.5, 121]) {
			const harness = makeHarness();
			await assert.rejects(
				harness.api.startCpu({...validOptions(), outputFrameRate}, () => undefined, () => undefined),
				/Invalid screen output frame rate/,
			);
			assert.equal(harness.calls.authorization.length, 0);
		}
	});

	test('filters frames before renderer delivery and reports dispatch savings', async () => {
		const harness = makeHarness();
		const timestamps = [];
		const result = await harness.api.startCpu(
			{...validOptions(), outputFrameRate: 60},
			(frame) => timestamps.push(frame.timestampUs),
			() => undefined,
		);
		const capture = harness.captures[0];
		capture.getDiagnostics = () => ({activeStrategy: 'fake'});
		assert.equal('outputFrameRate' in capture.options, false);
		for (const timestampUs of [0, 8333, 16667, 25000]) {
			capture.cpuFrameCallback({
				width: 1280,
				height: 720,
				pixelFormat: 'nv12',
				timestampUs,
				data: new Uint8Array(1),
			});
		}
		assert.deepEqual(timestamps, [0, 16667]);
		const dispatch = harness.getDiagnostics(result.captureId).cpuFrameDispatch;
		assert.deepEqual({
			received: dispatch.received,
			forwarded: dispatch.forwarded,
			rateDropped: dispatch.rateDropped,
		}, {
			received: 4,
			forwarded: 2,
			rateDropped: 2,
		});
		assert.equal(typeof dispatch.maxDispatchDurationMs, 'number');
		assert.ok(dispatch.maxDispatchDurationMs >= 0);
		await harness.api.stopCpu(result.captureId);
	});

	test('reads diagnostics from the local CPU session and drops access after stop', async () => {
		const harness = makeHarness();
		const result = await harness.api.startCpu(
			validOptions(),
			() => undefined,
			() => undefined,
		);
		harness.captures[0].getDiagnostics = () => ({activeStrategy: 'wgc', cpuPipeline: {conversionCount: 42}});
		const diagnostic = harness.getDiagnostics(result.captureId);
		assert.equal(diagnostic.captureId, result.captureId);
		assert.equal(diagnostic.activeStrategy, 'wgc');
		assert.equal(diagnostic.cpuPipeline.conversionCount, 42);
		assert.equal(harness.calls.authorization.length, 1);
		assert.equal(harness.getDiagnostics('another-session'), null);
		await harness.api.stopCpu(result.captureId);
		assert.equal(harness.getDiagnostics(result.captureId), null);
	});

	test('reserves the two-capture limit before authorization and keeps frame delivery local', async () => {
		const authorization = deferred();
		const harness = makeHarness({authorize: () => authorization.promise});
		const frames = [];
		const ended = [];
		const starts = [
			harness.api.startCpu(
				validOptions(),
				(frame) => frames.push(frame),
				(reason) => ended.push(reason),
			),
			harness.api.startCpu(
				validOptions(),
				(frame) => frames.push(frame),
				(reason) => ended.push(reason),
			),
		];

		await assert.rejects(
			harness.api.startCpu(
				validOptions(),
				() => undefined,
				() => undefined,
			),
			/Maximum CPU screen captures reached/,
		);
		assert.equal(harness.calls.authorization.length, 2);
		assert.equal(harness.calls.addonImports, 0);
		authorization.resolve(true);
		const results = await Promise.all(starts);
		assert.equal(harness.captures.length, 2);
		assert.deepEqual(harness.calls.events, ['authorize', 'authorize', 'addon-import', 'addon-import']);
		assert.equal(harness.calls.authorization[0].channel, 'native-screen-capture:authorize-cpu-start');
		assert.deepEqual({...harness.calls.authorization[0].options}, validOptions());

		const frame = {width: 1280, height: 720, pixelFormat: 'nv12', timestampUs: 1, data: new Uint8Array(1)};
		harness.captures[0].cpuFrameCallback(frame);
		assert.deepEqual(frames, [frame]);
		assert.equal(harness.calls.authorization.length, 2);

		await Promise.all(results.map(({captureId}) => harness.api.stopCpu(captureId)));
		assert.deepEqual(
			harness.captures.map((capture) => capture.stopCount),
			[1, 1],
		);
		assert.deepEqual(ended, ['closed', 'closed']);
	});

	test('cancels a pending authorization on renderer beforeunload without importing the addon', async () => {
		const authorization = deferred();
		const harness = makeHarness({authorize: () => authorization.promise});
		const ended = [];
		const start = harness.api.startCpu(
			validOptions(),
			() => undefined,
			(reason) => ended.push(reason),
		);

		harness.window.dispatch('beforeunload');
		authorization.resolve(true);
		await assert.rejects(start, /startup was cancelled/);
		assert.equal(harness.calls.addonImports, 0);
		assert.equal(harness.captures.length, 0);
		assert.deepEqual(ended, []);
	});

	test('stops a capture whose asynchronous start resolves after renderer beforeunload', async () => {
		const startup = deferred();
		const harness = makeHarness({start: () => startup.promise});
		const ended = [];
		const start = harness.api.startCpu(
			validOptions(),
			() => undefined,
			(reason) => ended.push(reason),
		);
		await waitFor(() => harness.captures[0]?.startCalled === true);

		harness.window.dispatch('beforeunload');
		await waitFor(() => harness.captures[0]?.stopCount === 1);
		startup.resolve({width: 1280, height: 720, frameRate: 120, pixelFormat: 'nv12'});
		await assert.rejects(start, /ended before startup completed/);
		assert.equal(harness.captures[0].stopCount, 1);
		assert.deepEqual(ended, ['closed']);
	});

	test('does not construct the native capture when main-process authorization refuses the source', async () => {
		const harness = makeHarness({authorize: async () => false});
		await assert.rejects(
			harness.api.startCpu(
				validOptions(),
				() => undefined,
				() => undefined,
			),
			/not authorized/,
		);
		assert.equal(harness.calls.addonImports, 0);
		assert.equal(harness.captures.length, 0);
	});
});
