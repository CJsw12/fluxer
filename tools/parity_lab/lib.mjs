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
	constructor(url) {
		this.ws = new WebSocket(url);
		this.id = 0;
		this.pending = new Map();
		this.events = [];
		this.handlers = new Map();
		this.ws.onmessage = (m) => {
			const msg = JSON.parse(m.data);
			if (msg.id && this.pending.has(msg.id)) {
				const {resolve, reject} = this.pending.get(msg.id);
				this.pending.delete(msg.id);
				msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
			} else if (msg.method && this.handlers.has(msg.method)) {
				for (const h of this.handlers.get(msg.method)) h(msg.params);
			} else if (msg.method === 'Runtime.exceptionThrown') {
				this.events.push(msg.params.exceptionDetails?.text);
			}
		};
	}
	open() {
		return new Promise((r, j) => {
			this.ws.onopen = r;
			this.ws.onerror = j;
		});
	}
	on(method, handler) {
		if (!this.handlers.has(method)) this.handlers.set(method, []);
		this.handlers.get(method).push(handler);
	}
	send(method, params = {}) {
		const id = ++this.id;
		this.ws.send(JSON.stringify({id, method, params}));
		return new Promise((resolve, reject) => this.pending.set(id, {resolve, reject}));
	}
	async eval(expr) {
		const r = await this.send('Runtime.evaluate', {expression: expr, awaitPromise: true, returnByValue: true});
		if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
		return r.result.value;
	}
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
		for (let t = 0; t < ms; t += 250) {
			if (await cdp.eval(expr).catch(() => false)) return true;
			await sleep(250);
		}
		return false;
	}
	// Clicks the element returned by a page expression with real input events (React handlers that
	// ignore synthetic .click() still fire). Returns false when the element is missing.
	async function realClickExpr(elementExpr) {
		const pt = await cdp.eval(`(() => { const el = ${elementExpr};
		  if (!el) return null; el.scrollIntoView({block: 'nearest'}); const r = el.getBoundingClientRect();
		  return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
		if (!pt) return false;
		const [x, y] = pt;
		await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x, y});
		for (const type of ['mousePressed', 'mouseReleased']) {
			await cdp.send('Input.dispatchMouseEvent', {type, x, y, button: 'left', clickCount: 1});
		}
		return true;
	}
	const realClick = (selector) => realClickExpr(`document.querySelector(${JSON.stringify(selector)})`);
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

// Official build stamp ("Stable Desktop …, Web …") so every capture records which version it is
// ground truth for; comparisons against a different fluxer_app commit are flagged, not trusted.
export async function clientInfo(cdp) {
	return cdp
		.eval(`(async () => { const d = window.__FLUXER_DEBUG__; try { return d && typeof d.getClientInfo === 'function' ? String(await d.getClientInfo()) : null; } catch { return null; } })()`)
		.catch(() => null);
}
