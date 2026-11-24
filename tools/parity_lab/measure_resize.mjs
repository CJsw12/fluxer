// Live CDP measurement of all M9.3 resize surfaces on the official desktop app (1280x800).
//
//   node tools/parity_lab/measure_resize.mjs --attach 9334 --origin fluxer-app://app --out DIR
//
// Measures sidebar handle, compact voice call split resize, expression/emoji picker, pins popout,
// inbox popout, and threads panel (if available). Restores defaults and clears overrides in finally.
import {mkdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {Cdp, clientInfo, helpers, pageWsUrl, sleep} from './lib.mjs';

const rawArgs = process.argv.slice(2);
const args = {};
for (let i = 0; i < rawArgs.length; i++) {
	const v = rawArgs[i];
	if (v.startsWith('--')) {
		args[v.slice(2)] = rawArgs[i + 1];
		i++;
	}
}
const PORT = Number(args.attach ?? 9334);
const ORIGIN = args.origin ?? 'fluxer-app://app';
const OUT = args.out ?? 'Logs/parity/m93_official';
const GUILD = args.guild ?? '1572284491404148736';
const CHANNEL = args.channel ?? '1572284491404148743';
const VOICE_CHANNEL = args.voiceChannel ?? '1572284491404148740';
const WIDTH = 1280;
const HEIGHT = 800;
mkdirSync(OUT, {recursive: true});

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

const cdp = new Cdp(await pageWsUrl(PORT));
await cdp.open();
await cdp.send('Page.enable');
await cdp.send('Runtime.enable');
await cdp.send('Emulation.setDeviceMetricsOverride', {width: WIDTH, height: HEIGHT, deviceScaleFactor: 1, mobile: false});
await cdp.send('Emulation.setEmulatedMedia', {features: [{name: 'prefers-reduced-motion', value: 'reduce'}]});
await cdp.send('Page.addScriptToEvaluateOnNewDocument', {source: WRITE_GUARD});
await cdp.eval(WRITE_GUARD).catch(() => {});
const {waitFor, realClick, realClickExpr, pressKey, clickText} = helpers(cdp);

async function park() {
	await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: WIDTH - 40, y: HEIGHT - 40});
	await sleep(250);
}

async function dismissNag() {
	for (let i = 0; i < 6; i++) {
		const hit = await realClickExpr(`document.querySelector('[data-flx="app.nagbar.dismiss-button"]')`);
		if (!hit) break;
		await sleep(600);
		await realClickExpr(`[...document.querySelectorAll('button')].find(b => /^Dismiss anyway$/i.test(b.innerText.trim()))`);
		await sleep(600);
	}
}

async function gotoMarkdown() {
	await cdp.send('Page.navigate', {url: `${ORIGIN}/channels/${GUILD}/${CHANNEL}`});
	await waitFor(`!!document.querySelector('[data-flx="channel.channel-header.header-wrapper"]')`, 30000);
	await sleep(2000);
	await clickText('^Not now$');
	await dismissNag();
	for (let i = 0; i < 2; i++) {
		await pressKey('Escape', 27);
		await sleep(150);
	}
	await park();
}

async function rectOfExpr(jsExpr) {
	return cdp.eval(`(() => {
	  const el = ${jsExpr};
	  if (!el) return null;
	  const r = el.getBoundingClientRect();
	  const cs = getComputedStyle(el);
	  return {
	    flx: el.getAttribute('data-flx'),
	    rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
	    cursor: cs.cursor,
	    opacity: cs.opacity,
	    bg: cs.backgroundColor,
	    radius: cs.borderRadius,
	    transform: cs.transform,
	    aria: {
	      role: el.getAttribute('role'),
	      label: el.getAttribute('aria-label'),
	      orientation: el.getAttribute('aria-orientation'),
	      valuemin: el.getAttribute('aria-valuemin'),
	      valuemax: el.getAttribute('aria-valuemax'),
	      valuenow: el.getAttribute('aria-valuenow'),
	      keyshortcuts: el.getAttribute('aria-keyshortcuts'),
	    },
	  };
	})()`);
}

