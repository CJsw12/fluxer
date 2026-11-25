// Extracts a per-element machine-readable spec of the official app's current screen: geometry,
// resolved style, paint order, floating state, matched CSS-module rules (raw values, var() kept)
// and icon path signature for every [data-flx] element. Consumed by spec_delta.py.
// Single-screen mode is strictly read-only: no clicks, navigation or storage writes. --scenes mode
// opens read-only states (popouts, modals, pickers) with real CDP input, captures each into
// <out-dir>/<slug>.spec.json and closes it again (write guard + deny-label list). :hover forced via
// CSS.forcePseudoState is always reverted (also on error).
//   node spec_official.mjs --attach 9334 --out <spec.json> [--hover <flx>]... [--viewport 1280x800]
import {mkdirSync, readdirSync, readFileSync, statSync, writeFileSync} from 'node:fs';
import {dirname, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {Cdp, clientInfo, helpers, pageWsUrl, sleep} from './lib.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '../..');
const DEFAULT_EXPECTED_WEB_BUILD = 'Web 2026.1120';
const argv = process.argv.slice(2);
const opts = {attach: null, out: null, viewport: null, hover: [], scenes: null, outDir: null, expectedWebBuild: null};
for (let i = 0; i < argv.length; i++) {
	const a = argv[i];
	if (a === '--attach') opts.attach = Number(argv[++i]);
	else if (a === '--out') opts.out = argv[++i];
	else if (a === '--viewport') opts.viewport = argv[++i];
	else if (a === '--hover') opts.hover.push(argv[++i]);
	else if (a === '--scenes') opts.scenes = argv[++i];
	else if (a === '--out-dir') opts.outDir = argv[++i];
	else if (a === '--expected-web-build') opts.expectedWebBuild = argv[++i];
}
if (!opts.attach || !(opts.out || (opts.scenes && opts.outDir))) {
	console.error('usage: node spec_official.mjs --attach <port> --out <spec.json> [--hover <flx>]... [--viewport 1280x800] [--expected-web-build <ver>]');
	console.error('   or: node spec_official.mjs --attach <port> --scenes <scenes.json> --out-dir <dir> [--viewport 1280x800] [--expected-web-build <ver>]');
	process.exit(2);
}

function resolveExpectedWebBuild() {
	if (opts.expectedWebBuild) return opts.expectedWebBuild.trim();
	try {
		const baseline = JSON.parse(readFileSync(join(HERE, 'spec_baseline.json'), 'utf8'));
		if (typeof baseline.official_client === 'string' && baseline.official_client.trim()) {
			return baseline.official_client.trim();
		}
	} catch {}
	return DEFAULT_EXPECTED_WEB_BUILD;
}

function extractWebBuild(info) {
	if (typeof info !== 'string') return null;
	const m = info.match(/Web\s+(\d{4}\.\d+(?:\.\d+)?)/i);
	return m ? `Web ${m[1]}` : null;
}

function checkVersionWarning(clientInfoStr, expectedRaw) {
	const actualWeb = extractWebBuild(clientInfoStr);
	const expectedWeb = extractWebBuild(expectedRaw) ?? (expectedRaw ? `Web ${expectedRaw.replace(/^Web\s+/i, '').trim()}` : null);
	if (!actualWeb || !expectedWeb) return null;
	const exact = actualWeb.toLowerCase() === expectedWeb.toLowerCase();
	const prefixMatch = !expectedWeb.slice(4).includes('.') ? false : actualWeb.toLowerCase().startsWith(`${expectedWeb.toLowerCase()}.`);
	if (exact || prefixMatch) return null;
	return {
		expected: expectedWeb,
		actual: actualWeb,
		client_info: clientInfoStr,
		message: `official web build mismatch: expected ${expectedWeb}, got ${actualWeb} (${clientInfoStr})`,
	};
}

const PROPS = [
	'display', 'position', 'zIndex', 'flexDirection', 'alignItems', 'justifyContent', 'rowGap', 'columnGap',
	'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
	'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth',
	'borderTopColor', 'borderRightColor', 'borderBottomColor', 'borderLeftColor',
	'borderTopLeftRadius', 'borderTopRightRadius', 'borderBottomRightRadius', 'borderBottomLeftRadius',
	'backgroundColor', 'color', 'fontSize', 'fontWeight', 'lineHeight', 'letterSpacing', 'textTransform', 'opacity',
	'boxShadow', 'outline', 'outlineWidth', 'outlineColor', 'overflowX', 'overflowY', 'transform',
];

// Page side: every [data-flx] in document order. `rendered` = has a layout box; idx counts rendered
// elements per flx only so it lines up with what the Rust probe registers.
function pageSpec(props) {
	const norm = (d) => d.replace(/\s+/g, ' ').trim();
	const all = [...document.querySelectorAll('[data-flx]')];
	const counts = new Map();
	const info = new Map();
	const rows = all.map((el, order) => {
		const flx = el.getAttribute('data-flx');
		const cs = getComputedStyle(el);
		const r = el.getBoundingClientRect();
		// scenes with `drop_hidden` (hover bars exist hidden on every message) skip visibility:hidden subtrees
		const rendered = el.getClientRects().length > 0 && cs.display !== 'none' && !(window.__dropHidden && cs.visibility === 'hidden');
		let idx = null;
		if (rendered) {
			idx = counts.get(flx) ?? 0;
			counts.set(flx, idx + 1);
		}
		const computed = {};
		for (const p of props) computed[p] = cs[p];
		const parent = el.parentElement ? el.parentElement.closest('[data-flx]') : null;
		let zEff = 0;
		for (let a = el; a; a = a.parentElement) {
			const z = parseInt(getComputedStyle(a).zIndex, 10);
			if (!Number.isNaN(z)) {
				zEff = z;
				break;
			}
		}
		info.set(el, {zEff});
		let icon = null;
		const svg = el.matches('svg') ? el : [...el.querySelectorAll('svg')].find((s) => s.closest('[data-flx]') === el);
		if (svg) {
			icon = {
				viewBox: svg.getAttribute('viewBox'),
				d: [...svg.querySelectorAll('path')].map((p) => norm(p.getAttribute('d') || '')).join('|'),
			};
		}
		const pr = parent ? parent.getBoundingClientRect() : null;
		const pz = parent ? (info.get(parent)?.zEff ?? 0) : 0;
		const pos = cs.position;
		let floating = false;
		let reason = null;
		if (pos === 'absolute' || pos === 'fixed') {
			floating = true;
			reason = pos;
		} else if (parent && pos !== 'static' && cs.zIndex !== 'auto' && zEff > pz) {
			floating = true;
			reason = 'stacking';
		}
		return {
			flx,
			idx,
			order,
			rendered,
			tag: el.tagName.toLowerCase(),
			rect: [r.x, r.y, r.width, r.height],
			rel: pr ? [r.x - pr.x, r.y - pr.y] : null,
			parent_flx: parent ? parent.getAttribute('data-flx') : null,
			in_viewport: rendered && r.right > 0 && r.bottom > 0 && r.left < innerWidth && r.top < innerHeight,
			computed,
			floating,
			floating_reason: reason,
			icon,
		};
	});
	return {rows, theme: document.documentElement.className, viewport: [innerWidth, innerHeight]};
}

// Computed props of the flx elements inside `this` (the hover root) and itself, keyed by order.
function pageProps(props) {
	const out = {};
	const all = [...document.querySelectorAll('[data-flx]')];
	all.forEach((el, order) => {
		if (el !== this && !this.contains(el)) return;
		const cs = getComputedStyle(el);
		const r = el.getBoundingClientRect();
		const o = {};
		for (const p of props) o[p] = cs[p];
		o.rect = [r.x, r.y, r.width, r.height].map((v) => Math.round(v * 100) / 100).join(',');
		out[order] = o;
	});
	return out;
}

function indexModules() {
	const map = new Map();
	const walk = (dir) => {
		let names = [];
		try {
			names = readdirSync(dir);
		} catch {
			return;
		}
		for (const n of names) {
			if (n === 'node_modules' || n.startsWith('.')) continue;
			const p = join(dir, n);
			let st;
			try {
				st = statSync(p);
			} catch {
				continue;
			}
			if (st.isDirectory()) walk(p);
			else if (n.endsWith('.module.css') && !map.has(n)) map.set(n, relative(REPO, p).split('\\').join('/'));
		}
	};
	walk(join(REPO, 'fluxer_app/src'));
	return map;
}

const MODULE_RE = /([A-Za-z0-9_-]+)\.module__([A-Za-z0-9_-]+?)___[A-Za-z0-9_-]+/g;
const STATE_RE = /:(hover|focus-visible|focus-within|focus|active|disabled|checked|visited)/;

function moduleRules(matched, modules) {
	const out = [];
	for (const m of matched.matchedCSSRules ?? []) {
		const rule = m.rule;
		if (rule.origin !== 'regular') continue;
		const sels = rule.selectorList?.selectors ?? [];
		for (const si of m.matchingSelectors ?? []) {
			const text = (sels[si]?.text ?? '').split('\\').join('');
			let last = null;
			for (const mm of text.matchAll(MODULE_RE)) last = mm;
			if (!last) continue;
			const state = (text.slice(last.index + last[0].length).match(STATE_RE) ?? [])[1] ?? null;
			const file = `${last[1]}.module.css`;
			const props = {};
			for (const p of rule.style?.cssProperties ?? []) {
				if (p.disabled || p.implicit || p.parsedOk === false || !p.name) continue;
				props[p.name] = p.value;
			}
			out.push({
				file: modules.get(file) ?? file,
				class: last[2],
				selector: text,
				state,
				media: rule.media?.length ? rule.media.map((x) => x.text).join(', ') : null,
				props,
			});
		}
	}
	const inline = {};
	for (const p of matched.inlineStyle?.cssProperties ?? []) if (p.name && p.value && !p.disabled) inline[p.name] = p.value;
	if (Object.keys(inline).length) out.push({file: '(inline)', class: null, selector: 'style=""', state: null, media: null, props: inline});
	return out;
}

const cdp = new Cdp(await pageWsUrl(opts.attach));
await cdp.open();
const forced = new Set();
let exitCode = 0;
const modules = indexModules();

async function capture(hoverList) {
	const page = await cdp.eval(`(${pageSpec.toString()})(${JSON.stringify(PROPS)})`);
	const rows = page.rows;

	// Paint order: snapshot layout nodes carrying data-flx, joined by (flx, occurrence).
	const snap = await cdp.send('DOMSnapshot.captureSnapshot', {computedStyles: [], includePaintOrder: true, includeDOMRects: false});
	const doc = snap.documents[0];
	const str = (i) => (i >= 0 ? snap.strings[i] : null);
	const flxOf = new Map();
	doc.nodes.attributes.forEach((attrs, ni) => {
		for (let k = 0; k + 1 < attrs.length; k += 2) if (str(attrs[k]) === 'data-flx') flxOf.set(ni, str(attrs[k + 1]));
	});
	const paint = new Map();
	const seen = new Map();
	doc.layout.nodeIndex.forEach((ni, li) => {
		const f = flxOf.get(ni);
		if (f === undefined) return;
		const n = seen.get(f) ?? 0;
		seen.set(f, n + 1);
		paint.set(`${f}|${n}`, doc.layout.paintOrders?.[li] ?? null);
	});

	const root = await cdp.send('DOM.getDocument', {depth: 0});
	const q = await cdp.send('DOM.querySelectorAll', {nodeId: root.root.nodeId, selector: '[data-flx]'});
	if (q.nodeIds.length !== rows.length) throw new Error(`DOM/page flx count mismatch ${q.nodeIds.length} vs ${rows.length}`);

	const matchedAll = await Promise.all(
		q.nodeIds.map((id, i) =>
			rows[i].rendered ? cdp.send('CSS.getMatchedStylesForNode', {nodeId: id}).catch(() => null) : null,
		),
	);

	const elements = [];
	const orderOf = [];
	rows.forEach((r, i) => {
		if (!r.rendered) return;
		const m = matchedAll[i];
		orderOf.push(i);
		elements.push({
			flx: r.flx,
			idx: r.idx,
			tag: r.tag,
			parent_flx: r.parent_flx,
			rect: r.rect.map((v) => Math.round(v * 100) / 100),
			rel: r.rel ? r.rel.map((v) => Math.round(v * 100) / 100) : null,
			in_viewport: r.in_viewport,
			paint_order: paint.get(`${r.flx}|${r.idx}`) ?? null,
			floating: r.floating,
			floating_reason: r.floating_reason,
			computed: r.computed,
			css_rules: m ? moduleRules(m, modules) : [],
			icon: r.icon,
		});
	});

	// Hover: force :hover on the first element with that flx and 3 ancestors, diff computed props.
	const hovers = {};
	const nodeFor = async (expr) => {
		const ev = await cdp.send('Runtime.evaluate', {expression: expr, returnByValue: false});
		if (!ev.result.objectId) return null;
		const rn = await cdp.send('DOM.requestNode', {objectId: ev.result.objectId});
		return {nodeId: rn.nodeId, objectId: ev.result.objectId};
	};
	const callProps = async (objectId) => {
		const r = await cdp.send('Runtime.callFunctionOn', {
			objectId,
			functionDeclaration: `function(){ return (${pageProps.toString()}).call(this, ${JSON.stringify(PROPS)}); }`,
			returnByValue: true,
		});
		return r.result.value ?? {};
	};
	for (const flx of hoverList) {
		const q1 = JSON.stringify(flx);
		const chain = [];
		for (let up = 0; up <= 3; up++) {
			const n = await nodeFor(
				`(() => { let e = [...document.querySelectorAll('[data-flx]')].find((x) => x.getAttribute('data-flx') === ${q1}); for (let i = 0; e && i < ${up}; i++) e = e.parentElement; return e && e !== document.documentElement && e !== document.body ? e : null; })()`,
			);
			if (n) chain.push(n);
		}
		if (!chain.length) {
			hovers[flx] = {error: 'not found'};
			continue;
		}
		const top = chain[chain.length - 1];
		const before = await callProps(top.objectId);
		try {
			for (const n of chain) {
				forced.add(n.nodeId);
				await cdp.send('CSS.forcePseudoState', {nodeId: n.nodeId, forcedPseudoClasses: ['hover']});
			}
			await new Promise((r) => setTimeout(r, 400)); // let transitions settle
			const after = await callProps(top.objectId);
			const deltas = [];
			for (const [order, a] of Object.entries(after)) {
				const b = before[order];
				if (!b) continue;
				const changed = {};
				for (const [k, v] of Object.entries(a)) if (b[k] !== v) changed[k] = {from: b[k], to: v};
				if (Object.keys(changed).length) {
					const row = rows[Number(order)];
					deltas.push({flx: row.flx, idx: row.idx, changed});
				}
			}
			hovers[flx] = {forced_levels: chain.length, deltas};
		} finally {
			for (const n of chain) {
				await cdp.send('CSS.forcePseudoState', {nodeId: n.nodeId, forcedPseudoClasses: []}).catch(() => {});
				forced.delete(n.nodeId);
			}
		}
	}
	for (const e of elements) {
		const hd = {};
		for (const [flx, h] of Object.entries(hovers)) {
			const hit = (h.deltas ?? []).find((d) => d.flx === e.flx && d.idx === e.idx);
			if (hit) hd[flx] = hit.changed;
		}
		if (Object.keys(hd).length) e.hover_delta = hd;
	}

	const client = await clientInfo(cdp);
	const expectedWebBuild = resolveExpectedWebBuild();
	const versionWarning = checkVersionWarning(client, expectedWebBuild);
	if (versionWarning) console.error(`WARN: ${versionWarning.message}`);

	const spec = {
		version: 1,
		client_info: client,
		expected_web_build: expectedWebBuild,
		...(versionWarning ? {version_warning: versionWarning} : {}),
		theme_class: page.theme,
		viewport: page.viewport,
		hover_requests: hoverList,
		hover_errors: Object.fromEntries(Object.entries(hovers).filter(([, h]) => h.error).map(([k, h]) => [k, h.error])),
		elements,
	};
	return spec;
}

// ---- scene mode helpers
const DENY = /\b(delete|leave|remove|ban|kick|mute|pin|unpin|report|block|send|favorite|favourite|logout|log-out|sign-out)\b/i;
const WRITE_GUARD = `(() => {
  if (window.__specGuard) return; window.__specGuard = true; window.__specBlocked = [];
  const isApi = (u) => String(u).includes('/api/') || String(u).includes('%2Fapi');
  const deny = (m, u) => { window.__specBlocked.push(m + ' ' + String(u).slice(-80)); return new TypeError('parity write guard'); };
  const f = window.fetch, open = XMLHttpRequest.prototype.open, send = XMLHttpRequest.prototype.send, beacon = navigator.sendBeacon;
  window.__parityUnguard = () => { window.fetch = f; XMLHttpRequest.prototype.open = open; XMLHttpRequest.prototype.send = send; if (beacon) navigator.sendBeacon = beacon; window.__specGuard = false; };
  window.fetch = function (input, init) {
    const url = typeof input === 'string' ? input : input && input.url;
    const method = ((init && init.method) || (input && input.method) || 'GET').toUpperCase();
    if (isApi(url) && !['GET', 'HEAD', 'OPTIONS'].includes(method)) return Promise.reject(deny(method, url));
    return f.apply(this, arguments);
  };
  XMLHttpRequest.prototype.open = function (m, u) { this.__pm = String(m).toUpperCase(); this.__pu = u; return open.apply(this, arguments); };
  XMLHttpRequest.prototype.send = function () {
    if (isApi(this.__pu) && !['GET', 'HEAD', 'OPTIONS'].includes(this.__pm)) { deny(this.__pm, this.__pu); throw new TypeError('parity write guard'); }
    return send.apply(this, arguments);
  };
  if (navigator.sendBeacon) navigator.sendBeacon = (u) => { if (isApi(u)) { deny('BEACON', u); return false; } return true; };
})()`;

async function runScenes() {
	const scenes = JSON.parse(readFileSync(opts.scenes, 'utf8'));
	const {realClickExpr, realClick, pressKey, waitFor, clickText} = helpers(cdp);
	await cdp.send('Page.enable').catch(() => {});
	const guardDoc = await cdp.send('Page.addScriptToEvaluateOnNewDocument', {source: WRITE_GUARD}).catch(() => null);
	await cdp.eval(WRITE_GUARD);
	const shown = (flx) => `[...document.querySelectorAll('[data-flx]')].some((e) => e.getAttribute('data-flx') === ${JSON.stringify(flx)} && e.getClientRects().length > 0)`;
	const route = () => cdp.eval('location.pathname + location.search');
	const baseRoute = await route();
	const baseOrigin = await cdp.eval('location.origin');
	const MUTE_SEL = '[data-flx="app.user-area.user-area-inner.control-button.mute"]';
	const DEAFEN_SEL = '[data-flx="app.user-area.user-area-inner.control-button.deafen"]';
	const VOICE_CONN_SEL = '[data-flx="app.user-area.user-area-inner.voice-connection-wrapper"]';
	const getAria = (sel) => cdp.eval(`document.querySelector(${JSON.stringify(sel)})?.getAttribute('aria-label') ?? null`).catch(() => null);
	const initialMicLabel = await getAria(MUTE_SEL);
	const initialDeafenLabel = await getAria(DEAFEN_SEL);
	const initialThemeDark = await cdp.eval(`document.documentElement.classList.contains('theme-dark')`);
	const initiallyInVoice = await cdp.eval(`!!document.querySelector(${JSON.stringify(VOICE_CONN_SEL)})`);
	const blockedWrites = [];
	const navigateTo = async (targetRoute) => {
		const prevBlocked = await cdp.eval('window.__specBlocked || []').catch(() => []);
		blockedWrites.push(...prevBlocked);
		const dest = targetRoute === '__BASE__' ? baseRoute : targetRoute;
		const url = /^https?:|^fluxer-app:/i.test(dest) ? dest : `${baseOrigin}${dest.startsWith('/') ? '' : '/'}${dest}`;
		await cdp.send('Page.navigate', {url});
		await waitFor(`document.readyState === 'complete' && document.querySelectorAll('[data-flx]').length > 10`, 20000);
		await sleep(1500);
		await cdp.eval(WRITE_GUARD).catch(() => {});
		await clickText('^Not now$').catch(() => false);
	};
	const ensureMicMuted = async () => {
		await waitFor(`!!document.querySelector('[data-flx="voice.voice-control-bar.voice-control-bar-inner.container"]')`, 8000);
		const lbl = await getAria(MUTE_SEL);
		if (lbl && /^Mute/i.test(lbl)) {
			await realClick(MUTE_SEL);
			await sleep(350);
		}
	};
	const restoreAudioAndVoice = async () => {
		if (!initiallyInVoice) {
			const inVoice = await cdp.eval(`!!document.querySelector(${JSON.stringify(VOICE_CONN_SEL)})`).catch(() => false);
			if (inVoice) {
				await realClick('[data-flx="voice.voice-control-bar.voice-control-bar-inner.button.disconnect"]').catch(() => false);
				if (await cdp.eval(`!!document.querySelector(${JSON.stringify(VOICE_CONN_SEL)})`).catch(() => false)) {
					await realClick('[data-flx="voice.voice-connection-status.voice-connection-status-inner.control-button"]').catch(() => false);
					await clickText('^Disconnect$').catch(() => false);
				}
				await waitFor(`!document.querySelector(${JSON.stringify(VOICE_CONN_SEL)})`, 5000);
			}
		}
		const curDeaf = await getAria(DEAFEN_SEL);
		if (initialDeafenLabel && curDeaf && /^Deafen/i.test(initialDeafenLabel) !== /^Deafen/i.test(curDeaf)) {
			await realClick(DEAFEN_SEL).catch(() => false);
			await sleep(350);
		}
		const curMic = await getAria(MUTE_SEL);
		if (initialMicLabel && curMic && /^Mute/i.test(initialMicLabel) !== /^Mute/i.test(curMic)) {
			await realClick(MUTE_SEL).catch(() => false);
			await sleep(350);
		}
	};
	const restoreThemeIfNeeded = async () => {
		if (!initialThemeDark) return;
		const isDark = await cdp.eval(`document.documentElement.classList.contains('theme-dark')`).catch(() => true);
		if (isDark) return;
		await realClick('[data-flx="app.user-area.user-area-inner.control-button.settings-click"]').catch(() => false);
		await sleep(400);
		await realClickExpr(`[...document.querySelectorAll('[data-flx="app.settings-modal-layout.settings-modal-sidebar-item.button"],[data-flx="app.settings-modal-layout.settings-modal-sidebar-item.button--2"]')].find((e) => e.innerText.trim().split('\\n')[0] === 'Themes')`).catch(() => false);
		await sleep(400);
		await realClick('[data-flx="user.appearance-tab.theme.theme-button.radio.click.button"][aria-label="Dark theme"]').catch(() => false);
		await sleep(400);
		for (let k = 0; k < 3 && (await cdp.eval(shown('user.user-settings-modal.modal-root')).catch(() => false)); k++) {
			await pressKey('Escape', 27);
			await sleep(300);
		}
	};
	mkdirSync(resolve(opts.outDir), {recursive: true});
	const report = [];
	try {
		for (const sc of scenes) {
			const rec = {slug: sc.slug, status: 'ok', error: null, elements: 0};
			try {
				for (const act of sc.official ?? []) {
					if (act.if_not_flx && (await cdp.eval(shown(act.if_not_flx)))) continue;
					if (act.click || act.context) {
						const sel = act.click ?? act.context;
						const nth = Number(act.index ?? 0);
						const label = await cdp.eval(`(() => { const e = document.querySelectorAll(${JSON.stringify(sel)})[${nth}]; return e ? [e.getAttribute('aria-label'), e.innerText, e.getAttribute('data-flx')].filter(Boolean).join(' ') : null; })()`);
						if (label === null) {
							if (act.optional) continue;
							throw new Error(`target not found: ${sel}`);
						}
						if (DENY.test(label)) throw new Error(`write-guard deny label: ${label.slice(0, 60)}`);
						const ok = await realClickExpr(`document.querySelectorAll(${JSON.stringify(sel)})[${nth}]`, act.context ? 'right' : 'left');
						if (!ok) throw new Error(`click failed: ${sel}`);
					} else if (act.hover) {
						// real CDP mouseMoved to the element centre (kept through capture); `index` picks the nth match
						const pt = await cdp.eval(`(() => { const e = document.querySelectorAll(${JSON.stringify(act.hover)})[${Number(act.index ?? 0)}];
						  if (!e) return null; e.scrollIntoView({block: 'nearest'}); const r = e.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
						if (!pt) throw new Error(`hover target not found: ${act.hover}`);
						await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: pt[0] - 1, y: pt[1] - 1});
						await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: pt[0], y: pt[1]});
					} else if (act.click_text) {
						// click the element matching `within` (or body leaf) whose text matches `click_text` (same deny list)
						if (DENY.test(act.click_text)) throw new Error(`write-guard deny label: ${act.click_text}`);
						const ok = act.within
							? await realClickExpr(`[...document.querySelectorAll(${JSON.stringify(act.within)})].find((e) => e.innerText.trim().split('\\n')[0] === ${JSON.stringify(act.click_text)})`, 'left')
							: await clickText(act.click_text);
						if (!ok && !act.optional) throw new Error(`click_text failed: ${act.click_text}`);
					} else if (act.navigate) {
						await navigateTo(act.navigate);
						continue;
					} else if (act.voice_mute) {
						await ensureMicMuted();
					} else if (act.key) {
						await pressKey(act.key, {Escape: 27, Enter: 13}[act.key] ?? 0);
					} else if (act.wait_ms) {
						await sleep(act.wait_ms);
						continue;
					} else throw new Error(`unknown action ${JSON.stringify(act)}`);
					await sleep(250);
				}
				if (sc.expect_flx && !(await waitFor(shown(sc.expect_flx), 4000))) throw new Error(`expect_flx not shown: ${sc.expect_flx}`);
				if (sc.expect_theme && !(await waitFor(`document.documentElement.classList.contains(${JSON.stringify(sc.expect_theme)})`, 4000))) {
					throw new Error(`expect_theme not active: ${sc.expect_theme}`);
				}
				await sleep(500); // let open animations settle
				await cdp.eval(`window.__dropHidden = ${sc.drop_hidden ? 'true' : 'false'}`);
				const spec = await capture([]);
				spec.scene = sc.slug;
				rec.elements = spec.elements.length;
				writeFileSync(join(resolve(opts.outDir), `${sc.slug}.spec.json`), JSON.stringify(spec, null, 1), 'utf8');
			} catch (e) {
				rec.status = 'failed';
				rec.error = String(e?.message ?? e);
			}
			// restore: optional scene-specific actions (e.g. leave a DM, disconnect voice, restore dark theme), then Escape
			for (const act of sc.official_restore ?? []) {
				try {
					if (act.click) await realClickExpr(`document.querySelectorAll(${JSON.stringify(act.click)})[${Number(act.index ?? 0)}]`, 'left');
					else if (act.click_text) {
						if (act.within) await realClickExpr(`[...document.querySelectorAll(${JSON.stringify(act.within)})].find((e) => e.innerText.trim().split('\\n')[0] === ${JSON.stringify(act.click_text)})`, 'left');
						else await clickText(act.click_text);
					} else if (act.navigate) {
						await navigateTo(act.navigate);
						continue;
					} else if (act.key) await pressKey(act.key, {Escape: 27, Enter: 13}[act.key] ?? 0);
					else if (act.wait_ms) {
						await sleep(act.wait_ms);
						continue;
					}
					await sleep(act.wait_ms ?? 700);
				} catch {}
			}
			await restoreAudioAndVoice().catch(() => {});
			await restoreThemeIfNeeded().catch(() => {});
			if ((sc.official ?? []).some((a) => a.hover)) await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: 640, y: 4}).catch(() => {});
			// restore: Escape until the state is gone, then verify the base route
			const isBaseHeader = sc.expect_flx === 'channel.channel-header.header-wrapper';
			const stillOpen = () => (sc.expect_flx && !isBaseHeader ? cdp.eval(shown(sc.expect_flx)) : false);
			for (let k = 0; k < 3 && (await stillOpen()); k++) {
				await pressKey('Escape', 27);
				await sleep(400);
			}
			if (await stillOpen()) {
				rec.status = 'failed';
				rec.error = (rec.error ? rec.error + '; ' : '') + 'state did not close';
			}
			if (sc.expect_theme && initialThemeDark && !(await cdp.eval(`document.documentElement.classList.contains('theme-dark')`))) {
				rec.status = 'failed';
				rec.error = (rec.error ? rec.error + '; ' : '') + 'dark theme did not restore';
			}
			const r = await route();
			if (r !== baseRoute) {
				rec.status = 'failed';
				rec.error = (rec.error ? rec.error + '; ' : '') + `route changed ${baseRoute} -> ${r}`;
			}
			console.log(`${rec.status.padEnd(6)} ${sc.slug} (${rec.elements} elements)${rec.error ? ' - ' + rec.error : ''}`);
			report.push(rec);
		}
	} finally {
		await restoreAudioAndVoice().catch(() => {});
		await restoreThemeIfNeeded().catch(() => {});
		if ((await route().catch(() => baseRoute)) !== baseRoute) {
			await navigateTo(baseRoute).catch(() => {});
		}
		const tailBlocked = await cdp.eval('window.__specBlocked || []').catch(() => []);
		blockedWrites.push(...tailBlocked);
		await cdp.eval('window.__parityUnguard && window.__parityUnguard()').catch(() => {});
		if (guardDoc?.identifier) {
			await cdp.send('Page.removeScriptToEvaluateOnNewDocument', {identifier: guardDoc.identifier}).catch(() => {});
		}
	}
	writeFileSync(join(resolve(opts.outDir), 'scenes.report.json'), JSON.stringify({base_route: baseRoute, blocked_writes: blockedWrites, scenes: report}, null, 1), 'utf8');
	if (blockedWrites.length) console.log(`blocked writes: ${blockedWrites.join('; ')}`);
	if (report.some((r) => r.status !== 'ok')) exitCode = 1;
}

try {
	if (opts.viewport) {
		const [w, h] = opts.viewport.split('x').map(Number);
		await cdp.send('Emulation.setDeviceMetricsOverride', {width: w, height: h, deviceScaleFactor: 1, mobile: false});
	}
	await cdp.send('DOM.enable');
	await cdp.send('CSS.enable');
	if (opts.scenes) await runScenes();
	else {
		const spec = await capture(opts.hover);
		mkdirSync(dirname(resolve(opts.out)), {recursive: true});
		writeFileSync(opts.out, JSON.stringify(spec, null, 1), 'utf8');
		console.log(`spec: ${spec.elements.length} elements -> ${opts.out}`);
	}
} catch (e) {
	console.error(String(e?.stack ?? e));
	exitCode = 1;
} finally {
	for (const id of forced) await cdp.send('CSS.forcePseudoState', {nodeId: id, forcedPseudoClasses: []}).catch(() => {});
	if (opts.viewport) await cdp.send('Emulation.clearDeviceMetricsOverride').catch(() => {});
}
process.exit(exitCode);
