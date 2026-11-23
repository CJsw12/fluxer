// Harvests every visible <svg> of the official app (current screen) with its nearest data-flx host,
// rendered size and path data, and identifies it against the Phosphor originals
// (target/phosphor-core/assets/<weight>/*.svg). Output: JSON list on stdout.
//   node harvest_icons.mjs --attach 9334 [--phosphor target/phosphor-core/assets]
import {readdirSync, readFileSync} from 'node:fs';
import {join} from 'node:path';
import {Cdp, pageWsUrl} from './lib.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => { const i = argv.indexOf(`--${k}`); return i >= 0 ? argv[i + 1] : d; };
const root = opt('phosphor', 'target/phosphor-core/assets');
const norm = (d) => d.replace(/\s+/g, ' ').trim();
const index = new Map();
for (const w of readdirSync(root)) {
	for (const f of readdirSync(join(root, w))) {
		const s = readFileSync(join(root, w, f), 'utf8');
		const d = [...s.matchAll(/\bd="([^"]+)"/g)].map((m) => norm(m[1])).join('|');
		if (!index.has(d)) index.set(d, `${w}/${f}`);
	}
}
const cdp = new Cdp(await pageWsUrl(Number(opt('attach', 9334))));
await cdp.open();
await cdp.send('Emulation.setDeviceMetricsOverride', {width: 1280, height: 800, deviceScaleFactor: 1, mobile: false});
const svgs = await cdp.eval(`[...document.querySelectorAll('svg')].map(s => { const r = s.getBoundingClientRect();
  if (r.width < 6 || r.height < 6 || r.bottom < 0 || r.top > innerHeight) return null;
  const host = s.closest('[data-flx]');
  return {flx: (s.getAttribute('data-flx') || (host && host.getAttribute('data-flx')) || ''), rect: [r.x, r.y, r.width, r.height].map(Math.round),
    viewBox: s.getAttribute('viewBox'), d: [...s.querySelectorAll('path')].map(p => p.getAttribute('d') || '').join('|')}; }).filter(Boolean)`);
const out = svgs.map((s) => ({...s, phosphor: index.get(s.d.split('|').map(norm).join('|')) ?? null, d: s.d.slice(0, 60)}));
console.log(JSON.stringify(out, null, 1));
await cdp.send('Emulation.clearDeviceMetricsOverride');
process.exit(0);