async function rectOf(sel) {
	return rectOfExpr(`document.querySelector(${JSON.stringify(sel)})`);
}

async function dragStep(x1, y1, x2, y2, steps = 10, holdRelease = true) {
	await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: x1, y: y1});
	await sleep(80);
	await cdp.send('Input.dispatchMouseEvent', {type: 'mousePressed', x: x1, y: y1, button: 'left', clickCount: 1});
	await sleep(60);
	for (let i = 1; i <= steps; i++) {
		const x = x1 + ((x2 - x1) * i) / steps;
		const y = y1 + ((y2 - y1) * i) / steps;
		await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x, y, button: 'left', buttons: 1});
		await sleep(25);
	}
	await sleep(80);
	if (holdRelease) {
		await cdp.send('Input.dispatchMouseEvent', {type: 'mouseReleased', x: x2, y: y2, button: 'left', clickCount: 1});
		await sleep(250);
	}
}

async function dblClick(x, y) {
	await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x, y});
	await sleep(50);
	for (const count of [1, 2]) {
		await cdp.send('Input.dispatchMouseEvent', {type: 'mousePressed', x, y, button: 'left', clickCount: count});
		await cdp.send('Input.dispatchMouseEvent', {type: 'mouseReleased', x, y, button: 'left', clickCount: count});
	}
	await sleep(250);
}

async function sendKey(key, vk, modifiers = 0) {
	for (const type of ['keyDown', 'keyUp']) {
		await cdp.send('Input.dispatchKeyEvent', {type, key, code: key, windowsVirtualKeyCode: vk, modifiers});
	}
	await sleep(150);
}

const spec = {client: await clientInfo(cdp), width: WIDTH, height: HEIGHT};
let joinedVoice = false;

