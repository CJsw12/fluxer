// Measure the official profile card in both themes without changing account settings.
import {Cdp, pageWsUrl, sleep} from './lib.mjs';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';

const cdp = new Cdp(await pageWsUrl(9334, 1), {timeoutMs: 5000});
await cdp.open();
await cdp.send('Page.enable');
await cdp.send('Emulation.setDeviceMetricsOverride', {width: 1280, height: 800, deviceScaleFactor: 1, mobile: false});
// The official Popout opens hover submenus (status menu) only while <html> has `window-focused`
// and no `window-focus-activation-guard` (WindowFocusInteractionGuard). When another window (the
// Rust client) took OS focus, the class is gone and hover never opens the menu. Emulate page
// focus and deliver the window `focus` event the app listens to, then require the precondition.
const hoverControls = () => cdp.eval(`(() => { const c = document.documentElement.classList;
	return (c.contains('window-focused') || c.contains('unfocused-fully-interactive')) && !c.contains('window-focus-activation-guard'); })()`);
const focusedBefore = await hoverControls();
const originalClass = await cdp.eval('document.documentElement.className');
await cdp.send('Emulation.setFocusEmulationEnabled', {enabled: true});
if (!focusedBefore) {
	await cdp.eval("window.dispatchEvent(new FocusEvent('focus'))");
	for (let i = 0; i < 20 && !(await hoverControls()); i++) await sleep(50);
}
if (!(await hoverControls())) {
	const cls = await cdp.eval('document.documentElement.className');
	await cdp.send('Emulation.setFocusEmulationEnabled', {enabled: false});
	throw new Error('official hover controls disabled (root class: ' + cls + ')');
}
console.error(`hover controls: ${focusedBefore ? 'already focused' : 'restored via focus emulation'}`);
const rootClass = await cdp.eval('document.documentElement.className');
const at = async (suffix, text = null) => cdp.eval(`(() => {
	const selector = '[data-flx$="' + ${JSON.stringify(suffix)} + '"]';
	const el = [...document.querySelectorAll(selector)].find(e => {
		return e.getBoundingClientRect().width && (${JSON.stringify(text)} === null || e.textContent.trim() === ${JSON.stringify(text)});
	});
	if (!el) throw new Error('Missing ' + selector);
	const r = el.getBoundingClientRect();
	return {x: r.x + r.width / 2, y: r.y + r.height / 2};
})()`);
const move = p => cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', ...p});
const click = async p => {
	await move(p);
	for (const type of ['mousePressed', 'mouseReleased']) {
		await cdp.send('Input.dispatchMouseEvent', {type, ...p, button: 'left', clickCount: 1});
	}
};
const shot = async filename => {
	const {data} = await cdp.send('Page.captureScreenshot', {format: 'png'});
	writeFileSync(join(process.argv[2], filename), Buffer.from(data, 'base64'));
};
try {
	const profile = '[data-flx="user.profile.profile-card.profile-card-layout.profile-card"]';
	const color = () => cdp.eval(`getComputedStyle(document.querySelector(${JSON.stringify(profile)})).backgroundColor`);
	await click(await at('app.user-area.user-area-inner.focus-ring-wrapper'));
	await sleep(240);
	const dark = await color();
	await cdp.eval("document.documentElement.classList.replace('theme-dark', 'theme-light')");
	const light = await color();
	await cdp.eval(`document.documentElement.className = ${JSON.stringify(rootClass)}`);
	await move(await at('app.floating.user-area-popout.action-button'));
	await sleep(250);
	await move(await at('status-menu-item.select.button', 'Idle'));
	await sleep(250);
	const expiry = await cdp.eval(`document.querySelector('[data-flx$="expiry-popup"]').getBoundingClientRect().toJSON()`);
	await shot('official_popout.png');
	await click({x: 1000, y: 210});
	await move(await at('app.guild-header.header-wrapper'));
	await sleep(120);
	await shot('official_hover.png');
	console.log(JSON.stringify({dark, light, expiry}));
} finally {
	await cdp.eval(`document.documentElement.className = ${JSON.stringify(rootClass)}`);
	// A failed run must not leave the popout open (Escape does not close it, and the
	// next run's user-area click would toggle it shut instead). Same outside click as above.
	const popoutOpen = `!!document.querySelector('[data-flx$="app.floating.user-area-popout.container"]')`;
	if (await cdp.eval(popoutOpen).catch(() => false)) {
		await click({x: 1000, y: 210}).catch(() => {});
		await sleep(200);
		if (await cdp.eval(popoutOpen).catch(() => false)) console.error('user-area popout still open after cleanup');
	}
	try {
		await cdp.send('Emulation.setFocusEmulationEnabled', {enabled: false});
		// Hand the focus state back (the app's blur handler ignores blur while document.hasFocus()).
		if (!focusedBefore) await cdp.eval(`document.documentElement.className = ${JSON.stringify(originalClass)}`);
		await cdp.send('Emulation.clearDeviceMetricsOverride');
	} finally {
		cdp.ws.close();
	}
}
