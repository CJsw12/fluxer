// Official web app ground-truth capture over Chrome DevTools Protocol (no npm deps; Node >= 22).
//
// Launches a private headless Chrome profile, logs in once through the real form when the
// profile has no session, pins viewport/DPR/theme/reduced-motion, opens each route and writes:
//   <out>/<slug>.png         viewport screenshot
//   <out>/<slug>.dom.json    visible [data-flx] elements: rect, key computed styles, own text
//   <out>/<slug>.ax.json     accessibility nodes (role, name, bounds) for structural parity
//
// Secrets: credentials come from env FLUXER_PARITY_EMAIL / FLUXER_PARITY_PASSWORD only.
// Usage: node web_capture.mjs --origin https://host --out DIR --chrome PATH --profile DIR
//        --scenes scenes.json   ([{slug, path, wait_for?, settle_ms?, theme?}])
import {spawn} from 'node:child_process';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {Cdp, DOM_DUMP, SVG_DUMP, clientInfo, helpers, navigate, pageWsUrl, sleep} from './lib.mjs';

const args = Object.fromEntries(
	process.argv.slice(2).reduce((acc, v, i, a) => (v.startsWith('--') ? [...acc, [v.slice(2), a[i + 1]]] : acc), []),
);
const ORIGIN = args.origin;
const OUT = args.out;
const WIDTH = Number(args.width ?? 1280);
const HEIGHT = Number(args.height ?? 800);
const ATTACH = args.attach ? Number(args.attach) : null; // official Electron app started with --remote-debugging-port
const PORT = ATTACH ?? Number(args.port ?? 9333);
const EMAIL = process.env.FLUXER_PARITY_EMAIL;
const PASSWORD = process.env.FLUXER_PARITY_PASSWORD;
if (!ORIGIN || !OUT || !args.scenes || (!ATTACH && (!args.chrome || !args.profile))) {
	console.error('missing --origin/--out/--chrome/--profile/--scenes');
	process.exit(2);
}
const scenes = JSON.parse(readFileSync(args.scenes, 'utf8'));
mkdirSync(OUT, {recursive: true});

const chrome = ATTACH
	? null
	: spawn(
		args.chrome,
		[
			'--headless=new',
			`--remote-debugging-port=${PORT}`,
			`--user-data-dir=${args.profile}`,
			`--window-size=${WIDTH},${HEIGHT}`,
			'--force-device-scale-factor=1',
			'--hide-scrollbars',
			'--mute-audio',
			'--no-first-run',
			'--no-default-browser-check',
			'--disable-features=Translate,MediaRouter',
			'about:blank',
		],
		{stdio: 'ignore'},
	);


const AX_ROLES = new Set([
	'button', 'link', 'textbox', 'tab', 'tablist', 'navigation', 'region', 'heading', 'listitem', 'menuitem',
	'checkbox', 'switch', 'radio', 'slider', 'combobox', 'dialog', 'menu', 'treeitem', 'img', 'StaticText',
]);

async function axDump(cdp) {
	const {nodes} = await cdp.send('Accessibility.getFullAXTree');
	const out = [];
	for (const n of nodes) {
		if (n.ignored) continue;
		const role = n.role?.value;
		if (!AX_ROLES.has(role) || n.backendDOMNodeId === undefined) continue;
		const name = (n.name?.value ?? '').trim();
		if (role === 'StaticText' && !name) continue;
		try {
			const {model} = await cdp.send('DOM.getBoxModel', {backendNodeId: n.backendDOMNodeId});
			const [x1, y1, x2, , , y3] = model.border;
			if (x2 - x1 < 1 || y3 - y1 < 1 || y1 > HEIGHT || x1 > WIDTH) continue;
			out.push({role, name: name.slice(0, 200), rect: [Math.round(x1), Math.round(y1), Math.round(x2 - x1), Math.round(y3 - y1)]});
		} catch {}
	}
	return out;
}

const cdp = new Cdp(await pageWsUrl(PORT));
async function setupSession(c) {
	await c.send('Page.enable');
	await c.send('Runtime.enable');
	await c.send('DOM.enable');
	await c.send('Accessibility.enable');
	await c.send('Emulation.setDeviceMetricsOverride', {width: WIDTH, height: HEIGHT, deviceScaleFactor: 1, mobile: false});
	await c.send('Emulation.setEmulatedMedia', {features: [{name: 'prefers-reduced-motion', value: 'reduce'}]});
}
await cdp.open();
await setupSession(cdp);
cdp.onReconnect = setupSession;

