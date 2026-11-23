// Drives the official desktop app (CDP) for voice / screen-share functional checks.
//
//   node voice_official.mjs --attach 9334 join   --guild G --channel C   (joins voice, mic muted)
//   node voice_official.mjs --attach 9334 watch                          (clicks "Watch stream", reports video)
//   node voice_official.mjs --attach 9334 share  --source "<regex>"      (in-app picker → shares matching window)
//   node voice_official.mjs --attach 9334 stop                           (stops own share)
//   node voice_official.mjs --attach 9334 leave
// Every command prints one JSON line. Pick a harmless share source (e.g. the Rust client window):
// the picker lists every open window of the machine.
import {Cdp, helpers, pageWsUrl, sleep} from './lib.mjs';

const argv = process.argv.slice(2);
const opt = (k) => {
	const i = argv.indexOf(`--${k}`);
	return i >= 0 ? argv[i + 1] : undefined;
};
const cmd = argv.find((a, i) => !a.startsWith('--') && !(i > 0 && argv[i - 1].startsWith('--')));
const cdp = new Cdp(await pageWsUrl(Number(opt('attach') ?? 9334)));
await cdp.open();
const h = helpers(cdp);
await cdp.send('Emulation.setDeviceMetricsOverride', {width: 1280, height: 800, deviceScaleFactor: 1, mobile: false});
const MUTE = '[data-flx="app.user-area.user-area-inner.control-button.mute"]';
const out = {};

async function videoReport() {
	return cdp.eval(`(async () => {
	  const res = [];
	  for (const v of document.querySelectorAll('video')) {
	    const q0 = v.getVideoPlaybackQuality(); await new Promise(r => setTimeout(r, 2000)); const q1 = v.getVideoPlaybackQuality();
	    res.push({width: v.videoWidth, height: v.videoHeight, ready: v.readyState, frames_2s: q1.totalVideoFrames - q0.totalVideoFrames, dropped: q1.droppedVideoFrames});
	  }
	  return res;
	})()`);
}

if (cmd === 'join') {
	await cdp.send('Page.navigate', {url: `fluxer-app://app/channels/${opt('guild')}/${opt('channel')}`});
	await sleep(5000);
	// "Restarted while connected" nag bar would otherwise reconnect on its own terms.
	await h.clickText('^Not now$');
	await sleep(500);
	// Opening a voice channel route shows a landing page with an explicit join button.
	out.joined = (await h.clickText('^Join voice channel$')) || (await h.clickText(`^${opt('name') ?? 'General'}$`));
	await sleep(5000);
	const label = await cdp.eval(`document.querySelector('${MUTE}')?.getAttribute('aria-label')`);
	if (label && /^Mute/i.test(label)) await h.realClick(MUTE);
	await sleep(400);
	out.mic = await cdp.eval(`document.querySelector('${MUTE}')?.getAttribute('aria-label')`);
	out.in_voice = await cdp.eval(`[...document.querySelectorAll('button')].some(b => /^Share your screen$/i.test(b.getAttribute('aria-label') || ''))`);
} else if (cmd === 'watch') {
	out.clicked = await h.clickText('^Watch stream$');
	await sleep(8000);
	out.videos = await videoReport();
	out.live_badges = await cdp.eval(`[...document.querySelectorAll('body *')].filter(e => e.children.length === 0 && /^LIVE$/.test((e.innerText||'').trim())).length`);
} else if (cmd === 'share') {
	out.opened = await h.clickText('^Share your screen$');
	await sleep(2500);
	const re = opt('source') ?? 'Fluxer - #';
	out.picked = await h.realClickExpr(`[...document.querySelectorAll('[role="dialog"] button')].find(b => new RegExp(${JSON.stringify(re)}).test(b.innerText))`);
	await sleep(3000);
	out.picker_closed = !(await cdp.eval(`!!document.querySelector('[role="dialog"]')`));
} else if (cmd === 'stop') {
	out.clicked = await h.clickText('^(End screen share|Stop (sharing|streaming))$');
} else if (cmd === 'leave') {
	out.clicked = await h.clickText('^Disconnect$');
}
console.log(JSON.stringify({cmd, ...out}));
await cdp.send('Emulation.clearDeviceMetricsOverride');
process.exit(0);
