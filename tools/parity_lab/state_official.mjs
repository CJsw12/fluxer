// Captures hover and voice/mute/deafen states on the official desktop app (CDP) for M9.2.
//
//   node tools/parity_lab/state_official.mjs --attach 9334 --origin fluxer-app://app --out DIR [--voice]
//
// Write guard stays active across all baseline and hover checks. When --voice is passed, joins
// Dev Lab General (mic ALWAYS muted), captures voice stage, in-voice #markdown, and deafened
// states, then restores clean #markdown state (disconnected, undeafened, pointer parked) in finally.
import {mkdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {Cdp, DOM_DUMP, clientInfo, helpers, pageWsUrl, sleep} from './lib.mjs';

const rawArgs = process.argv.slice(2);
const VOICE = rawArgs.includes('--voice');
const args = {};
for (let i = 0; i < rawArgs.length; i++) {
	const v = rawArgs[i];
	if (v === '--voice') continue;
	if (v.startsWith('--')) {
		args[v.slice(2)] = rawArgs[i + 1];
		i++;
	}
}
const PORT = Number(args.attach ?? 9334);
const ORIGIN = args.origin ?? 'fluxer-app://app';
const OUT = args.out;
const GUILD = args.guild ?? '1572284491404148736';
const CHANNEL = args.channel ?? '1572284491404148743';
const VOICE_CHANNEL = args.voiceChannel ?? '1572284491404148740';
const WIDTH = Number(args.width ?? 1280);
const HEIGHT = Number(args.height ?? 800);
if (!OUT) {
	console.error('usage: --attach PORT --origin ORIGIN --out DIR [--voice]');
	process.exit(2);
}
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

const TOOLTIP_DUMP = `(() => {
  const keys = ['backgroundColor','color','fontSize','fontWeight','borderRadius','borderTopWidth','borderTopColor','paddingTop','paddingRight','paddingBottom','paddingLeft','boxShadow'];
  const out = [];
  for (const el of document.querySelectorAll('[role="tooltip"], [data-flx^="ui.tooltip."]')) {
    const r = el.getBoundingClientRect();
    if (r.width < 4 || r.height < 4 || r.bottom < 0 || r.right < 0 || r.top > innerHeight || r.left > innerWidth) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) === 0) continue;
    const text = (el.innerText ?? '').trim();
    if (!text) continue;
    const style = {}; for (const k of keys) style[k] = cs[k];
    out.push({flx: el.getAttribute('data-flx') ?? '', role: el.getAttribute('role') ?? '', text: text.slice(0, 200),
      rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)], style});
  }
  return out;
})()`;

const STYLE_KEYS = ['backgroundColor', 'color', 'borderRadius', 'borderTopWidth', 'borderTopStyle', 'borderTopColor', 'opacity', 'transform', 'boxShadow', 'cursor'];
const MUTE_SEL = '[data-flx="app.user-area.user-area-inner.control-button.mute"]';
const DEAFEN_SEL = '[data-flx="app.user-area.user-area-inner.control-button.deafen"]';

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
	await sleep(350);
}