try {
	await gotoMarkdown();

	// 1. Sidebar width resize
	const snapSidebar = async () => ({
		handle: await rectOf('[data-flx="app.sidebar-resize-handle"]'),
		indicator: await rectOf('[data-flx="app.sidebar-resize-handle.indicator"]'),
		navbar: await rectOf('[data-flx="app.guild-sidebar.guild-navbar"]'),
		user_area: await rectOf('[data-flx="app.user-area.user-area-inner.section"]'),
		outline_frame: await rectOf('[data-flx="app.outline-frame.frame"]'),
		channel_header: await rectOf('[data-flx="channel.channel-header.header-wrapper"]'),
	});

	const sbDefault = await snapSidebar();
	const [hx, hy, hw, hh] = sbDefault.handle.rect;
	const hcx = hx + hw / 2;
	const hcy = hy + hh / 2;
	await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: hcx, y: hcy});
	await sleep(300);
	const sbHoverIndicator = await rectOf('[data-flx="app.sidebar-resize-handle.indicator"]');

	// Drag while keeping button down briefly to inspect active indicator style
	await dragStep(hcx, hcy, hcx + 80, hcy, 6, false);
	const sbActiveIndicator = await rectOf('[data-flx="app.sidebar-resize-handle.indicator"]');
	await cdp.send('Input.dispatchMouseEvent', {type: 'mouseReleased', x: hcx + 80, y: hcy, button: 'left', clickCount: 1});
	await sleep(250);

	// Drag to min (x=150) and max (x=650)
	let cur = await rectOf('[data-flx="app.sidebar-resize-handle"]');
	await dragStep(cur.rect[0] + cur.rect[2] / 2, cur.rect[1] + cur.rect[3] / 2, 150, hcy, 10, true);
	const sbMin = await snapSidebar();

	cur = await rectOf('[data-flx="app.sidebar-resize-handle"]');
	await dragStep(cur.rect[0] + cur.rect[2] / 2, cur.rect[1] + cur.rect[3] / 2, 650, hcy, 12, true);
	const sbMax = await snapSidebar();

	// Double-click reset
	cur = await rectOf('[data-flx="app.sidebar-resize-handle"]');
	await dblClick(cur.rect[0] + cur.rect[2] / 2, cur.rect[1] + cur.rect[3] / 2);
	const sbAfterDbl = await snapSidebar();

	// Focus handle & test keyboard steps
	await cdp.eval(`document.querySelector('[data-flx="app.sidebar-resize-handle"]')?.focus()`);
	await sleep(100);
	const sbKey0 = (await snapSidebar()).navbar.rect[2];
	await sendKey('ArrowRight', 39, 0);
	const sbKeyArrow = (await snapSidebar()).navbar.rect[2];
	await sendKey('ArrowRight', 39, 8); // Shift = 8
	const sbKeyShift = (await snapSidebar()).navbar.rect[2];
	await sendKey('Home', 36, 0);
	const sbKeyHome = (await snapSidebar()).navbar.rect[2];
	await sendKey('End', 35, 0);
	const sbKeyEnd = (await snapSidebar()).navbar.rect[2];
	await sendKey('Enter', 13, 0);
	const sbKeyReset = (await snapSidebar()).navbar.rect[2];
	await park();

	spec.sidebar = {
		default: sbDefault,
		hover_indicator: sbHoverIndicator,
		active_indicator: sbActiveIndicator,
		min: sbMin,
		max: sbMax,
		after_double_click: sbAfterDbl,
		keyboard: {
			initial: sbKey0,
			after_arrow_right: sbKeyArrow,
			after_shift_arrow_right: sbKeyShift,
			after_home: sbKeyHome,
			after_end: sbKeyEnd,
			after_enter_reset: sbKeyReset,
		},
	};

	// 2. Resizable popouts (Expression/Emoji Picker, Pins, Inbox) + Threads
	async function measureResizablePopout(name, triggerSel, expectedContainerExpr, measureKeys = false) {
		await park();
		const opened = await realClick(triggerSel);
		await sleep(800);
		const readContainer = () => rectOfExpr(expectedContainerExpr);
		const container = await readContainer();
		const handles = await cdp.eval(`(() => {
		  return [...document.querySelectorAll('[data-flx="ui.resizable-pane.resizable-pane-handles.handle.button"]')].map(el => {
		    const r = el.getBoundingClientRect();
		    const cs = getComputedStyle(el);
		    return {
		      edge: el.getAttribute('data-resizable-pane-edge'),
		      label: el.getAttribute('aria-label'),
		      cursor: cs.cursor,
		      rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
		    };
		  });
		})()`);
		let dragMin = null;
		let dragMax = null;
		let widthSteps = [];
		let keyboard = null;
		let afterDbl = null;
		if (container && handles.length) {
			const topH = handles.find((h) => h.edge === 'top') || handles.find((h) => h.edge === 'bottom');
			const leftH = handles.find((h) => h.edge === 'left') || handles.find((h) => h.edge === 'right');
			if (leftH) {
				for (const dx of [-40, -80, -140, -240, 200, 400]) {
					const nowH = await cdp.eval(`(() => {
					  const el = document.querySelector('[data-flx="ui.resizable-pane.resizable-pane-handles.handle.button"][data-resizable-pane-edge="${leftH.edge}"]');
					  if (!el) return null; const r = el.getBoundingClientRect(); return [r.left + r.width/2, r.top + r.height/2];
					})()`);
					if (!nowH) break;
					await dragStep(nowH[0], nowH[1], nowH[0] + dx, nowH[1], 8, true);
					const cNow = await readContainer();
					if (cNow) widthSteps.push({dx, rect: cNow.rect});
				}
				dragMax = await readContainer();
			}
			if (topH) {
				const dir = topH.edge === 'top' ? 1 : -1;
				const curH = await cdp.eval(`(() => {
				  const el = document.querySelector('[data-flx="ui.resizable-pane.resizable-pane-handles.handle.button"][data-resizable-pane-edge="${topH.edge}"]');
				  if (!el) return null; const r = el.getBoundingClientRect(); return [r.left + r.width/2, r.top + r.height/2];
				})()`);
				if (curH) {
					await dragStep(curH[0], curH[1], curH[0], curH[1] + dir * 500, 8, true);
					dragMin = await readContainer();
				}
			}
			const anyH = await cdp.eval(`(() => {
			  const el = document.querySelector('[data-flx="ui.resizable-pane.resizable-pane-handles.handle.button"]');
			  if (!el) return null; const r = el.getBoundingClientRect(); return [r.left + r.width/2, r.top + r.height/2];
			})()`);
			if (anyH) {
				await dblClick(anyH[0], anyH[1]);
				afterDbl = await readContainer();
			}
			if (measureKeys && leftH) {
				await cdp.eval(`document.querySelector('[data-flx="ui.resizable-pane.resizable-pane-handles.handle.button"][data-resizable-pane-edge="${leftH.edge}"]')?.focus()`);
				await sleep(100);
				const k0 = (await readContainer())?.rect;
				await sendKey('ArrowLeft', 37, 0);
				const kArrow = (await readContainer())?.rect;
				await sendKey('ArrowLeft', 37, 8);
				const kShift = (await readContainer())?.rect;
				await sendKey('Enter', 13, 0);
				const kReset = (await readContainer())?.rect;
				keyboard = {initial: k0, after_arrow_left: kArrow, after_shift_arrow_left: kShift, after_enter_reset: kReset};
			}
		}
		const shot = await cdp.send('Page.captureScreenshot', {format: 'png'});
		writeFileSync(join(OUT, `${name}.png`), Buffer.from(shot.data, 'base64'));
		await pressKey('Escape', 27);
		await sleep(350);
		return {opened, default_container: container, handles, width_steps: widthSteps, drag_min: dragMin, drag_max: dragMax, after_double_click: afterDbl, keyboard};
	}

	spec.expression_picker = await measureResizablePopout(
		'expression_picker',
		'[data-flx="channel.textarea.textarea-buttons.textarea-button.expression-picker-toggle--4"]',
		`document.querySelector('[data-flx="ui.resizable-pane.resizable-pane-handles.handle.button"]')?.parentElement`,
		true,
	);

	spec.pins_popout = await measureResizablePopout(
		'pins_popout',
		'[data-flx="channel.channel-header-components.channel-pins-button.channel-header-icon"]',
		`document.querySelector('[data-flx="channel.channel-pins-popout.container"]')`,
	);
	spec.inbox_popout = await measureResizablePopout(
		'inbox_popout',
		'[data-flx="channel.channel-header.inbox-button"]',
		`document.querySelector('[data-flx="messaging.inbox-popout.container"]')`,
	);

	// Threads button check
	await park();
	const thrOpened = await realClick('[data-flx="threads.threads-button.channel-header-icon"]');
	await sleep(700);
	spec.threads_trigger = {
		opened: thrOpened,
		side_panel: await rectOf('[data-flx="threads.thread-side-panel.panel"]'),
		side_handle: await rectOf('[data-flx="threads.thread-side-panel.resize-handle"]'),
		popout_count: await cdp.eval(`document.querySelectorAll('[data-flx*="thread"]').length`),
		visible_thread_flxs: await cdp.eval(`[...document.querySelectorAll('[data-flx*="thread"]')].map(e => ({flx: e.getAttribute('data-flx'), rect: (r=>[r.x,r.y,r.width,r.height].map(Math.round))(e.getBoundingClientRect())}))`),
	};
	await pressKey('Escape', 27);
	await sleep(300);

	// 3. Compact voice call <-> chat height resize
	await cdp.send('Page.navigate', {url: `${ORIGIN}/channels/${GUILD}/${VOICE_CHANNEL}`});
	await sleep(4000);
	await clickText('^Not now$');
	await sleep(400);
	const vJoined = (await clickText('^Join voice channel$')) || (await clickText('^General$'));
	await sleep(4500);
	joinedVoice = true;
	const muteLbl = await cdp.eval(`document.querySelector('[data-flx="app.user-area.user-area-inner.control-button.mute"]')?.getAttribute('aria-label')`);
	if (muteLbl && /^Mute/i.test(muteLbl)) await realClick('[data-flx="app.user-area.user-area-inner.control-button.mute"]');
	await sleep(300);

	const findCallResize = () => rectOf('[data-flx="voice.compact-voice-call-view.compact-voice-call-view-inner.resize-handle.resize-key-down"]');
	let callHandle = await findCallResize();
	const heightToggle = await rectOf('[data-flx="voice.compact-voice-call-view.compact-call-height-toggle-button.height-toggle-pill"]');
	const chatBtn = await cdp.eval(`[...document.querySelectorAll('button')].map(b => ({label: b.getAttribute('aria-label'), text: (b.innerText||'').trim(), rect: (r=>[r.x,r.y,r.width,r.height].map(Math.round))(b.getBoundingClientRect())})).filter(b => /chat|resize|height|collapse|split/i.test((b.label||'') + ' ' + b.text))`);
	if (!callHandle) {
		(await realClick('[aria-label="Show chat"]')) ||
			(await realClick('[data-flx="voice.compact-voice-call-view.compact-call-height-toggle-button.height-toggle-pill"]'));
		await sleep(800);
		callHandle = await findCallResize();
	}
	const callSectionSel = '[data-flx="voice.compact-voice-call-view.compact-voice-call-view-inner.section.voice-root-pointer-activity"]';
	const callDefault = await rectOf(callSectionSel);
	let callMin = null;
	let callMax = null;
	let callArrow = null;
	if (callHandle) {
		const [cx, cy, cw, ch] = callHandle.rect;
		await dragStep(cx + cw / 2, cy + ch / 2, cx + cw / 2, 100, 10, true);
		callMin = await rectOf(callSectionSel);
		let nowH = await findCallResize();
		if (nowH) {
			await dragStep(nowH.rect[0] + nowH.rect[2] / 2, nowH.rect[1] + nowH.rect[3] / 2, nowH.rect[0] + nowH.rect[2] / 2, 760, 10, true);
			callMax = await rectOf(callSectionSel);
		}
		await cdp.eval(`document.querySelector('[data-flx="voice.compact-voice-call-view.compact-voice-call-view-inner.resize-handle.resize-key-down"]')?.focus()`);
		await sleep(100);
		const beforeK = (await rectOf(callSectionSel))?.rect[3];
		await sendKey('ArrowUp', 38, 0);
		const afterUp = (await rectOf(callSectionSel))?.rect[3];
		await sendKey('ArrowUp', 38, 8);
		const afterShiftUp = (await rectOf(callSectionSel))?.rect[3];
		await sendKey('ArrowDown', 40, 0);
		const afterDown = (await rectOf(callSectionSel))?.rect[3];
		callArrow = {before: beforeK, after_arrow_up: afterUp, after_shift_arrow_up: afterShiftUp, after_arrow_down: afterDown};
		// Restore near initial height if changed
		if (callDefault && afterDown && afterDown !== callDefault.rect[3]) {
			nowH = await findCallResize();
			if (nowH) await dragStep(nowH.rect[0] + nowH.rect[2] / 2, nowH.rect[1] + nowH.rect[3] / 2, nowH.rect[0] + nowH.rect[2] / 2, callDefault.rect[1] + callDefault.rect[3], 8, true);
		}
	}
	spec.voice_compact = {
		joined: vJoined,
		height_toggle: heightToggle,
		matching_buttons: chatBtn,
		handle: callHandle,
		pill: await rectOf('[data-flx="voice.compact-voice-call-view.compact-voice-call-view-inner.resize-pill"]'),
		default_section: callDefault,
		drag_min: callMin,
		drag_max: callMax,
		keyboard: callArrow,
	};
} finally {
	if (joinedVoice) {
		await realClick('[data-flx="voice.voice-connection-status.voice-connection-status-inner.control-button"]').catch(() => false);
		await clickText('^Disconnect$').catch(() => {});
		await sleep(1000);
	}
	await gotoMarkdown().catch(() => {});
	spec.blocked_writes = await cdp.eval('window.__parityBlocked || []').catch(() => []);
	writeFileSync(join(OUT, 'resize_spec.json'), JSON.stringify(spec, null, 1));
	await cdp.send('Emulation.clearDeviceMetricsOverride').catch(() => {});
	cdp.ws.close();
}
console.log('wrote', join(OUT, 'resize_spec.json'));
process.exit(0);
