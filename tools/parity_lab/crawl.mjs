// Official-app click crawler: every clickable [data-flx] control on a route is clicked from a
// clean baseline and the UI state it opens (popout / modal / menu / route change) is recorded with
// a screenshot and a DOM dump. The resulting state graph is the checklist the Rust port must match.
//
// Usage: node crawl.mjs --attach 9334 --origin fluxer-app://app --route /channels/<g>/<c>
//        --out DIR [--depth 2] [--max 400] [--width 1280 --height 800]
// Safety: window controls, navigation (channel/guild/DM switches) and destructive or outward
// actions (delete, leave, kick, ban, block, report, logout, send, call) are never clicked.
import {mkdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {Cdp, DOM_DUMP, clientInfo, helpers, pageWsUrl, sleep} from './lib.mjs';

const args = Object.fromEntries(
	process.argv.slice(2).reduce((acc, v, i, a) => (v.startsWith('--') ? [...acc, [v.slice(2), a[i + 1]]] : acc), []),
);
const PORT = Number(args.attach);
const ORIGIN = args.origin;
const ROUTE = args.route;
const OUT = args.out;
const DEPTH = Number(args.depth ?? 1);
const MAX = Number(args.max ?? 400);
const WIDTH = Number(args.width ?? 1280);
const HEIGHT = Number(args.height ?? 800);
if (!PORT || !ORIGIN || !ROUTE || !OUT) {
	console.error('usage: --attach PORT --origin ORIGIN --route PATH --out DIR');
	process.exit(2);
}
mkdirSync(join(OUT, 'states'), {recursive: true});

const DENY = new RegExp(
	[
		'native-titlebar', 'minimize', 'maximize', 'close-window', 'log-?out', 'sign-?out', 'delete', 'leave',
		'kick', '\\bban\\b', 'block', 'report', 'revoke', 'deauthori', 'disable-account', 'send-message',
		'start-call', 'join-call', 'voice-call', 'video-call', 'disconnect', 'hang-?up', 'mark-as-read',
		'channel-item', 'guild-list-item', 'dm-list-item', 'private-channel', 'nagbar', 'download',
	].join('|'),
	'i',
);
const LAYER = /popout|modal|dialog|menu|context|tooltip|picker|sheet|layer|overlay|drawer|panel|flyout/i;
const base = (flx) => flx.replace(/--\d+$/, '');

const cdp = new Cdp(await pageWsUrl(PORT));
await cdp.open();
await cdp.send('Page.enable');
await cdp.send('Runtime.enable');
await cdp.send('Emulation.setDeviceMetricsOverride', {width: WIDTH, height: HEIGHT, deviceScaleFactor: 1, mobile: false});
await cdp.send('Emulation.setEmulatedMedia', {features: [{name: 'prefers-reduced-motion', value: 'reduce'}]});
const {waitFor, realClickExpr, pressKey} = helpers(cdp);

const dump = () => cdp.eval(DOM_DUMP);
const idSet = (d) => new Set(d.map((e) => base(e.flx)));
async function shot(file) {
	await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: WIDTH - 40, y: HEIGHT - 40});
	await sleep(250);
	const s = await cdp.send('Page.captureScreenshot', {format: 'png'});
	writeFileSync(file, Buffer.from(s.data, 'base64'));
}

async function loadRoute() {
	await cdp.send('Page.navigate', {url: `${ORIGIN}${ROUTE}`});
	await waitFor(`!!document.querySelector('[data-flx*="channel-header"], [data-flx*="guild-list"]')`, 30000);
	await sleep(2500);
	// Dismiss nag bars, which would shift the layout and are not part of the crawl.
	for (let i = 0; i < 6; i++) {
		const hit = await realClickExpr(`document.querySelector('[data-flx="app.nagbar.dismiss-button"]')`);
		if (!hit) break;
		await sleep(700);
		await realClickExpr(
			`[...document.querySelectorAll('button')].find(b => /^Dismiss anyway$/i.test(b.innerText.trim()))`,
		);
		await sleep(700);
	}
}

// Back to the baseline without a reload when Escape is enough.
async function reset(baselineIds) {
	for (let i = 0; i < 3; i++) {
		await pressKey('Escape', 27);
		await sleep(250);
	}
	const here = await cdp.eval('location.pathname + location.search');
	const ids = idSet(await dump());
	const extra = [...ids].filter((x) => !baselineIds.has(x)).length;
	const missing = [...baselineIds].filter((x) => !ids.has(x)).length;
	// Any residue (toggled member list, open layer, scrolled pane) invalidates the next diff.
	if (here !== ROUTE || extra > 0 || missing > 0) await loadRoute();
}