async function dismissNag() {
	for (let i = 0; i < 6; i++) {
		const hit = await realClickExpr(`document.querySelector('[data-flx="app.nagbar.dismiss-button"]')`);
		if (!hit) break;
		await sleep(700);
		await realClickExpr(`[...document.querySelectorAll('button')].find(b => /^Dismiss anyway$/i.test(b.innerText.trim()))`);
		await sleep(700);
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

async function clickMarkdownInSidebar() {
	const clicked = await realClickExpr(`[...document.querySelectorAll('[data-flx="app.generic-channel-item.long-pressable.click"]')].find(e => (e.innerText || '').trim() === 'markdown')`);
	if (!clicked) {
		throw new Error('markdown channel item not found in sidebar for SPA navigation');
	}
	await waitFor(`!!document.querySelector('[data-flx="channel.channel-header.header-wrapper"]')`, 10000);
	await sleep(1200);
	await park();
}

async function saveState(slug, extra = {}) {
	const dom = await cdp.eval(DOM_DUMP);
	const tooltips = await cdp.eval(TOOLTIP_DUMP);
	const shot = await cdp.send('Page.captureScreenshot', {format: 'png'});
	writeFileSync(join(OUT, `${slug}.png`), Buffer.from(shot.data, 'base64'));
	writeFileSync(join(OUT, `${slug}.dom.json`), JSON.stringify(dom, null, 1));
	console.log(`${slug}: dom=${dom.length} tooltips=${tooltips.length}`);
	return {slug, dom_count: dom.length, tooltips, ...extra};
}

async function getAria(sel) {
	return cdp.eval(`document.querySelector(${JSON.stringify(sel)})?.getAttribute('aria-label') ?? null`);
}

async function ensureMicMuted() {
	const lbl = await getAria(MUTE_SEL);
	if (lbl && /^Mute/i.test(lbl)) {
		await realClick(MUTE_SEL);
		await sleep(350);
	}
	return getAria(MUTE_SEL);
}

async function ensureUndeafenedKeepMuted() {
	const dlbl = await getAria(DEAFEN_SEL);
	if (dlbl && /^Undeafen/i.test(dlbl)) {
		await realClick(DEAFEN_SEL);
		await sleep(350);
	}
	const mlbl = await ensureMicMuted();
	return {mic: mlbl, deafen: await getAria(DEAFEN_SEL)};
}

const HOVER_TARGETS = [
	{slug: 'hover_rail_home', expr: `document.querySelector('[data-flx="app.sidebar-nav.fluxer-button.fluxer-button.select"]')`},
	{slug: 'hover_rail_favorites', expr: `document.querySelector('[data-flx="app.sidebar-nav.favorites-button.fluxer-button.select"]')`},
	{slug: 'hover_rail_guild', expr: `document.querySelector('[data-flx="app.sidebar-nav.guild-list-item-presentation.guild-list-item.click"]')`},
	{slug: 'hover_rail_discover', expr: `document.querySelector('[data-flx="app.sidebar-nav.discovery-button.button.click"]')`},
	{slug: 'hover_rail_add', expr: `document.querySelector('[data-flx="app.sidebar-nav.add-guild-button.button.add-guild"]')`},
	{slug: 'hover_guild_header', expr: `document.querySelector('[data-flx="app.guild-header.header-wrapper"]')`},
	{
		slug: 'hover_channel_row',
		expr: `[...document.querySelectorAll('[data-flx="app.generic-channel-item.long-pressable.click"]')].find(e => {
		  const l = e.getAttribute('aria-label') || '';
		  return /,\\s*channel\\b/i.test(l) && !/selected/i.test(l) && !/category/i.test(l) && !/voice channel/i.test(l);
		})`,
	},
	{slug: 'hover_header_mute', expr: `document.querySelector('[data-flx="channel.channel-header-components.channel-notification-settings-button.channel-header-icon.click"]')`},
	{slug: 'hover_header_members', expr: `document.querySelector('[data-flx="channel.channel-header.channel-header-icon.toggle-members"]')`},
	{slug: 'hover_header_inbox', expr: `document.querySelector('[data-flx="channel.channel-header.inbox-button"]')`},
	{slug: 'hover_message_row', expr: `document.querySelector('[data-flx="channel.message.article.alt-click"]')`},
	{slug: 'hover_user_mute', expr: `document.querySelector('${MUTE_SEL}')`},
	{slug: 'hover_user_deafen', expr: `document.querySelector('${DEAFEN_SEL}')`},
	{slug: 'hover_user_settings', expr: `document.querySelector('[data-flx="app.user-area.user-area-inner.control-button.settings-click"]')`},
];

const results = [];
const meta = {
	client: await clientInfo(cdp),
	route: `/channels/${GUILD}/${CHANNEL}`,
	width: WIDTH,
	height: HEIGHT,
	channels: [],
	initial_audio: {},
	voice_states: {},
	blocked_writes: [],
};

let joinedVoice = false;

try {
	await gotoMarkdown();
	meta.initial_audio = {mic: await getAria(MUTE_SEL), deafen: await getAria(DEAFEN_SEL)};
	meta.channels = await cdp.eval(`(() => {
	  return [...document.querySelectorAll('[data-flx="app.generic-channel-item.long-pressable.click"]')].map((e, idx) => {
	    const r = e.getBoundingClientRect();
	    const parent = e.closest('[data-flx="app.generic-channel-item.div"]') || e.parentElement;
	    const unread = !!(parent && parent.querySelector('[data-flx*="unread"], [class*="unread"]'));
	    return {idx, label: e.getAttribute('aria-label') || '', text: (e.innerText || '').trim(),
	      rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)], unread};
	  });
	})()`);
	results.push(await saveState('rest'));

	for (const t of HOVER_TARGETS) {
		await park();
		const info = await cdp.eval(`(() => {
		  const el = ${t.expr};
		  if (!el) return null;
		  const r = el.getBoundingClientRect();
		  const flx = el.getAttribute('data-flx') || '';
		  const idx = flx ? [...document.querySelectorAll('[data-flx="' + CSS.escape(flx) + '"]')].indexOf(el) : 0;
		  const cs = getComputedStyle(el);
		  const before = {}; for (const k of ${JSON.stringify(STYLE_KEYS)}) before[k] = cs[k];
		  return {flx, idx, label: el.getAttribute('aria-label') || '', rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
		    cx: Math.round(r.left + r.width / 2), cy: Math.round(r.top + r.height / 2), before};
		})()`);
		if (!info) {
			console.log(`${t.slug}: target missing`);
			results.push({slug: t.slug, missing: true});
			continue;
		}
		await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x: info.cx, y: info.cy});
		await sleep(700);
		const after = await cdp.eval(`(() => {
		  const el = ${t.expr};
		  if (!el) return {};
		  const cs = getComputedStyle(el);
		  const out = {}; for (const k of ${JSON.stringify(STYLE_KEYS)}) out[k] = cs[k];
		  return out;
		})()`);
		const styleDiff = {};
		for (const k of STYLE_KEYS) {
			if (info.before[k] !== after[k]) styleDiff[k] = [info.before[k], after[k]];
		}
		results.push(await saveState(t.slug, {
			target: {flx: info.flx, idx: info.idx, label: info.label, rect: info.rect, center: [info.cx, info.cy]},
			style_diff: styleDiff,
		}));
	}

	if (VOICE) {
		await park();
		await cdp.send('Page.navigate', {url: `${ORIGIN}/channels/${GUILD}/${VOICE_CHANNEL}`});
		await sleep(4000);
		await clickText('^Not now$');
		await sleep(400);
		const joined = (await clickText('^Join voice channel$')) || (await clickText('^General$'));
		await sleep(4500);
		joinedVoice = true;
		const audioAfterJoin = await ensureUndeafenedKeepMuted();
		const inVoice = await cdp.eval(`[...document.querySelectorAll('button')].some(b => /^Share your screen$/i.test(b.getAttribute('aria-label') || ''))`);
		meta.voice_states.join = {joined, in_voice: inVoice, ...audioAfterJoin};
		await park();
		results.push(await saveState('voice_stage'));

		// Check if compact/split chat toggle exists in the voice channel view.
		const splitClicked =
			(await realClick('[aria-label="Show chat"]')) ||
			(await realClick('[data-flx="voice.compact-voice-call-view.compact-call-height-toggle-button.height-toggle-pill"]')) ||
			(await realClick('[data-flx="voice.compact-voice-call-view.compact-voice-call-view-inner.compact-call-height-toggle-button"] button, [data-flx="voice.compact-voice-call-view.compact-voice-call-view-inner.compact-call-height-toggle-button"]'));
		if (splitClicked) {
			await sleep(800);
			await park();
			results.push(await saveState('voice_stage_split'));
		}

		// Navigate back to #markdown via SPA click while remaining in voice.
		await clickMarkdownInSidebar();
		const inVoiceMd = await cdp.eval(`!!document.querySelector('[data-flx="app.user-area.user-area-inner.voice-connection-wrapper"]')`);
		results.push(await saveState('voice_in_markdown', {in_voice: inVoiceMd}));

		// Deafen -> capture -> Undeafen (keeping mic muted).
		const dBefore = await getAria(DEAFEN_SEL);
		if (dBefore && /^Deafen/i.test(dBefore)) {
			await realClick(DEAFEN_SEL);
			await sleep(500);
			await park();
			const inVoiceDeaf = await cdp.eval(`!!document.querySelector('[data-flx="app.user-area.user-area-inner.voice-connection-wrapper"]')`);
			results.push(await saveState('voice_deafened', {in_voice: inVoiceDeaf, deafen_label: await getAria(DEAFEN_SEL), mic_label: await getAria(MUTE_SEL)}));
			await realClick(DEAFEN_SEL);
			await sleep(400);
		}
		meta.voice_states.restored_audio = await ensureUndeafenedKeepMuted();
	}
} finally {
	if (joinedVoice) {
		await realClick('[data-flx="voice.voice-connection-status.voice-connection-status-inner.control-button"]').catch(() => false);
		await clickText('^Disconnect$').catch(() => {});
		await sleep(1000);
	}
	await gotoMarkdown().catch(() => {});
	meta.blocked_writes = await cdp.eval('window.__parityBlocked || []').catch(() => []);
	writeFileSync(join(OUT, 'manifest.json'), JSON.stringify({...meta, states: results}, null, 1));
	await cdp.send('Emulation.clearDeviceMetricsOverride').catch(() => {});
	cdp.ws.close();
}
process.exit(0);
