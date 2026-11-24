// Captures the CSS custom properties the official app currently resolves on :root (computed
// style plus declarations from :root/html/[data-theme...] rules in same-origin sheets) and
// reports coverage against the generated ThemeVariableManifest. Strictly read-only: no clicks,
// navigation, storage writes or theme switching.
//   node theme_tokens.mjs --attach 9334 --out <file.json> [--manifest PATH]
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {Cdp, clientInfo, pageWsUrl} from './lib.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const DEFAULT_MANIFEST = resolve(HERE, '../../fluxer_app/src/features/theme/variables/ThemeVariableManifest.ts');

const args = Object.fromEntries(
	process.argv.slice(2).reduce((acc, v, i, a) => (v.startsWith('--') ? [...acc, [v.slice(2), a[i + 1]]] : acc), []),
);
if (!args.attach || !args.out) {
	console.error('usage: node theme_tokens.mjs --attach <port> --out <file.json> [--manifest PATH]');
	process.exit(2);
}

// Runs in the page. Kept as a function so its regexes are not re-escaped through a template string.
function pageTokens() {
	const root = document.documentElement;
	const cs = getComputedStyle(root);
	const names = new Set();
	for (const prop of cs) if (prop.startsWith('--')) names.add(prop);
	const isRootLike = (sel) =>
		sel.split(',').some((part) => {
			const p = part.trim().toLowerCase();
			return (
				p.startsWith(':root') ||
				p.startsWith('html') ||
				p.startsWith('[data-theme') ||
				p.startsWith(':where([data-theme') ||
				p.startsWith(':is([data-theme')
			);
		});
	const addDeclared = (style) => {
		if (!style) return;
		for (const prop of style) if (prop.startsWith('--')) names.add(prop);
		for (const m of (style.cssText || '').matchAll(/(?:^|;)\s*(--[A-Za-z0-9_-]+)\s*:/g)) names.add(m[1]);
	};
	const walk = (rules) => {
		for (const rule of rules) {
			if (rule.selectorText && isRootLike(rule.selectorText)) addDeclared(rule.style);
			if (rule.cssRules) {
				try {
					walk(rule.cssRules);
				} catch {}
			}
		}
	};
	let sheets = 0;
	let unreadable = 0;
	for (const sheet of Array.from(document.styleSheets)) {
		sheets += 1;
		try {
			walk(sheet.cssRules);
		} catch {
			unreadable += 1;
		}
	}
	const tokens = {};
	for (const name of Array.from(names).sort()) tokens[name] = cs.getPropertyValue(name).trim();
	// Colour tokens are often unresolved expressions (hsl(258, calc(10% * var(--saturation-factor)), 5%)).
	// Let the engine resolve them on a transient, invisible probe element (removed immediately).
	const resolved = {};
	const probe = document.createElement('div');
	probe.style.cssText = 'position:absolute;visibility:hidden;pointer-events:none';
	document.body.appendChild(probe);
	try {
		for (const [name, value] of Object.entries(tokens)) {
			if (!value || !CSS.supports('color', value)) continue;
			probe.style.color = `var(${name})`;
			resolved[name] = getComputedStyle(probe).color;
		}
	} finally {
		probe.remove();
	}
	return {
		resolved,
		attrs: Object.fromEntries(Array.from(root.attributes, (a) => [a.name, a.value])),
		tokens,
		sheets,
		unreadable,
	};
}

const manifestSrc = readFileSync(args.manifest ? resolve(args.manifest) : DEFAULT_MANIFEST, 'utf8');
const manifest = new Set();
for (const m of manifestSrc.matchAll(/\{name: "(--[^"]+)"/g)) manifest.add(m[1]);
if (manifest.size === 0) {
	for (const m of manifestSrc.matchAll(/^\s*\| "(--[^"]+)"/gm)) manifest.add(m[1]);
}
if (manifest.size === 0) throw new Error('no --* names parsed from ThemeVariableManifest.ts');

const cdp = new Cdp(await pageWsUrl(Number(args.attach)));
await cdp.open();
const [client, dump] = await Promise.all([clientInfo(cdp), cdp.eval(`(${pageTokens.toString()})()`)]);
if (dump.unreadable > 0) console.error(`warning: ${dump.unreadable}/${dump.sheets} stylesheets unreadable`);

const tokens = {};
for (const name of Object.keys(dump.tokens).sort()) tokens[name] = dump.tokens[name];
const manifestMissing = [...manifest].filter((n) => !tokens[n]).sort();
const extra = Object.keys(tokens).filter((n) => tokens[n] && !manifest.has(n)).sort();
const payload = {
	client: client ?? null,
	theme_attrs: Object.fromEntries(Object.entries(dump.attrs).sort(([a], [b]) => a.localeCompare(b))),
	captured_at: new Date().toISOString(),
	count: Object.keys(tokens).length,
	tokens,
	resolved: Object.fromEntries(Object.entries(dump.resolved).sort(([a], [b]) => a.localeCompare(b))),
	manifest_missing: manifestMissing,
	extra,
};
const out = resolve(args.out);
mkdirSync(dirname(out), {recursive: true});
writeFileSync(out, `${JSON.stringify(payload, null, 1)}\n`, 'utf8');
console.log(`tokens=${payload.count} manifest=${manifest.size} missing=${manifestMissing.length} extra=${extra.length}`);
cdp.ws.close();
process.exit(0);