// Clickable controls inside an optional subset of flx ids (for depth-2 crawling inside a layer).
async function candidates(within) {
	const list = await cdp.eval(`(() => {
	  const sel = 'button,a[href],[role="button"],[role="tab"],[role="menuitem"],[role="menuitemradio"],[role="menuitemcheckbox"],[role="switch"],[role="checkbox"],[role="radio"],[role="option"],[role="treeitem"]';
	  const seen = new Map(); const out = [];
	  for (const el of document.querySelectorAll(sel)) {
	    const host = el.hasAttribute('data-flx') ? el : el.closest('[data-flx]');
	    if (!host) continue;
	    const r = el.getBoundingClientRect();
	    if (r.width < 4 || r.height < 4 || r.top < 0 || r.left < 0 || r.bottom > innerHeight || r.right > innerWidth) continue;
	    const cs = getComputedStyle(el);
	    if (cs.visibility === 'hidden' || Number(cs.opacity) === 0 || el.disabled || el.getAttribute('aria-disabled') === 'true') continue;
	    const flx = host.getAttribute('data-flx');
	    const all = [...document.querySelectorAll('[data-flx="' + CSS.escape(flx) + '"]')];
	    const idx = all.indexOf(host);
	    const key = flx + '#' + idx;
	    if (seen.has(key)) continue; seen.set(key, true);
	    out.push({flx, idx, tag: el.tagName.toLowerCase(), role: el.getAttribute('role'),
	      label: el.getAttribute('aria-label') ?? '', text: (el.innerText ?? '').trim().slice(0, 60),
	      href: el.getAttribute('href'), rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)]});
	  }
	  return out;
	})()`);
	return list.filter((c) => {
		if (within && !within.has(base(c.flx))) return false;
		if (c.href && /^https?:/i.test(c.href)) return false;
		return !DENY.test(`${c.flx} ${c.label} ${c.text}`);
	});
}

const target = (c) => `[...document.querySelectorAll('[data-flx="${c.flx.replace(/"/g, '\\"')}"]')][${c.idx}]`;

await loadRoute();
const baseline = await dump();
const baselineIds = idSet(baseline);
await shot(join(OUT, 'baseline.png'));
writeFileSync(join(OUT, 'baseline.dom.json'), JSON.stringify(baseline));

const states = [];
const bySignature = new Map();
let clicks = 0;

async function explore(path, within, depth) {
	const list = await candidates(within);
	for (const c of list) {
		if (clicks >= MAX) return;
		await reset(baselineIds);
		// Re-open the parent chain before clicking a nested control.
		for (const p of path) {
			await realClickExpr(target(p));
			await sleep(700);
		}
		const before = idSet(await dump());
		const ok = await realClickExpr(target(c));
		clicks++;
		if (!ok) continue;
		await sleep(900);
		const after = await dump();
		const afterIds = idSet(after);
		const added = [...afterIds].filter((x) => !before.has(x)).sort();
		const removed = [...before].filter((x) => !afterIds.has(x)).sort();
		const route = await cdp.eval('location.pathname + location.search');
		const isLayer = added.some((a) => LAYER.test(a));
		if (!isLayer && route === ROUTE) {
			// In-place toggle (favorite, mute, collapse): undo it so the test account state is kept.
			await realClickExpr(target(c));
			await sleep(500);
		}
		if (added.length === 0 && route === ROUTE) continue; // hover/toggle with no new UI
		const signature = `${route}|${added.join(',')}`;
		const trail = [...path, c].map((p) => ({flx: p.flx, idx: p.idx, label: p.label, text: p.text}));
		if (bySignature.has(signature)) {
			bySignature.get(signature).also_opened_by.push(trail);
			continue;
		}
		const id = String(states.length + 1).padStart(3, '0');
		await shot(join(OUT, 'states', `${id}.png`));
		writeFileSync(join(OUT, 'states', `${id}.dom.json`), JSON.stringify(after));
		const state = {
			id, trail, route_after: route, layer: isLayer, toggle_reverted: !isLayer && route === ROUTE,
			added, removed_count: removed.length, also_opened_by: [],
		};
		states.push(state);
		bySignature.set(signature, state);
		console.log(`${id} ${trail.map((t) => t.label || t.text || t.flx).join(' > ')} → +${added.length}${route !== ROUTE ? ` route ${route}` : ''}`);
		if (depth < DEPTH && state.layer && route === ROUTE) {
			await explore([...path, c], new Set(added), depth + 1);
		}
	}
}

await explore([], null, 1);
await reset(baselineIds);
writeFileSync(
	join(OUT, 'graph.json'),
	JSON.stringify({route: ROUTE, client: await clientInfo(cdp), width: WIDTH, height: HEIGHT, clicks, baseline: baseline.length, states}, null, 1),
);
console.log(`done: ${states.length} distinct states from ${clicks} clicks`);
cdp.ws.close();
