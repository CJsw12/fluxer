// Official-app click crawler: every clickable [data-flx] control on a route is clicked from a
// clean baseline and the UI state it opens (popout / modal / menu / route change) is recorded with
// a screenshot and a DOM dump. The resulting state graph is the checklist the Rust port must match.
//
// Usage: node crawl.mjs --attach 9334 --origin fluxer-app://app --route /channels/<g>/<c>
//        --out DIR [--depth 2] [--max 400] [--width 1280 --height 800]
// Safety: window controls, navigation (channel/guild/DM switches) and destructive or outward
// actions (delete, leave, kick, ban, block, report, logout, send, call) are never clicked, and a
// CDP network guard fails every non-GET API request so no click can mutate server state
// (reactions, pins, settings). States and the click log are written as they are found.
import {mkdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {Cdp, DOM_DUMP, SVG_DUMP, clientInfo, helpers, pageWsUrl, sleep} from './lib.mjs';

const rawArgs = process.argv.slice(2);
const HOVER = rawArgs.includes('--hover');
const args = {};
for (let i = 0; i < rawArgs.length; i++) {
	const v = rawArgs[i];
	if (v === '--hover') continue;
	if (v.startsWith('--')) {
		args[v.slice(2)] = rawArgs[i + 1];
		i++;
	}
}
const PORT = Number(args.attach);
const ORIGIN = args.origin;
const ROUTE = args.route;
const OUT = args.out;
const DEPTH = Number(args.depth ?? 1);
const MAX = Number(args.max ?? 400);
const HOVER_MAX = Number(args['hover-max'] ?? 80);
const WIDTH = Number(args.width ?? 1280);
const HEIGHT = Number(args.height ?? 800);
if (!PORT || !ORIGIN || !ROUTE || !OUT) {
	console.error('usage: --attach PORT --origin ORIGIN --route PATH --out DIR [--hover] [--hover-max N]');
	process.exit(2);
}
mkdirSync(join(OUT, HOVER ? 'hover' : 'states'), {recursive: true});

const DENY = new RegExp(
	[
		'native-titlebar', 'minimize', 'maximize', 'close-window', 'log-?out', 'sign-?out', 'delete', 'leave',
		'kick', '\\bban\\b', 'block', 'report', 'revoke', 'deauthori', 'disable-account', 'send-message',
		'start-call', 'join-call', 'voice-call', 'video-call', 'disconnect', 'hang-?up', 'mark-as-read',
		'channel-item', 'guild-list-item', 'react', 'reaction', 'message-action', 'action-bar', 'dm-list-item', 'private-channel', 'nagbar', 'download',
		// Native OS dialogs / device capture would block an unattended crawl.
		'upload', 'attach', 'voice-message', 'record', 'camera', 'screen-share', 'go-live', 'stream', 'open-file', 'import',
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

// Write guard (in-page): the desktop app proxies its API through a custom scheme
// (fluxer-app://app/api/<encoded url>) that CDP Fetch cannot intercept, so fetch/XHR/beacon are
// patched in every document to fail any non-GET request to an API path before it is sent.
const WRITE_GUARD = `(() => {
  if (window.__parityGuard) return; window.__parityGuard = true; window.__parityBlocked = [];
  const isApi = (u) => String(u).includes('/api/') || String(u).includes('%2Fapi');
  const deny = (m, u) => { window.__parityBlocked.push(m + ' ' + String(u).slice(-80)); return new TypeError('parity write guard'); };
  const f = window.fetch;
  window.fetch = function (input, init) {
    const url = typeof input === 'string' ? input : input && input.url;
    const method = ((init && init.method) || (input && input.method) || 'GET').toUpperCase();
    if (isApi(url) && !['GET', 'HEAD', 'OPTIONS'].includes(method)) return Promise.reject(deny(method, url));
    return f.apply(this, arguments);
  };
  const open = XMLHttpRequest.prototype.open, send = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (m, u) { this.__pm = String(m).toUpperCase(); this.__pu = u; return open.apply(this, arguments); };
  XMLHttpRequest.prototype.send = function () {
    if (isApi(this.__pu) && !['GET', 'HEAD', 'OPTIONS'].includes(this.__pm)) { deny(this.__pm, this.__pu); throw new TypeError('parity write guard'); }
    return send.apply(this, arguments);
  };
  if (navigator.sendBeacon) navigator.sendBeacon = (u) => { if (isApi(u)) { deny('BEACON', u); return false; } return true; };
})()`;
await cdp.send('Page.addScriptToEvaluateOnNewDocument', {source: WRITE_GUARD});
await cdp.eval(WRITE_GUARD).catch(() => {});
const blockedWrites = () => cdp.eval('window.__parityBlocked || []').catch(() => []);
const blocked = [];
cdp.on('Fetch.requestPaused', async ({requestId, request}) => {
	if (['GET', 'HEAD', 'OPTIONS'].includes(request.method)) {
		await cdp.send('Fetch.continueRequest', {requestId}).catch(() => {});
	} else {
		blocked.push(`${request.method} ${request.url.replace(/^.*?\/api/, '/api')}`);
		await cdp.send('Fetch.failRequest', {requestId, errorReason: 'BlockedByClient'}).catch(() => {});
	}
});

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
	blocked.push(...(await blockedWrites()));
	await cdp.eval('window.__parityBlocked = []').catch(() => {});
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

const HOVER_STYLE_KEYS = [
	'backgroundColor', 'color', 'borderRadius', 'borderTopWidth', 'borderTopStyle', 'borderTopColor',
	'opacity', 'transform', 'boxShadow', 'cursor',
];
const HOVER_EXTENDED_KEYS = [
	'backgroundColor', 'color', 'fontSize', 'fontWeight', 'borderRadius', 'borderTopWidth', 'borderTopColor',
	'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'boxShadow',
];

async function collectHoverTargets() {
	return cdp.eval(`(() => {
	  const seen = new Map();
	  const out = [];
	  const addEl = (el, hostOverride) => {
	    if (!el) return;
	    const host = hostOverride || (el.hasAttribute('data-flx') ? el : el.closest('[data-flx]'));
	    if (!host) return;
	    const r = el.getBoundingClientRect();
	    if (r.width < 4 || r.height < 4 || r.top < 0 || r.left < 0 || r.bottom > innerHeight || r.right > innerWidth) return;
	    const cs = getComputedStyle(el);
	    if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) === 0) return;
	    const flx = host.getAttribute('data-flx');
	    if (/native-titlebar|minimize|maximize|close-window/i.test(flx)) return;
	    const all = [...document.querySelectorAll('[data-flx="' + CSS.escape(flx) + '"]')];
	    const idx = all.indexOf(host);
	    if (idx < 0) return;
	    const key = flx + '#' + idx;
	    if (seen.has(key)) return;
	    seen.set(key, true);
	    out.push({
	      flx, idx,
	      label: host.getAttribute('aria-label') || el.getAttribute('aria-label') || '',
	      text: (host.innerText || '').trim().slice(0, 60),
	      rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
	      cx: Math.round(r.left + r.width / 2),
	      cy: Math.round(r.top + r.height / 2),
	    });
	  };
	  for (const el of document.querySelectorAll('button,a[href],[role="button"],[role="tab"],[role="menuitem"],[role="separator"]')) addEl(el);
	  for (const el of document.querySelectorAll('[data-flx^="app.sidebar-nav."]')) addEl(el, el);
	  addEl(document.querySelector('[data-flx="app.guild-header.header-wrapper"]'));
	  for (const el of document.querySelectorAll('[data-flx="app.generic-channel-item.long-pressable.click"]')) addEl(el, el);
	  for (const el of [...document.querySelectorAll('[data-flx="channel.message.article.alt-click"]')].slice(0, 3)) addEl(el, el);
	  for (const el of [...document.querySelectorAll('[data-flx="channel.member-list-item.content"]')].slice(0, 2)) addEl(el, el);
	  for (const el of document.querySelectorAll('[data-flx^="app.user-area."]')) addEl(el, el);
	  return out;
	})()`);
}

async function runHoverSweep() {
	await shot(join(OUT, 'baseline.png'));
	writeFileSync(join(OUT, 'baseline.dom.json'), JSON.stringify(baseline));
	const targets = (await collectHoverTargets()).slice(0, HOVER_MAX);
	const hoverStates = [];
	try {
		for (const t of targets) {
			await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: WIDTH - 40, y: HEIGHT - 40});
			await sleep(350);
			const before = await dump();
			const beforeIds = idSet(before);
			const beforeStyle = await cdp.eval(`(() => {
			  const el = ${target(t)};
			  if (!el) return null;
			  const cs = getComputedStyle(el);
			  const s = {}; for (const k of ${JSON.stringify(HOVER_STYLE_KEYS)}) s[k] = cs[k];
			  return s;
			})()`);
			if (!beforeStyle) continue;
			await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: t.cx, y: t.cy});
			await sleep(700);
			const after = await dump();
			const afterIds = idSet(after);
			const {afterStyle, tooltips} = await cdp.eval(`(() => {
			  const el = ${target(t)};
			  const cs = el ? getComputedStyle(el) : null;
			  const afterStyle = {};
			  if (cs) for (const k of ${JSON.stringify(HOVER_STYLE_KEYS)}) afterStyle[k] = cs[k];
			  const tooltips = [];
			  for (const tt of document.querySelectorAll('[role="tooltip"]')) {
			    const r = tt.getBoundingClientRect();
			    if (r.width < 4 || r.height < 4 || r.bottom < 0 || r.right < 0 || r.top > innerHeight || r.left > innerWidth) continue;
			    const tcs = getComputedStyle(tt);
			    if (tcs.visibility === 'hidden' || tcs.display === 'none' || Number(tcs.opacity) === 0) continue;
			    const style = {}; for (const k of ${JSON.stringify(HOVER_EXTENDED_KEYS)}) style[k] = tcs[k];
			    tooltips.push({
			      flx: tt.getAttribute('data-flx') || '',
			      rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
			      text: (tt.innerText || '').trim().slice(0, 200),
			      style,
			    });
			  }
			  return {afterStyle, tooltips};
			})()`);
			const addedElements = after.filter((e) => !beforeIds.has(base(e.flx)));
			const styleDiff = {};
			for (const k of HOVER_STYLE_KEYS) {
				if (beforeStyle[k] !== afterStyle[k]) styleDiff[k] = [beforeStyle[k], afterStyle[k]];
			}
			if (addedElements.length > 0 || tooltips.length > 0 || Object.keys(styleDiff).length > 0) {
				const id = String(hoverStates.length + 1).padStart(3, '0');
				const s = await cdp.send('Page.captureScreenshot', {format: 'png'});
				writeFileSync(join(OUT, 'hover', `${id}.png`), Buffer.from(s.data, 'base64'));
				hoverStates.push({
					id, flx: t.flx, idx: t.idx, label: t.label, text: t.text, rect: t.rect,
					added_elements: addedElements, tooltips, style_diff: styleDiff,
				});
				console.log(`${id} ${t.flx}#${t.idx} (${t.label || t.text}) → +${addedElements.length} tooltips=${tooltips.length} style=${Object.keys(styleDiff).join(',')}`);
			}
		}
	} finally {
		await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: WIDTH - 40, y: HEIGHT - 40}).catch(() => {});
		await cdp.send('Emulation.clearDeviceMetricsOverride').catch(() => {});
	}
	blocked.push(...(await blockedWrites()));
	const outGraph = {
		route: ROUTE, client: await clientInfo(cdp), width: WIDTH, height: HEIGHT,
		total_targets: targets.length, changed_count: hoverStates.length,
		blocked_writes: blocked, hover_states: hoverStates,
	};
	writeFileSync(join(OUT, 'hover_graph.json'), JSON.stringify(outGraph, null, 1));
	console.log(`hover sweep done: ${hoverStates.length}/${targets.length} changed`);
}

if (HOVER) {
	await runHoverSweep();
	cdp.ws.close();
	process.exit(0);
}

await shot(join(OUT, 'baseline.png'));
writeFileSync(join(OUT, 'baseline.dom.json'), JSON.stringify(baseline));

const states = [];
const writeGraph = () =>
	writeFileSync(
		join(OUT, 'graph.json'),
		JSON.stringify({route: ROUTE, width: WIDTH, height: HEIGHT, clicks, baseline: baseline.length, blocked_writes: blocked, states}, null, 1),
	);
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
		// Icon originals visible in this state (identified later against the Phosphor set).
		writeFileSync(join(OUT, 'states', `${id}.svgs.json`), JSON.stringify(await cdp.eval(SVG_DUMP)));
		const state = {
			id, trail, route_after: route, layer: isLayer, toggle_reverted: !isLayer && route === ROUTE,
			added, removed_count: removed.length, also_opened_by: [],
		};
		states.push(state);
		bySignature.set(signature, state);
		writeGraph();
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
