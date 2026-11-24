// Extracts a per-element machine-readable spec of the official app's current screen: geometry,
// resolved style, paint order, floating state, matched CSS-module rules (raw values, var() kept)
// and icon path signature for every [data-flx] element. Consumed by spec_delta.py.
// Strictly read-only: no clicks, navigation or storage writes. :hover forced via
// CSS.forcePseudoState is always reverted (also on error).
//   node spec_official.mjs --attach 9334 --out <spec.json> [--hover <flx>]... [--viewport 1280x800]
import {mkdirSync, readdirSync, statSync, writeFileSync} from 'node:fs';
import {dirname, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {Cdp, clientInfo, pageWsUrl} from './lib.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '../..');
const argv = process.argv.slice(2);
const opts = {attach: null, out: null, viewport: null, hover: []};
for (let i = 0; i < argv.length; i++) {
	const a = argv[i];
	if (a === '--attach') opts.attach = Number(argv[++i]);
	else if (a === '--out') opts.out = argv[++i];
	else if (a === '--viewport') opts.viewport = argv[++i];
	else if (a === '--hover') opts.hover.push(argv[++i]);
}
if (!opts.attach || !opts.out) {
	console.error('usage: node spec_official.mjs --attach <port> --out <spec.json> [--hover <flx>]... [--viewport 1280x800]');
	process.exit(2);
}

const PROPS = [
	'display', 'position', 'zIndex', 'flexDirection', 'alignItems', 'justifyContent', 'rowGap', 'columnGap',
	'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
	'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth',
	'borderTopColor', 'borderRightColor', 'borderBottomColor', 'borderLeftColor',
	'borderTopLeftRadius', 'borderTopRightRadius', 'borderBottomRightRadius', 'borderBottomLeftRadius',
	'backgroundColor', 'color', 'fontSize', 'fontWeight', 'lineHeight', 'letterSpacing', 'opacity',
	'boxShadow', 'overflowX', 'overflowY', 'transform',
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
		const rendered = el.getClientRects().length > 0 && cs.display !== 'none';
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
try {
	if (opts.viewport) {
		const [w, h] = opts.viewport.split('x').map(Number);
		await cdp.send('Emulation.setDeviceMetricsOverride', {width: w, height: h, deviceScaleFactor: 1, mobile: false});
	}
	await cdp.send('DOM.enable');
	await cdp.send('CSS.enable');
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

	const modules = indexModules();
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
	for (const flx of opts.hover) {
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

	const spec = {
		version: 1,
		client_info: await clientInfo(cdp),
		theme_class: page.theme,
		viewport: page.viewport,
		hover_requests: opts.hover,
		hover_errors: Object.fromEntries(Object.entries(hovers).filter(([, h]) => h.error).map(([k, h]) => [k, h.error])),
		elements,
	};
	mkdirSync(dirname(resolve(opts.out)), {recursive: true});
	writeFileSync(opts.out, JSON.stringify(spec, null, 1), 'utf8');
	console.log(`spec: ${elements.length} elements -> ${opts.out}`);
} catch (e) {
	console.error(String(e?.stack ?? e));
	exitCode = 1;
} finally {
	for (const id of forced) await cdp.send('CSS.forcePseudoState', {nodeId: id, forcedPseudoClasses: []}).catch(() => {});
	if (opts.viewport) await cdp.send('Emulation.clearDeviceMetricsOverride').catch(() => {});
}
process.exit(exitCode);
