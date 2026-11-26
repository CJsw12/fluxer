#!/usr/bin/env node
// Read-only mic/speaker state of the official (CDP-attached) app.
//
//   node tools/parity_lab/mic_state.mjs [--attach 9334]
//
// Prints {"official": [aria-label...], "mic_on": bool, "speaker_on": bool} and exits 0.
// mic_on is true when the user-area control still offers "Mute microphone" (i.e. the mic is not
// muted); speaker_on is true when it offers "Deafen yourself". Exits 2 when the CDP endpoint is
// not reachable within 15 s. Never clicks, types or writes anything.

import {Cdp, pageWsUrl} from './lib.mjs';

const SEL = '[data-flx^="app.user-area.user-area-inner.control-button"]';
const MIC_LABEL = 'Mute microphone';
const SPEAKER_LABEL = 'Deafen yourself';
const BUDGET_MS = 15000;

function parseArgs(argv) {
	let attach = 9334;
	for (let i = 0; i < argv.length; i++) {
		if (argv[i] === '--attach') {
			attach = Number(argv[++i]);
		} else if (argv[i] === '--help' || argv[i] === '-h') {
			console.log('usage: node tools/parity_lab/mic_state.mjs [--attach PORT]');
			process.exit(0);
		} else {
			console.error(`mic_state: unknown argument: ${argv[i]}`);
			process.exit(2);
		}
	}
	if (!Number.isInteger(attach) || attach <= 0 || attach > 65535) {
		console.error('mic_state: invalid --attach port');
		process.exit(2);
	}
	return {attach};
}

const deadline = Date.now() + BUDGET_MS;
const left = () => Math.max(1, deadline - Date.now());
const withDeadline = (promise, what) =>
	Promise.race([
		promise,
		new Promise((_, reject) => {
			const timer = setTimeout(() => reject(new Error(`${what} timed out after ${BUDGET_MS}ms`)), left());
			timer.unref?.();
		}),
	]);

let cdp = null;
try {
	const {attach} = parseArgs(process.argv.slice(2));
	const url = await withDeadline(pageWsUrl(attach, 60), `CDP endpoint on ${attach}`);
	cdp = new Cdp(url, {timeoutMs: Math.min(8000, left())});
	await withDeadline(cdp.open(), 'CDP socket');
	const labels = await withDeadline(
		cdp.eval(`[...document.querySelectorAll(${JSON.stringify(SEL)})]
			.map((el) => el.getAttribute('aria-label'))
			.filter((label) => !!label)`),
		'aria-label query',
	);
	const official = Array.isArray(labels) ? labels.map((label) => String(label)) : [];
	console.log(
		JSON.stringify({
			official,
			mic_on: official.includes(MIC_LABEL),
			speaker_on: official.includes(SPEAKER_LABEL),
		}),
	);
	try {
		cdp.ws.close();
	} catch {}
	process.exit(0);
} catch (err) {
	const message = err && err.message ? err.message : String(err);
	console.error(`mic_state: ${message}`);
	try {
		cdp?.ws?.close();
	} catch {}
	process.exit(2);
}
