// Shared CDP plumbing for the parity lab (no npm deps; Node >= 22 global WebSocket/fetch).
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function pageWsUrl(port, tries = 60) {
	for (let i = 0; i < tries; i++) {
		try {
			const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
			const page = list.find((t) => t.type === 'page' && !t.url.startsWith('devtools://'));
			if (page) return page.webSocketDebuggerUrl;
		} catch {}
		await sleep(250);
	}
	throw new Error(`devtools endpoint on ${port} not reachable`);
}

export class Cdp {
	constructor(url, {timeoutMs = 30000} = {}) {
		this.id = 0;
		this.pending = new Map();
		this.events = [];
		this.handlers = new Map();
		this.timeoutMs = timeoutMs;
		this.closed = false;
		this.onReconnect = null; // async (cdp) => void, re-applies per-session setup after navigate()
		this.url = url;
		this.attach(new WebSocket(url));
	}
	attach(ws) {
		this.ws = ws;
		this.closed = false;
		ws.onmessage = (m) => {
			const msg = JSON.parse(m.data);
			if (msg.id && this.pending.has(msg.id)) {
				const {resolve, reject, timer} = this.pending.get(msg.id);
				clearTimeout(timer);
				this.pending.delete(msg.id);
				msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
			} else if (msg.method && this.handlers.has(msg.method)) {
				for (const h of this.handlers.get(msg.method)) h(msg.params);
			} else if (msg.method === 'Runtime.exceptionThrown') {
				this.events.push(msg.params.exceptionDetails?.text);
			}
		};
		const fail = (why) => {
			if (this.ws !== ws) return; // stale socket after reconnect
			this.closed = true;
			this.rejectAll(new Error(`CDP socket ${why}`));
		};
		ws.addEventListener('close', () => fail('closed'));
		ws.addEventListener('error', () => fail('error'));
	}
	rejectAll(err) {
		for (const [id, p] of this.pending) {
			clearTimeout(p.timer);
			p.reject(err);
			this.pending.delete(id);
		}
	}
	open() {
		return new Promise((r, j) => {
			if (this.ws.readyState === 1) return r();
			this.ws.addEventListener('open', () => r(), {once: true});
			this.ws.addEventListener('error', (e) => j(new Error(`CDP open failed: ${e?.message ?? 'error'}`)), {once: true});
		});
	}
	on(method, handler) {
		if (!this.handlers.has(method)) this.handlers.set(method, []);
		this.handlers.get(method).push(handler);
	}
	send(method, params = {}, timeoutMs = this.timeoutMs) {
		return new Promise((resolve, reject) => {
			if (this.closed || this.ws.readyState !== 1) return reject(new Error(`CDP socket not open (${method})`));
			const id = ++this.id;
			const timer = setTimeout(() => {
				this.pending.delete(id);
				reject(new Error(`CDP ${method} timed out after ${timeoutMs}ms`));
			}, timeoutMs);
			this.pending.set(id, {resolve, reject, timer});
			try {
				this.ws.send(JSON.stringify({id, method, params}));
			} catch (e) {
				clearTimeout(timer);
				this.pending.delete(id);
				reject(e);
			}
		});
	}
	async eval(expr, timeoutMs) {
		const r = await this.send('Runtime.evaluate', {expression: expr, awaitPromise: true, returnByValue: true}, timeoutMs);
		if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
		return r.result.value;
	}
	// Replaces the socket with a fresh one to the (re-resolved) page target. Keeps handlers/pending API.
	async reconnect(port, tries = 60) {
		try { this.ws.close(); } catch {}
		this.rejectAll(new Error('CDP reconnecting'));
		this.url = await pageWsUrl(port, tries);
		this.attach(new WebSocket(this.url));
		await this.open();
		if (this.onReconnect) await this.onReconnect(this);
	}
}

// Navigates (or reloads when url is omitted) and survives the renderer target being replaced:
// the old socket may go silent or close, so we do not trust the Page.navigate reply, re-resolve the
// target, reconnect and wait until `selector` exists. Bounded by overallMs; throws on timeout.
export async function navigate(cdp, port, url, {selector = '[data-flx*="message-username"]', overallMs = 120000, settleMs = 500} = {}) {
	const deadline = Date.now() + overallMs;
	const left = () => Math.max(1000, deadline - Date.now());
	const fire = url ? cdp.send('Page.navigate', {url}, 8000) : cdp.send('Page.reload', {}, 8000);
	await fire.catch(() => {}); // timeout/close here is expected when the target is replaced
	await sleep(settleMs);
	let lastErr = 'no attempt';
	while (Date.now() < deadline) {
		try {
			if (cdp.closed || cdp.ws.readyState !== 1) await cdp.reconnect(port, 8);
			const ok = await cdp.eval(`!!document.querySelector(${JSON.stringify(selector)})`, Math.min(8000, left()));
			if (ok) return true;
			lastErr = `selector not present: ${selector}`;
		} catch (e) {
			lastErr = e.message;
			try {
				await cdp.reconnect(port, 8); // silent socket (timeout) -> force a fresh one
			} catch (e2) {
				lastErr = e2.message;
			}
		}
		await sleep(500);
	}
	throw new Error(`navigate(${url ?? 'reload'}) did not reach ${selector} within ${overallMs}ms (${lastErr})`);
}

