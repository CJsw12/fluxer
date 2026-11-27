// Official reference captures for check_surface_parity.py pixel checks (no account setting changes).
//   node tools/parity_lab/surface_reference.mjs <out-dir> [--evidence]
// --evidence also writes official_hover_rail_home.png / official_hover_composer_media.png.
// Writes official_rest_{dark,light}.png and official_settings_{dark,light}.png (1280x800, theme by
// swapping the <html> theme class only, like user_area_reference.mjs) and prints
// {"modal": [x, y, w, h], "theme_class": "..."} on stdout. Restores the class and closes the modal.
import {Cdp, pageWsUrl, sleep} from './lib.mjs';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';

const outDir = process.argv[2];
if (!outDir) {
	console.error('usage: node surface_reference.mjs <out-dir>');
	process.exit(2);
}
const MODAL = '[data-flx="user.user-settings-modal.modal-root"]';
const SETTINGS_BTN = '[data-flx="app.user-area.user-area-inner.control-button.settings-click"]';
const cdp = new Cdp(await pageWsUrl(9334, 1), {timeoutMs: 8000});
await cdp.open();
await cdp.send('Page.enable');
await cdp.send('Emulation.setDeviceMetricsOverride', {width: 1280, height: 800, deviceScaleFactor: 1, mobile: false});
const rootClass = await cdp.eval('document.documentElement.className');
const move = (x, y) => cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x, y});
const escape = async () => {
	for (const type of ['keyDown', 'keyUp']) {
		await cdp.send('Input.dispatchKeyEvent', {type, key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27});
	}
};
const modalOpen = () => cdp.eval(`!!document.querySelector(${JSON.stringify(MODAL)})`);
const setTheme = async (theme) => {
	await cdp.eval(`(() => { const c = document.documentElement.classList; c.remove('theme-dark', 'theme-light'); c.add('theme-${theme}'); })()`);
	await sleep(250);
};
const shot = async (name) => {
	const {data} = await cdp.send('Page.captureScreenshot', {format: 'png'});
	writeFileSync(join(outDir, name), Buffer.from(data, 'base64'));
};
let modal = null;
try {
	if (process.argv.includes('--evidence')) {
		// hover screenshots for before/after evidence (real mouseMoved, nothing clicked)
		for (const [name, flx] of [
			['official_hover_rail_home.png', 'app.sidebar-nav.fluxer-button.fluxer-button-icon'],
			['official_hover_composer_media.png', 'channel.textarea.textarea-buttons.textarea-button.expression-picker-toggle--2'],
		]) {
			const c = await cdp.eval(`(() => { const r = document.querySelector('[data-flx="${flx}"]').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
			await move(c[0] - 1, c[1] - 1);
			await move(c[0], c[1]);
			await sleep(900);
			await shot(name);
			await move(640, 4);
			await sleep(300);
		}
	}
	await move(640, 4);
	await sleep(250);
	for (const theme of ['dark', 'light']) {
		await setTheme(theme);
		await shot(`official_rest_${theme}.png`);
	}
	await setTheme('dark');
	const pt = await cdp.eval(`(() => { const r = document.querySelector(${JSON.stringify(SETTINGS_BTN)}).getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
	await move(pt[0], pt[1]);
	for (const type of ['mousePressed', 'mouseReleased']) {
		await cdp.send('Input.dispatchMouseEvent', {type, x: pt[0], y: pt[1], button: 'left', clickCount: 1});
	}
	await move(640, 4);
	for (let i = 0; i < 20 && !(await modalOpen()); i++) await sleep(100);
	await sleep(700); // open animation
	modal = await cdp.eval(`(() => { const r = document.querySelector(${JSON.stringify(MODAL)}).getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; })()`);
	for (const theme of ['dark', 'light']) {
		await setTheme(theme);
		await shot(`official_settings_${theme}.png`);
	}
	console.log(JSON.stringify({modal, theme_class: rootClass}));
} finally {
	await cdp.eval(`document.documentElement.className = ${JSON.stringify(rootClass)}`).catch(() => {});
	for (let k = 0; k < 3 && (await modalOpen().catch(() => false)); k++) {
		await escape().catch(() => {});
		await sleep(400);
	}
	if (await modalOpen().catch(() => false)) console.error('settings modal still open after cleanup');
	try {
		await cdp.send('Emulation.clearDeviceMetricsOverride');
	} finally {
		cdp.ws.close();
	}
}