// Session: the app restores accounts through its own catalog/credential mirror, so raw token
// injection is not enough. Log in once through the real form (the in-page ALTCHA widget solves
// itself) and let the private profile keep the session for later runs.
const {waitFor, realClick, realClickExpr} = helpers(cdp);
async function typeInto(selector, text) {
	await cdp.eval(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); el.focus(); el.select?.(); return true; })()`);
	await cdp.send('Input.insertText', {text});
}
async function ensureLoggedIn() {
	await navigate(cdp, PORT, `${ORIGIN}/channels/@me`, {selector: 'nav, [data-flx*="guild-list"], input[type="password"], button'});
	const settled = await waitFor(`!!document.querySelector('nav, [data-flx*="guild-list"], input[type="password"], button')`);
	if (!settled) throw new Error('app did not boot');
	await sleep(1500);
	const signedIn = await cdp.eval(`location.pathname.startsWith('/channels') && !document.querySelector('input[type="password"]')
	  && !/Sign in with email/.test(document.body.innerText)`);
	if (signedIn) return;
	if (!EMAIL || !PASSWORD) throw new Error('signed out and no FLUXER_PARITY_EMAIL/PASSWORD');
	await cdp.eval(`(() => { const b = [...document.querySelectorAll('button')].find(b => /Sign in with email/i.test(b.innerText)); b?.click(); return true; })()`);
	if (!(await waitFor(`!!document.querySelector('input[type="password"]')`))) throw new Error('login form not shown');
	await typeInto('input[type="email"], input[autocomplete="username"], input[autocomplete="email"], input[name="email"]', EMAIL);
	await typeInto('input[type="password"]', PASSWORD);
	await cdp.eval(`(() => { const f = document.querySelector('input[type="password"]').form;
	  const b = f?.querySelector('button[type="submit"]') ?? [...document.querySelectorAll('button')].find(b => /^(log ?in|sign in)$/i.test(b.innerText.trim()));
	  b ? b.click() : f?.requestSubmit(); return true; })()`);
	if (!(await waitFor(`location.pathname.startsWith('/channels') && !document.querySelector('input[type="password"]')`, 60000))) {
		throw new Error('login did not complete (captcha/2FA?)');
	}
	await sleep(2000);
}
if (!ATTACH) await ensureLoggedIn();

const BUTTON_BY_TEXT = (re, within = 'body') => `[...document.querySelector(${JSON.stringify(within)}).querySelectorAll('button,[role="button"],[role="tab"],a,[role="menuitem"]')]
  .find(e => new RegExp(${JSON.stringify(re)}, 'i').test(e.innerText.trim()) || new RegExp(${JSON.stringify(re)}, 'i').test(e.getAttribute('aria-label') ?? ''))`;

const manifest = [];
for (const scene of scenes) {
	const theme = scene.theme ?? 'dark';
	if (!ATTACH) {
		// The booted app removes window.localStorage, so write storage from a non-app document.
		await navigate(cdp, PORT, `${ORIGIN}/favicon.ico`, {selector: 'body'});
		await waitFor(`document.readyState === 'complete'`, 5000);
		await cdp.eval(`localStorage.setItem('theme', ${JSON.stringify(theme)}); true`);
	}
	await navigate(cdp, PORT, `${ORIGIN}${scene.path}`, {selector: scene.wait_for ?? '[data-flx]'});
	const waitSel = scene.wait_for ?? '[data-flx]';
	let ready = false;
	for (let i = 0; i < 80 && !ready; i++) {
		await sleep(250);
		ready = await cdp.eval(`!!document.querySelector(${JSON.stringify(waitSel)})`).catch(() => false);
	}
	// Scene actions: [{click: css}] | [{context: css}] (right click) | [{hover: css}] | [{click_text: regex, within?: css}]
	// | [{key: 'Escape'}] | [{wait_ms}]
	for (const act of scene.actions ?? []) {
		if (act.click || act.context) {
			const sel = act.click ?? act.context;
			const ok = await waitFor(`!!document.querySelector(${JSON.stringify(sel)})`, 8000);
			if (!ok || !(await realClick(sel, act.context ? 'right' : 'left'))) ready = false;
		} else if (act.hover) {
			const pt = await cdp.eval(`(() => { const e = document.querySelector(${JSON.stringify(act.hover)}); if (!e) return null;
			  const r = e.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
			if (pt) await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: pt[0], y: pt[1]});
			else ready = false;
		} else if (act.click_text) {
			if (!(await realClickExpr(BUTTON_BY_TEXT(act.click_text, act.within ?? 'body')))) ready = false;
		} else if (act.key) {
			for (const type of ['keyDown', 'keyUp']) await cdp.send('Input.dispatchKeyEvent', {type, key: act.key, code: act.key, windowsVirtualKeyCode: act.vk ?? 27});
		}
		await sleep(act.wait_ms ?? 700);
	}
	if (scene.expect) ready = ready && (await waitFor(`!!document.querySelector(${JSON.stringify(scene.expect)})`, 8000));
	await sleep(scene.settle_ms ?? 1500);
	// Web-only nag bars (mobile app / desktop download) do not exist in the desktop client.
	for (let i = 0; i < 6 && (await realClick('[data-flx="app.nagbar.dismiss-button"]')); i++) {
		await sleep(800);
		// Some nag bars confirm dismissal in a modal ("Dismiss anyway").
		if (await realClickExpr(BUTTON_BY_TEXT('^Dismiss anyway$'))) await sleep(800);
	}
	// Park the pointer on inert member-list blank space so no hover tooltip/affordance leaks in.
	if (!scene.keep_pointer) await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: WIDTH - 40, y: HEIGHT - 40});
	await sleep(300);
	const shot = await cdp.send('Page.captureScreenshot', {format: 'png'});
	writeFileSync(join(OUT, `${scene.slug}.png`), Buffer.from(shot.data, 'base64'));
	const dom = await cdp.eval(DOM_DUMP);
	writeFileSync(join(OUT, `${scene.slug}.dom.json`), JSON.stringify(dom, null, 1));
	const ax = await axDump(cdp);
	writeFileSync(join(OUT, `${scene.slug}.ax.json`), JSON.stringify(ax, null, 1));
	writeFileSync(join(OUT, `${scene.slug}.svgs.json`), JSON.stringify(await cdp.eval(SVG_DUMP)));
	manifest.push({slug: scene.slug, path: scene.path, theme, ready, dom: dom.length, ax: ax.length});
	console.log(`${scene.slug}: ready=${ready} dom=${dom.length} ax=${ax.length}`);
}
writeFileSync(join(OUT, 'manifest.json'), JSON.stringify({client: await clientInfo(cdp), width: WIDTH, height: HEIGHT, scenes: manifest}, null, 1));
cdp.ws.close();
chrome?.kill();
process.exit(manifest.every((m) => m.ready) ? 0 : 1);