// Runs in the page: every visible [data-flx] element with geometry, the styles that decide
// visual parity, and only its own (non-descendant) text so parents don't duplicate children.
export const DOM_DUMP = `(() => {
  const keys = ['color','backgroundColor','fontSize','fontWeight','fontFamily','lineHeight','borderRadius',
    'borderTopWidth','borderTopColor','paddingTop','paddingRight','paddingBottom','paddingLeft','gap','opacity'];
  const out = [];
  for (const el of document.querySelectorAll('[data-flx]')) {
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1 || r.bottom < 0 || r.right < 0 || r.top > innerHeight || r.left > innerWidth) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) === 0) continue;
    const own = [...el.childNodes].filter(n => n.nodeType === 3).map(n => n.textContent.trim()).filter(Boolean).join(' ');
    const style = {}; for (const k of keys) style[k] = cs[k];
    out.push({flx: el.getAttribute('data-flx'), tag: el.tagName.toLowerCase(),
      role: el.getAttribute('role'), label: el.getAttribute('aria-label'),
      rect: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
      text: own.slice(0, 200), style});
  }
  return out;
})()`;

// Page-expression helpers bound to one CDP session.
export function helpers(cdp) {
	async function waitFor(expr, ms = 20000) {
		const end = Date.now() + ms;
		while (Date.now() < end) {
			if (await cdp.eval(expr, Math.min(10000, Math.max(1000, end - Date.now()))).catch(() => false)) return true;
			await sleep(250);
		}
		return false;
	}
	// Clicks the element returned by a page expression with real input events (React handlers that
	// ignore synthetic .click() still fire). Returns false when the element is missing.
	async function realClickExpr(elementExpr, button = 'left') {
		const pt = await cdp.eval(`(() => { const el = ${elementExpr};
		  if (!el) return null; el.scrollIntoView({block: 'nearest'}); const r = el.getBoundingClientRect();
		  return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
		if (!pt) return false;
		const [x, y] = pt;
		await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x, y});
		for (const type of ['mousePressed', 'mouseReleased']) {
			await cdp.send('Input.dispatchMouseEvent', {type, x, y, button, clickCount: 1});
		}
		return true;
	}
	const realClick = (selector, button) => realClickExpr(`document.querySelector(${JSON.stringify(selector)})`, button);
	async function pressKey(key, vk) {
		for (const type of ['keyDown', 'keyUp']) {
			await cdp.send('Input.dispatchKeyEvent', {type, key, code: key, windowsVirtualKeyCode: vk});
		}
	}
	// Deepest visible element whose trimmed text (or aria-label) matches `pattern`, clicked through its
	// nearest interactive ancestor. Pattern/flags are passed as data, so no escaping pitfalls.
	const byText = (pattern, flags = 'i') => `(() => { const re = new RegExp(${JSON.stringify(pattern)}, ${JSON.stringify(flags)});
	  const vis = (e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
	  const hits = [...document.querySelectorAll('body *')].filter(e => vis(e) && (re.test((e.innerText || '').trim()) || re.test(e.getAttribute('aria-label') || '')));
	  const leaf = hits.filter(e => !hits.some(o => o !== e && e.contains(o))).pop();
	  return leaf ? (leaf.closest('button,a,[role="button"],[role="menuitem"],[role="tab"],[role="option"],[tabindex]') ?? leaf) : null; })()`;
	const clickText = (pattern, flags) => realClickExpr(byText(pattern, flags));
	return {waitFor, realClickExpr, realClick, pressKey, byText, clickText};
}

// Every visible <svg> with its nearest data-flx host, rendered rect, viewBox and path data
// (identified against Phosphor originals by tools/parity_lab/icon_sites.py / harvest_icons.mjs).
export const SVG_DUMP = `[...document.querySelectorAll('svg')].map(s => { const r = s.getBoundingClientRect();
  if (r.width < 6 || r.height < 6 || r.bottom < 0 || r.top > innerHeight) return null;
  const host = s.closest('[data-flx]');
  return {flx: (s.getAttribute('data-flx') || (host && host.getAttribute('data-flx')) || ''), rect: [r.x, r.y, r.width, r.height].map(Math.round),
    viewBox: s.getAttribute('viewBox'), d: [...s.querySelectorAll('path')].map(p => p.getAttribute('d') || '').join('|')}; }).filter(Boolean)`;

// Official build stamp ("Stable Desktop …, Web …") so every capture records which version it is
// ground truth for; comparisons against a different fluxer_app commit are flagged, not trusted.
export async function clientInfo(cdp) {
	return cdp
		.eval(`(async () => { const d = window.__FLUXER_DEBUG__; try { return d && typeof d.getClientInfo === 'function' ? String(await d.getClientInfo()) : null; } catch { return null; } })()`)
		.catch(() => null);
}
