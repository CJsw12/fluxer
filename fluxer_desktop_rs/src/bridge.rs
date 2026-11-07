// SPDX-License-Identifier: AGPL-3.0-or-later

/// 웹뷰가 로드되기 직전에 주입되는 JavaScript Preload 스크립트입니다.
/// 원본 React/TypeScript UI 코드를 1글자도 수정하지 않고도,
/// window.electron API 및 타이틀바 무지연 Win32 네이티브 드래그를 완벽하게 모방합니다.
pub fn get_initialization_script() -> &'static str {
    r#"
(function() {
    'use strict';

    // 1. 비동기 Invoke/Reply 통신 파이프라인
    let nextRequestId = 1;
    const pendingInvokes = new Map();

    window.__fluxerIpcReply = function(id, result, error) {
        const promise = pendingInvokes.get(id);
        if (!promise) return;
        pendingInvokes.delete(id);
        if (error) {
            promise.reject(new Error(error));
        } else {
            promise.resolve(result);
        }
    };

    // 1-0. Rust 네이티브 게이트웨이 WebSocket 실시간 이벤트 디스패처
    let nextGatewayListenerId = 1;
    const gatewayListeners = new Map();

    window.__fluxerNativeGatewayEmit = function(event) {
        if (!event) return;
        if (event.binary && typeof event.binary === 'string') {
            try {
                var binaryStr = atob(event.binary);
                var len = binaryStr.length;
                var bytes = new Uint8Array(len);
                for (var i = 0; i < len; i++) {
                    bytes[i] = binaryStr.charCodeAt(i);
                }
                event.binary = bytes.buffer;
            } catch (err) {
                console.error('[Fluxer Shell] Failed to decode gateway binary payload:', err);
            }
        }
        gatewayListeners.forEach(function(listener) {
            try {
                listener(event);
            } catch (err) {
                console.error('[Fluxer Shell] Error in gateway listener callback:', err);
            }
        });
    };

    function invokeRust(cmd, payload) {
        return new Promise(function(resolve, reject) {
            const id = nextRequestId++;
            pendingInvokes.set(id, { resolve: resolve, reject: reject });
            try {
                window.ipc.postMessage(JSON.stringify({
                    id: id,
                    cmd: cmd,
                    payload: payload || {}
                }));
            } catch (err) {
                pendingInvokes.delete(id);
                reject(err);
            }
        });
    }

    function sendRust(cmd, payload) {
        try {
            window.ipc.postMessage(JSON.stringify({
                cmd: cmd,
                payload: payload || {}
            }));
        } catch (err) {
            console.error('[Fluxer Shell] Failed to send message:', cmd, err);
        }
    }

    // 1-1. 웹뷰 내부 에러 및 경고를 Rust 콘솔로 실시간 포워딩
    var origConsoleError = console.error;
    var origConsoleWarn = console.warn;
    console.error = function() {
        try {
            var msg = Array.prototype.slice.call(arguments).map(function(a) {
                return (a instanceof Error) ? (a.stack || a.message) : (typeof a === 'object' ? JSON.stringify(a) : String(a));
            }).join(' ');
            sendRust('consoleLog', { level: 'error', message: msg });
        } catch (_) {}
        origConsoleError.apply(console, arguments);
    };
    console.warn = function() {
        try {
            var msg = Array.prototype.slice.call(arguments).map(function(a) {
                return (a instanceof Error) ? (a.stack || a.message) : (typeof a === 'object' ? JSON.stringify(a) : String(a));
            }).join(' ');
            sendRust('consoleLog', { level: 'warn', message: msg });
        } catch (_) {}
        origConsoleWarn.apply(console, arguments);
    };
    window.addEventListener('error', function(e) {
        try {
            sendRust('consoleLog', { level: 'error', message: 'Window error: ' + (e.error ? (e.error.stack || e.error.message) : e.message) });
        } catch (_) {}
    });
    window.addEventListener('unhandledrejection', function(e) {
        try {
            sendRust('consoleLog', { level: 'error', message: 'Unhandled rejection: ' + (e.reason ? (e.reason.stack || e.reason.message) : String(e.reason)) });
        } catch (_) {}
    });

    // 2. 외부 셀프호스트 인스턴스 CORS 우회용 네이티브 Fetch 프록시
    const originalFetch = window.fetch;
    window.fetch = async function(input, init) {
        var url = typeof input === 'string' ? input : (input && input.url ? input.url : '');
        if (url && (url.startsWith('http://') || url.startsWith('https://'))) {
            var urlObj = null;
            try { urlObj = new URL(url); } catch (_) {}
            if (urlObj && urlObj.origin !== window.location.origin) {
                try {
                    var method = (init && init.method) || (input && input.method) || 'GET';
                    var headers = {};
                    if (init && init.headers) {
                        if (init.headers instanceof Headers) {
                            init.headers.forEach(function(v, k) { headers[k] = v; });
                        } else if (Array.isArray(init.headers)) {
                            init.headers.forEach(function(pair) { headers[pair[0]] = pair[1]; });
                        } else {
                            headers = init.headers;
                        }
                    }
                    var body = (init && init.body) ? String(init.body) : null;

                    var resp = await invokeRust('fetchProxy', {
                        url: url,
                        method: method,
                        headers: headers,
                        body: body
                    });

                    var responseHeaders = new Headers(resp.headers || {});
                    return new Response(resp.body, {
                        status: resp.status,
                        statusText: resp.statusText || 'OK',
                        headers: responseHeaders
                    });
                } catch (proxyErr) {
                    console.warn('[Fluxer Native] Fetch proxy fallback to original:', proxyErr);
                }
            }
        }
        return originalFetch.apply(this, arguments);
    };

    // 2-1. 외부 셀프호스트 인스턴스 로그인 및 REST API CORS 우회용 네이티브 XMLHttpRequest 프록시
    const OriginalXHR = window.XMLHttpRequest;
    function ProxyXMLHttpRequest() {
        this._native = new OriginalXHR();
        this._method = 'GET';
        this._url = '';
        this._isCrossOrigin = false;
        this._aborted = false;
        this._reqHeaders = {};
        this._respHeaders = {};
        this._listeners = {};
        this.readyState = 0;
        this.status = 0;
        this.statusText = '';
        this.responseText = '';
        this.response = '';
        this.responseType = '';
        this.timeout = 0;
        this.withCredentials = false;
        this.onload = null;
        this.onerror = null;
        this.onabort = null;
        this.ontimeout = null;
        this.onloadend = null;
        this.onreadystatechange = null;
        var nativeUpload = this._native.upload;
        this.upload = {
            addEventListener: function(t, fn, opt) {
                if (nativeUpload && nativeUpload.addEventListener) nativeUpload.addEventListener(t, fn, opt);
            },
            removeEventListener: function(t, fn, opt) {
                if (nativeUpload && nativeUpload.removeEventListener) nativeUpload.removeEventListener(t, fn, opt);
            },
            dispatchEvent: function(ev) {
                return nativeUpload && nativeUpload.dispatchEvent ? nativeUpload.dispatchEvent(ev) : true;
            }
        };
    }
    ProxyXMLHttpRequest.UNSENT = 0;
    ProxyXMLHttpRequest.OPENED = 1;
    ProxyXMLHttpRequest.HEADERS_RECEIVED = 2;
    ProxyXMLHttpRequest.LOADING = 3;
    ProxyXMLHttpRequest.DONE = 4;
    ProxyXMLHttpRequest.prototype.UNSENT = 0;
    ProxyXMLHttpRequest.prototype.OPENED = 1;
    ProxyXMLHttpRequest.prototype.HEADERS_RECEIVED = 2;
    ProxyXMLHttpRequest.prototype.LOADING = 3;
    ProxyXMLHttpRequest.prototype.DONE = 4;

    ProxyXMLHttpRequest.prototype._emit = function(type) {
        var ev = new ProgressEvent(type);
        var propHandler = this['on' + type];
        if (typeof propHandler === 'function') {
            try { propHandler.call(this, ev); } catch (e) { console.error(e); }
        }
        var list = this._listeners[type];
        if (list && list.length > 0) {
            var copy = list.slice();
            for (var i = 0; i < copy.length; i++) {
                var item = copy[i];
                if (item.once) {
                    this.removeEventListener(type, item.fn);
                }
                try { item.fn.call(this, ev); } catch (e) { console.error(e); }
            }
        }
    };

    ProxyXMLHttpRequest.prototype.addEventListener = function(type, fn, options) {
        if (typeof fn !== 'function') return;
        if (!this._listeners[type]) this._listeners[type] = [];
        var once = Boolean(options && typeof options === 'object' && options.once);
        this._listeners[type].push({ fn: fn, once: once });
    };

    ProxyXMLHttpRequest.prototype.removeEventListener = function(type, fn) {
        var list = this._listeners[type];
        if (!list) return;
        this._listeners[type] = list.filter(function(item) { return item.fn !== fn; });
    };

    ProxyXMLHttpRequest.prototype.open = function(method, url, async, user, password) {
        this._method = method || 'GET';
        this._url = String(url || '');
        this._reqHeaders = {};
        this._respHeaders = {};
        this._aborted = false;
        var isCross = false;
        try {
            var parsed = new URL(this._url, window.location.href);
            if ((parsed.protocol === 'http:' || parsed.protocol === 'https:') && parsed.origin !== window.location.origin) {
                isCross = true;
                this._url = parsed.toString();
            }
        } catch (_) {}
        this._isCrossOrigin = isCross;
        if (!isCross) {
            this._native.open(this._method, this._url, async !== false, user, password);
        }
        this.readyState = 1;
    };

    ProxyXMLHttpRequest.prototype.setRequestHeader = function(name, value) {
        this._reqHeaders[name] = String(value);
        if (!this._isCrossOrigin) {
            this._native.setRequestHeader(name, value);
        }
    };

    ProxyXMLHttpRequest.prototype.getAllResponseHeaders = function() {
        if (!this._isCrossOrigin) {
            return this._native.getAllResponseHeaders();
        }
        var lines = [];
        var headers = this._respHeaders || {};
        Object.keys(headers).forEach(function(k) {
            lines.push(k + ': ' + headers[k]);
        });
        return lines.join('\r\n');
    };

    ProxyXMLHttpRequest.prototype.getResponseHeader = function(name) {
        if (!this._isCrossOrigin) {
            return this._native.getResponseHeader(name);
        }
        var target = String(name || '').toLowerCase();
        var headers = this._respHeaders || {};
        var keys = Object.keys(headers);
        for (var i = 0; i < keys.length; i++) {
            if (keys[i].toLowerCase() === target) {
                return headers[keys[i]];
            }
        }
        return null;
    };

    ProxyXMLHttpRequest.prototype.abort = function() {
        this._aborted = true;
        if (!this._isCrossOrigin) {
            this._native.abort();
            return;
        }
        this.readyState = 4;
        this._emit('readystatechange');
        this._emit('abort');
        this._emit('loadend');
    };

    ProxyXMLHttpRequest.prototype.send = function(body) {
        var self = this;
        if (!this._isCrossOrigin) {
            var nat = this._native;
            nat.responseType = this.responseType;
            nat.timeout = this.timeout;
            nat.withCredentials = this.withCredentials;
            var syncFromNative = function() {
                self.readyState = nat.readyState;
                try { self.status = nat.status; } catch (_) {}
                try { self.statusText = nat.statusText; } catch (_) {}
                try { self.response = nat.response; } catch (_) {}
                if (self.responseType === '' || self.responseType === 'text') {
                    try { self.responseText = nat.responseText; } catch (_) {}
                }
            };
            ['readystatechange', 'load', 'error', 'abort', 'timeout', 'loadend'].forEach(function(evName) {
                nat.addEventListener(evName, function() {
                    syncFromNative();
                    self._emit(evName);
                });
            });
            nat.send(body);
            return;
        }

        // 외부 셀프호스트 인스턴스 요청 시 RestTransport가 off-origin이라는 이유로 생략한 인증 및 기능 헤더 보강
        var hasAuth = Object.keys(self._reqHeaders).some(function(k) {
            return k.toLowerCase() === 'authorization';
        });
        var isUnauthRoute = /\/auth\/(login|register|forgot|reset|handoff|sso)|\.well-known\/fluxer/i.test(self._url);
        if (!hasAuth && !isUnauthRoute) {
            try {
                var storedToken = window.__fluxerLastAuthToken || localStorage.getItem('token') || localStorage.getItem('__fluxer_last_token');
                if (!storedToken) {
                    var rawAccount = localStorage.getItem('__fluxer_last_account');
                    if (rawAccount) {
                        var acc = JSON.parse(rawAccount);
                        storedToken = acc && (acc.token || (acc.record && acc.record.token));
                    }
                }
                if (storedToken && storedToken !== 'null' && storedToken !== 'undefined') {
                    self._reqHeaders['Authorization'] = storedToken;
                }
            } catch (_) {}
        }
        var hasFeatures = Object.keys(self._reqHeaders).some(function(k) {
            return k.toLowerCase() === 'x-fluxer-features';
        });
        if (!hasFeatures) {
            self._reqHeaders['X-Fluxer-Features'] = 'view_channel_members_permission,channel_threads';
            self._reqHeaders['X-Fluxer-Capabilities'] = 'guild_visits';
        }

        var bodyStr = (body !== undefined && body !== null) ? String(body) : null;
        invokeRust('fetchProxy', {
            url: self._url,
            method: self._method,
            headers: self._reqHeaders,
            body: bodyStr
        }).then(function(resp) {
            if (self._aborted) return;
            self.status = resp.status || 200;
            self.statusText = resp.statusText || 'OK';
            self._respHeaders = resp.headers || {};
            self.responseText = typeof resp.body === 'string' ? resp.body : '';
            if (self.responseType === 'json') {
                try { self.response = JSON.parse(self.responseText); } catch (_) { self.response = null; }
            } else if (self.responseType === 'blob') {
                self.response = new Blob([self.responseText]);
            } else {
                self.response = self.responseText;
            }
            if (self.status >= 200 && self.status < 300 && self.responseText) {
                try {
                    var parsedBody = JSON.parse(self.responseText);
                    if (parsedBody && typeof parsedBody.token === 'string' && parsedBody.token.length > 0) {
                        window.__fluxerLastAuthToken = parsedBody.token;
                    }
                } catch (_) {}
            }
            self.readyState = 4;
            self._emit('readystatechange');
            self._emit('load');
            self._emit('loadend');
        }).catch(function(err) {
            if (self._aborted) return;
            console.warn('[Fluxer Native] XHR proxy error:', err);
            self.status = 0;
            self.statusText = '';
            self.readyState = 4;
            self._emit('readystatechange');
            self._emit('error');
            self._emit('loadend');
        });
    };
    window.XMLHttpRequest = ProxyXMLHttpRequest;

    // 2-2. 화면 공유 (Screen Share / getDisplayMedia) 로깅 및 파이프라인
    try {
        if (navigator.mediaDevices && navigator.mediaDevices.getDisplayMedia) {
            const rawGetDisplayMedia = navigator.mediaDevices.getDisplayMedia.bind(navigator.mediaDevices);
            navigator.mediaDevices.getDisplayMedia = async function(constraints) {
                sendRust('consoleLog', { level: 'info', message: '[ScreenShare] navigator.mediaDevices.getDisplayMedia requested' });
                try {
                    const stream = await rawGetDisplayMedia(constraints);
                    sendRust('consoleLog', { level: 'info', message: '[ScreenShare] getDisplayMedia succeeded with ' + stream.getTracks().length + ' tracks' });
                    return stream;
                } catch (err) {
                    sendRust('consoleLog', { level: 'error', message: '[ScreenShare] getDisplayMedia failed: ' + (err.name || '') + ': ' + (err.message || String(err)) });
                    throw err;
                }
            };
        }
    } catch (_) {}

    // 2-3. HTML5 Fullscreen API 감지 및 Win32 창 모니터 전체화면 자동 동기화
    function syncNativeFullscreen() {
        var isFs = Boolean(document.fullscreenElement || document.webkitFullscreenElement || document.mozFullScreenElement || document.msFullscreenElement);
        sendRust('windowSetFullscreen', { fullscreen: isFs });
    }
    document.addEventListener('fullscreenchange', syncNativeFullscreen);
    document.addEventListener('webkitfullscreenchange', syncNativeFullscreen);
    document.addEventListener('mozfullscreenchange', syncNativeFullscreen);
    document.addEventListener('MSFullscreenChange', syncNativeFullscreen);

    // 2-4. 시스템 트레이 및 설정 이벤트 디스패처
    var _openSettingsListeners = [];
    window.__fluxerEmitOpenSettings = function() {
        _openSettingsListeners.forEach(function(fn) {
            try { fn(); } catch(e) { console.error('[Fluxer] onOpenSettings listener error', e); }
        });
    };
    var _trayActionListeners = [];
    window.__fluxerEmitTrayAction = function(payload) {
        _trayActionListeners.forEach(function(fn) {
            try { fn(payload); } catch(e) { console.error('[Fluxer] onTrayAction listener error', e); }
        });
    };

    // 2. 최대화 상태 로컬 캐시 관리
    let isMaximizedCached = false;

    // 3. window.electron 호환 객체 정의 (Zero-Touch UI 보장)
    const electronApi = {
        platform: 'win32',
        buildChannel: 'stable',
        offlineBuild: false,

        // 창 제어 API
        windowMinimize: function() {
            sendRust('windowMinimize');
        },
        windowMaximize: function() {
            sendRust('windowMaximize');
        },
        windowClose: function() {
            sendRust('windowClose');
        },
        windowIsMaximized: function() {
            return invokeRust('windowIsMaximized').then(function(isMax) {
                isMaximizedCached = Boolean(isMax);
                return isMaximizedCached;
            });
        },
        onWindowMaximizeChange: function(callback) {
            const handler = function(e) {
                isMaximizedCached = Boolean(e.detail && e.detail.isMaximized);
                callback(isMaximizedCached);
            };
            window.addEventListener('fluxer:maximize-change', handler);
            return function() {
                window.removeEventListener('fluxer:maximize-change', handler);
            };
        },
        onWindowLiveResizeChange: function(callback) {
            const handler = function(e) {
                callback(Boolean(e.detail && e.detail.isResizing));
            };
            window.addEventListener('fluxer:resize-change', handler);
            return function() {
                window.removeEventListener('fluxer:resize-change', handler);
            };
        },
        notifyFirstContentPainted: function() {
            sendRust('firstContentPainted');
        },

        // 시스템 및 진단 API
        getDesktopInfo: function() {
            return invokeRust('getDesktopInfo');
        },
        getGpuInfo: function() {
            return invokeRust('getGpuInfo');
        },
        getAppMetrics: function() {
            return invokeRust('getAppMetrics');
        },
        getSystemIdleTimeMs: function() {
            return invokeRust('systemIdleTimeMs');
        },

        // 창 동작 설정 API
        getDesktopWindowBehaviorSettings: function() {
            return invokeRust('desktopWindowBehaviorGet');
        },
        setDesktopWindowBehaviorSettings: function(settings) {
            return invokeRust('desktopWindowBehaviorSet', settings);
        },
        getDesktopWindowBehaviorPendingRestart: function() {
            return invokeRust('desktopWindowBehaviorPendingRestart');
        },
        desktopAppRelaunch: function() {
            return invokeRust('desktopAppRelaunch');
        },

        // 외부 링크 및 클립보드 API
        openExternal: function(url) {
            return invokeRust('openExternal', { url: url });
        },
        clipboardWriteText: function(text) {
            return invokeRust('clipboardWriteText', { text: text });
        },
        clipboardReadText: function() {
            return invokeRust('clipboardReadText');
        },
        clipboardWriteFile: function() {
            return Promise.resolve({ success: false, error: 'Not supported on Rust Shell yet' });
        },
        pasteFromClipboard: function() {
            return Promise.resolve();
        },

        // 딥링크 및 알림 API (DeepLinkUtils, NotificationUtils 호환)
        getInitialDeepLink: function() {
            return Promise.resolve(null);
        },
        onDeepLink: function(callback) {
            return function() {};
        },
        onNotificationClick: function(callback) {
            return function() {};
        },
        showNotification: function(payload) {
            return Promise.resolve({ id: 'stub-notification-' + Date.now() });
        },
        shouldPlayNotificationSound: function() {
            return Promise.resolve(true);
        },
        closeNotification: function(id) {},
        closeNotifications: function(ids) {},

        // 스펠체크 API (Spellcheck.ts 호환)
        onSpellcheckStateChanged: function(callback) {
            return function() {};
        },
        onSpellcheckEngineResolved: function(callback) {
            return function() {};
        },
        spellcheckGetAvailableLanguages: function() {
            return Promise.resolve(['en-US']);
        },
        spellcheckGetBundledDictionaries: function() {
            return Promise.resolve([]);
        },
        spellcheckSetState: function(state) {
            return Promise.resolve(state || {});
        },
        spellcheckReplaceMisspelling: function(word) {},
        spellcheckAddWordToDictionary: function(word) {},
        onTextareaContextMenu: function(callback) {
            return function() {};
        },

        // 미디어 권한 및 시작 프로그램
        checkMediaAccess: function(type) {
            return invokeRust('checkMediaAccess', { type: type });
        },
        requestMediaAccess: function(type) {
            return invokeRust('requestMediaAccess', { type: type });
        },
        openMediaAccessSettings: function(type) {
            return Promise.resolve();
        },
        openInputMonitoringSettings: function() {
            return Promise.resolve();
        },
        checkInputMonitoringAccess: function() {
            return Promise.resolve(true);
        },
        getInputMonitoringPermissionStatus: function() {
            return Promise.resolve('granted');
        },
        requestInputMonitoringPermission: function() {
            return Promise.resolve('granted');
        },
        getScreenRecordingPermissionStatus: function() {
            return Promise.resolve('granted');
        },
        requestScreenRecordingPermission: function() {
            return Promise.resolve('granted');
        },
        autostartIsEnabled: function() {
            return invokeRust('autostartIsEnabled');
        },
        autostartEnable: function() {
            return invokeRust('autostartEnable');
        },
        autostartDisable: function() {
            return invokeRust('autostartDisable');
        },
        autostartIsInitialized: function() {
            return Promise.resolve(true);
        },
        autostartMarkInitialized: function() {
            return Promise.resolve();
        },

        // 글로벌 키훅 및 숏컷
        globalKeyHookStart: function() {
            return Promise.resolve(false);
        },
        globalKeyHookStop: function() {
            return Promise.resolve();
        },
        globalKeyHookRegister: function(options) {
            return Promise.resolve();
        },
        globalKeyHookUnregister: function(id) {
            return Promise.resolve();
        },
        globalKeyHookUnregisterAll: function() {
            return Promise.resolve();
        },
        onGlobalKeyEvent: function(callback) {
            return function() {};
        },
        onGlobalMouseEvent: function(callback) {
            return function() {};
        },
        onGlobalKeybindTriggered: function(callback) {
            return function() {};
        },
        linuxEvdevStatus: function() {
            return Promise.resolve(null);
        },
        globalShortcuts: {
            sync: function() { return Promise.resolve(); },
            setPaused: function() { return Promise.resolve(); },
            getStatus: function() {
                return Promise.resolve({
                    state: 'unavailable',
                    version: null,
                    canConfigure: false,
                    recovering: false,
                    canRecheck: false,
                    portalAppId: null,
                    shortcuts: [],
                    error: null
                });
            },
            onStatus: function(cb) { return function() {}; },
            onEvent: function(cb) { return function() {}; },
            setUp: function() {
                return Promise.resolve({
                    state: 'unavailable',
                    version: null,
                    canConfigure: false,
                    recovering: false,
                    canRecheck: false,
                    portalAppId: null,
                    shortcuts: [],
                    error: null
                });
            },
            recheck: function() {
                return Promise.resolve({
                    state: 'unavailable',
                    version: null,
                    canConfigure: false,
                    recovering: false,
                    canRecheck: false,
                    portalAppId: null,
                    shortcuts: [],
                    error: null
                });
            },
            configure: function() { return Promise.resolve(); },
            setDirectInputEnabled: function() {
                return Promise.resolve({
                    state: 'unavailable',
                    version: null,
                    canConfigure: false,
                    recovering: false,
                    canRecheck: false,
                    portalAppId: null,
                    shortcuts: [],
                    error: null
                });
            },
            startCapture: function() { return Promise.resolve(null); },
            stopCapture: function() { return Promise.resolve(); },
            onCapture: function(cb) { return function() {}; }
        },

        // 화면 공유 및 디스플레이 미디어
        getDesktopSources: function(types, reqId, opt) {
            var requestedTypes = Array.isArray(types) ? types : ['screen', 'window'];
            var sources = [];

            function makeThumbnail(label, isWindow) {
                try {
                    var canvas = document.createElement('canvas');
                    canvas.width = 320;
                    canvas.height = 180;
                    var ctx = canvas.getContext('2d');
                    if (ctx) {
                        ctx.fillStyle = '#1e1f22';
                        ctx.fillRect(0, 0, 320, 180);

                        ctx.fillStyle = '#2b2d31';
                        ctx.strokeStyle = isWindow ? '#23a55a' : '#5865f2';
                        ctx.lineWidth = 4;
                        if (typeof ctx.roundRect === 'function') {
                            ctx.roundRect(24, 16, 272, 148, 8);
                        } else {
                            ctx.rect(24, 16, 272, 148);
                        }
                        ctx.fill();
                        ctx.stroke();

                        if (isWindow) {
                            ctx.fillStyle = '#35373c';
                            ctx.fillRect(24, 16, 272, 24);
                            ctx.fillStyle = '#ed4245';
                            ctx.beginPath(); ctx.arc(38, 28, 4, 0, Math.PI * 2); ctx.fill();
                            ctx.fillStyle = '#faa81a';
                            ctx.beginPath(); ctx.arc(50, 28, 4, 0, Math.PI * 2); ctx.fill();
                            ctx.fillStyle = '#57f287';
                            ctx.beginPath(); ctx.arc(62, 28, 4, 0, Math.PI * 2); ctx.fill();
                        } else {
                            ctx.fillStyle = '#4e5058';
                            ctx.fillRect(150, 150, 20, 14);
                            ctx.fillRect(130, 162, 60, 4);
                        }

                        ctx.fillStyle = '#f2f3f5';
                        ctx.font = 'bold 15px sans-serif';
                        ctx.textAlign = 'center';
                        ctx.textBaseline = 'middle';
                        ctx.fillText(label, 160, isWindow ? 100 : 85);
                        return canvas.toDataURL('image/png');
                    }
                } catch (_) {}
                return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
            }

            var screenWidth = (window.screen && window.screen.width) || 1920;
            var screenHeight = (window.screen && window.screen.height) || 1080;

            if (requestedTypes.includes('screen')) {
                sources.push({
                    id: 'screen:0:0',
                    name: 'Entire Screen (전체 화면)',
                    display_id: '0',
                    nativeWidth: screenWidth,
                    nativeHeight: screenHeight,
                    isOwnWindow: false,
                    thumbnailDataUrl: makeThumbnail('Entire Screen', false)
                });
            }

            if (requestedTypes.includes('window')) {
                sources.push({
                    id: 'window:0:0',
                    name: 'Application Window (창 / 앱)',
                    display_id: '0',
                    nativeWidth: 1280,
                    nativeHeight: 720,
                    isOwnWindow: false,
                    thumbnailDataUrl: makeThumbnail('Application Window', true)
                });
            }

            return Promise.resolve(sources);
        },
        onDisplayMediaRequested: function(callback) {
            return function() {};
        },
        onDisplayMediaPortalEmpty: function(callback) {
            return function() {};
        },
        selectDisplayMediaSource: function(requestId, sourceId, withAudio) {},
        setDisplayMediaPortalPreference: function(preference) {
            return Promise.resolve();
        },
        getStreamerModeCaptureAppStatus: function() {
            return Promise.resolve({ active: false });
        },

        // 팝아웃 및 진단
        popoutSetAlwaysOnTop: function(key, flag) {
            return invokeRust('popoutSetAlwaysOnTop', { key: key, flag: Boolean(flag) }).catch(function() { return false; });
        },
        popoutFocus: function(key) {
            return invokeRust('popoutFocus', { key: key }).catch(function() { return false; });
        },
        cssEditorWindow: true,
        openVoiceDebugEventSinkPopout: function(entries) {
            return Promise.resolve();
        },
        appendVoiceDebugEventSinkEntries: function(entries) {},
        setVoiceDebugEventSinkStatsHtml: function(html) {},

        // 테마 관리
        pickThemeLocalFiles: function() { return Promise.resolve([]); },
        readThemeLocalFiles: function(paths) { return Promise.resolve([]); },
        clearThemeLocalFiles: function() { return Promise.resolve(); },
        importThemeDirectory: function() { return Promise.resolve([]); },
        pickThemeLinkedFiles: function(options) { return Promise.resolve([]); },
        watchThemeLinkedFiles: function(paths) { return Promise.resolve(); },
        onThemeLinkedFileChange: function(callback) { return function() {}; },
        importThemeFolder: function() { return Promise.resolve(null); },
        watchThemeFolders: function(paths) { return Promise.resolve(); },
        removeThemeFolder: function(path) { return Promise.resolve(); },
        onThemeFolderChange: function(callback) { return function() {}; },
        themeFolderWatch: true,

        // 보이스 백그라운드 미디어 캐시
        cacheVoiceBackgroundMedia: function(options) { return Promise.resolve({ ok: false }); },
        readVoiceBackgroundMedia: function(id) { return Promise.resolve(null); },
        deleteVoiceBackgroundMedia: function(id) { return Promise.resolve(); },

        // 트러블슈팅
        getDesktopTroubleshootingSettings: function() { return Promise.resolve({}); },
        setDesktopDisableHardwareAcceleration: function(options) { return Promise.resolve({}); },
        setDesktopDisableModuleUpdates: function(options) { return Promise.resolve({}); },

        // 패스키 및 보안
        passkeyIsSupported: function() {
            return invokeRust('passkeyIsSupported');
        },
        passkeyAuthenticate: function(options, requestContext) {
            return Promise.reject(new Error('Passkey not supported in webview'));
        },
        passkeyRegister: function(options, requestContext) {
            return Promise.reject(new Error('Passkey not supported in webview'));
        },

        // 다운로드
        downloadFile: function(url, suggestedName, sha256) {
            return Promise.resolve({ success: false, error: 'Not supported' });
        },

        // 업데이트 모듈
        onUpdaterEvent: function(callback) {
            return function() {};
        },
        updaterCheck: function() {
            return Promise.resolve();
        },
        updaterDownload: function() {
            return Promise.resolve();
        },
        updaterInstall: function() {
            return Promise.resolve();
        },
        desktopUpdate: {
            state: function() { return invokeRust('desktopUpdateState'); },
            start: function() { return Promise.resolve(); },
            onStateChanged: function() { return function() {}; }
        },
        desktopModules: {
            ensure: function(name) { return invokeRust('desktopModulesEnsure', { name: name }); },
            confirmLaunch: function() { return Promise.resolve(); }
        },

        // 줌 및 접근성
        onJumpListNewDm: function(callback) { return function() {}; },
        getAccessibilitySupportEnabled: function() { return Promise.resolve(false); },
        onAccessibilitySupportChanged: function(callback) { return function() {}; },
        getLinuxAppearance: function() { return Promise.resolve(null); },
        setPrebootTheme: function(theme) {},
        onLinuxAppearanceChanged: function(callback) { return function() {}; },
        onZoomIn: function(callback) { return function() {}; },
        onZoomOut: function(callback) { return function() {}; },
        onZoomReset: function(callback) { return function() {}; },
        onOpenSettings: function(callback) {
            if (typeof callback === 'function') {
                _openSettingsListeners.push(callback);
                return function() {
                    _openSettingsListeners = _openSettingsListeners.filter(function(cb) { return cb !== callback; });
                };
            }
            return function() {};
        },
        setTrayRuntimeState: function(state) {},
        setDirectMessagesAvailable: function(available) {},
        acquireStreamingPriority: function() {},
        releaseStreamingPriority: function() {},
        getStreamingPriorityDiagnostics: function() { return Promise.resolve(null); },
        onTrayAction: function(callback) {
            if (typeof callback === 'function') {
                _trayActionListeners.push(callback);
                return function() {
                    _trayActionListeners = _trayActionListeners.filter(function(cb) { return cb !== callback; });
                };
            }
            return function() {};
        },

        // 음성 및 하드웨어 인코더 서브시스템
        virtmic: {
            getAvailability: function() { return Promise.resolve({ available: false }); },
            listTargets: function() { return Promise.resolve({ ok: true, targets: [], availability: { available: false } }); },
            stop: function() { return Promise.resolve(); }
        },
        nativeAudio: {
            getAvailability: function() {
                return invokeRust('nativeAudioGetAvailability');
            },
            listAudibleApplications: function() {
                return invokeRust('nativeAudioListApplications').then(function(res) {
                    return Array.isArray(res) ? res : [];
                });
            },
            resolveAudioRootPidForSource: function(sourceId) {
                return invokeRust('nativeAudioResolveRootPid', { sourceId: sourceId });
            },
            start: function(options) {
                return invokeRust('nativeAudioStart', options || {});
            },
            setRule: function(captureId, linuxRule) {
                return invokeRust('nativeAudioSetRule', { captureId: captureId, linuxRule: linuxRule });
            },
            listPlaybackDevices: function() {
                return invokeRust('nativeAudioListPlaybackDevices').then(function(res) {
                    return Array.isArray(res) ? res : [];
                });
            },
            setWinEndpoints: function(captureId, endpoints) {
                return invokeRust('nativeAudioSetWinEndpoints', { captureId: captureId, endpoints: endpoints || [] });
            },
            stop: function(captureId) {
                return invokeRust('nativeAudioStop', { captureId: captureId });
            },
            onFrame: function(cb) { return function() {}; },
            onEnd: function(cb) { return function() {}; }
        },
        nativeScreenCapture: {
            onLifecycleEvent: function(cb) { return function() {}; }
        },
        voiceEngine: {
            getHardwareEncoderCapabilities: function() {
                return Promise.resolve({ h264: false, hevc: false, av1: false });
            }
        },
        localApp: {
            origin: window.location.origin
        },

        // 런타임 인스턴스 및 데스크톱 스토리지 API (Zero-Touch UI 호환)
        desktopRuntimeConfig: {
            initialInput: function() {
                return invokeRust('desktopRuntimeInitialInput').then(function(savedUrl) {
                    if (savedUrl && typeof savedUrl === 'string' && savedUrl.length > 0) {
                        return savedUrl;
                    }
                    return null;
                }).catch(function() {
                    return null;
                });
            },
            resolve: function(req) {
                var input = (req && req.input) ? req.input : '';
                return invokeRust('desktopRuntimeResolve', { input: input });
            },
            cancelResolution: function() {
                return Promise.resolve();
            },
            prepare: function(req) {
                return Promise.resolve({
                    preparationId: (req && req.preparationId) || 'prep-1',
                    instanceKey: (req && req.instanceKey) || ''
                });
            },
            commit: function(req) {
                if (req && req.instanceKey) {
                    sendRust('commitInstance', { instanceKey: req.instanceKey });
                }
                return Promise.resolve({
                    disposition: 'committed',
                    revision: ((req && req.revision) || 0) + 1,
                    activeInstanceKey: (req && req.instanceKey) || null
                });
            },
            abort: function() {
                return Promise.resolve({
                    disposition: 'aborted',
                    revision: 0,
                    activeInstanceKey: null
                });
            },
            finalize: function() {
                return Promise.resolve();
            },
            rollback: function() {
                return Promise.resolve({
                    revision: 0,
                    activeInstanceKey: null
                });
            },
            deactivate: function() {
                sendRust('deactivateInstance');
                return Promise.resolve({
                    revision: 0,
                    deactivatedInstanceKey: ''
                });
            }
        },
        desktopAccounts: {
            getAll: function() {
                return invokeRust('desktopAccountsGetAll').then(function(res) {
                    return Array.isArray(res) ? res : [];
                });
            },
            get: function(storageKey) {
                return invokeRust('desktopAccountsGet', { storageKey: storageKey });
            },
            upsert: function(account) {
                if (account) {
                    try {
                        var token = account.token || (account.record && account.record.token);
                        if (token) {
                            window.__fluxerLastAuthToken = token;
                            localStorage.setItem('__fluxer_last_account', JSON.stringify(account));
                        }
                        var endpoint = (account.instance && account.instance.apiEndpoint) ||
                                       (account.record && account.record.instance && account.record.instance.apiEndpoint) ||
                                       account.storageKey || '';
                        if (endpoint) {
                            var rawInstance = endpoint.split('::')[0] || endpoint;
                            var match = rawInstance.match(/^https?:\/\/[^\/:]+(?::\d+)?/i);
                            if (match) {
                                var instUrl = match[0];
                                if (window.location.origin !== instUrl && !instUrl.includes('web.fluxer.app')) {
                                    console.log('[Fluxer Bridge] Detected login to remote instance:', instUrl, 'Navigating...');
                                    sendRust('navigateToInstance', { url: instUrl });
                                }
                            }
                        }
                    } catch (e) {
                        console.error('[Fluxer Bridge] Error detecting instance during upsert:', e);
                    }
                }
                return invokeRust('desktopAccountsUpsert', { account: account });
            },
            compareAndSwap: function(request) {
                return invokeRust('desktopAccountsCompareAndSwap', request || {});
            },
            delete: function(storageKey) {
                return invokeRust('desktopAccountsDelete', { storageKey: storageKey });
            },
            import: function(request) {
                var records = (request && request.records) ? request.records : [];
                var promises = records.map(function(acc) {
                    return invokeRust('desktopAccountsUpsert', { account: acc });
                });
                return Promise.all(promises).then(function() {
                    return { imported: records.length, skipped: [] };
                });
            },
            prune: function(request) {
                var keys = (request && request.keysToKeep) ? request.keysToKeep : [];
                return invokeRust('desktopAccountsPrune', { keysToKeep: keys });
            }
        },
        desktopKnownInstances: {
            getAll: function() {
                return invokeRust('knownInstancesGetAll').then(function(rustList) {
                    var merged = Array.isArray(rustList) ? rustList.slice() : [];
                    try {
                        var raw = localStorage.getItem('__fluxer_known_instances');
                        if (raw) {
                            var localList = JSON.parse(raw);
                            if (Array.isArray(localList)) {
                                localList.forEach(function(item) {
                                    if (item && item.instanceKey && !merged.some(function(m) { return m.instanceKey === item.instanceKey; })) {
                                        merged.push(item);
                                    }
                                });
                            }
                        }
                        localStorage.setItem('__fluxer_known_instances', JSON.stringify(merged));
                    } catch (_) {}
                    return merged;
                }).catch(function() {
                    try {
                        var raw = localStorage.getItem('__fluxer_known_instances');
                        if (raw) return JSON.parse(raw);
                    } catch (_) {}
                    return [];
                });
            },
            upsert: function(inst) {
                try {
                    var list = [];
                    var raw = localStorage.getItem('__fluxer_known_instances');
                    if (raw) list = JSON.parse(raw);
                    var filtered = list.filter(function(i) { return i && i.instanceKey !== inst.instanceKey; });
                    filtered.push(inst);
                    localStorage.setItem('__fluxer_known_instances', JSON.stringify(filtered));
                } catch (_) {}
                return invokeRust('knownInstancesUpsert', inst).then(function() {
                    if (inst && inst.domain) {
                        var domain = String(inst.domain).trim();
                        var fullUrl = (domain.startsWith('http://') || domain.startsWith('https://')) ? domain : ('https://' + domain);
                        var targetOrigin = '';
                        try { targetOrigin = new URL(fullUrl).origin; } catch(_) {}
                        if (targetOrigin && window.location.origin !== targetOrigin && !targetOrigin.includes('web.fluxer.app')) {
                            console.log('[Fluxer Bridge] Instance selected in picker, navigating:', targetOrigin);
                            sendRust('navigateToInstance', { url: targetOrigin });
                        }
                    }
                });
            },
            delete: function(key) {
                try {
                    var raw = localStorage.getItem('__fluxer_known_instances');
                    if (raw) {
                        var list = JSON.parse(raw);
                        var filtered = list.filter(function(i) { return i && i.instanceKey !== key; });
                        localStorage.setItem('__fluxer_known_instances', JSON.stringify(filtered));
                    }
                } catch (_) {}
                return invokeRust('knownInstancesDelete', { instanceKey: key }).then(function() {});
            }
        },
        desktopStorage: {
            getStatus: function() {
                return invokeRust('desktopStorageGetStatus');
            },
            load: function(scope) {
                return invokeRust('desktopStorageLoad', { scope: scope }).then(function(res) {
                    return Array.isArray(res) ? res : [];
                });
            },
            get: function(scope, key) {
                return invokeRust('desktopStorageGet', { scope: scope, key: key });
            },
            set: function(scope, key, value) {
                return invokeRust('desktopStorageSet', { scope: scope, key: key, value: String(value) });
            },
            delete: function(scope, key) {
                return invokeRust('desktopStorageDelete', { scope: scope, key: key });
            },
            clearAllForScope: function(scope) {
                return invokeRust('desktopStorageClearAllForScope', { scope: scope });
            },
            clearAllExcept: function(keysToKeep) {
                return invokeRust('desktopStorageClearAllExcept', { keysToKeep: keysToKeep });
            },
            setMany: function(writes) {
                return invokeRust('desktopStorageSetMany', { writes: writes || [] });
            },
            import: function(request) {
                var entries = (request && request.entries) ? request.entries : [];
                var writes = entries.map(function(e) {
                    return { scope: e.scope, key: e.key, value: e.value };
                });
                var markerPromise = (request && request.marker)
                    ? invokeRust('desktopStorageSetMarker', { key: request.marker.key, value: request.marker.value })
                    : Promise.resolve();
                return Promise.all([
                    invokeRust('desktopStorageSetMany', { writes: writes }),
                    markerPromise
                ]).then(function() {
                    return { imported: entries.length, skipped: [] };
                });
            },
            getMarker: function(key) {
                return invokeRust('desktopStorageGetMarker', { key: key });
            },
            setMarker: function(key, value) {
                return invokeRust('desktopStorageSetMarker', { key: key, value: String(value) });
            }
        },
        desktopLegacyHarvest: {
            read: function() { return Promise.resolve(null); },
            markReplanted: function() { return Promise.resolve(); },
            discard: function() { return Promise.resolve(); }
        },
        desktopHandoff: {
            initiate: function() { return Promise.reject(new Error('Handoff unavailable in Rust shell')); },
            status: function() { return Promise.reject(new Error('Handoff unavailable in Rust shell')); }
        },
        otherPlatformTemplate: {
            fetch: function() { return Promise.reject(new Error('OtherPlatformTemplate unavailable')); }
        },
        localAppUpload: {
            subscribe: function() { return function() {}; }
        },
        nativeGatewayTransport: {
            isAvailable: true,
            create: function(request) {
                return invokeRust('nativeGatewayTransportCreate', request || {});
            },
            sendText: function(request) {
                return invokeRust('nativeGatewayTransportSendText', request || {});
            },
            sendBinary: function(request) {
                var payload = request && request.payload;
                var base64 = '';
                if (payload instanceof ArrayBuffer) {
                    var bytes = new Uint8Array(payload);
                    var binary = '';
                    var len = bytes.byteLength;
                    for (var i = 0; i < len; i++) {
                        binary += String.fromCharCode(bytes[i]);
                    }
                    base64 = btoa(binary);
                } else if (typeof payload === 'string') {
                    base64 = payload;
                }
                return invokeRust('nativeGatewayTransportSendBinary', {
                    connectionId: request ? request.connectionId : '',
                    payload: base64
                });
            },
            close: function(request) {
                return invokeRust('nativeGatewayTransportClose', request || {});
            },
            dispose: function(connectionId) {
                return invokeRust('nativeGatewayTransportDispose', { connectionId: connectionId });
            },
            onEvent: function(callback) {
                var id = nextGatewayListenerId++;
                gatewayListeners.set(id, callback);
                return function() {
                    gatewayListeners.delete(id);
                };
            }
        },
        accountEngine: {
            request: function() { return Promise.reject(new Error('AccountEngine unavailable in Rust shell')); },
            on: function() { return function() {}; }
        },

        // 상태 리포트
        reportLastRoute: function(route) {
            sendRust('reportLastRoute', { route: route });
        },
        reportInstanceProductName: function(key, name) {
            sendRust('reportInstanceProductName', { instanceKey: key, productName: name });
        },

        // 뱃지 및 작업표시줄
        setBadgeCount: function() {},
        setUnreadBadge: function() {},
        flashFrame: function() {},
        stopFlashFrame: function() {},
        setTaskbarProgress: function() {},
        setZoomFactor: function() {},

        // 기능 매니페스트
        capabilities: {
            appStore: true,
            gatewaySocket: true,
            audioCapture: true,
            voiceEngineV2: true,
            nativeGatewayTransport: true
        }
    };

    // 4. 화면 공유 시작 및 창 변경 시 2중 선택창 없이 크로미움 시스템 선택창으로 자연스럽게 직결 (통합 파이프라인)
    let isScreenShareStarting = false;

    function isScreenShareGesture(target) {
        if (!target) return false;
        var el = (target.nodeType === 1) ? target : target.parentElement;
        if (!el) return false;

        // 클릭된 요소부터 상위 6단계 부모까지 탐색 (버튼, 메뉴 아이템, div, 아이콘 등 모두 포괄)
        var curr = el;
        var depth = 0;
        while (curr && depth < 6 && curr !== document.body) {
            var flx = (curr.getAttribute('data-flx') || '').toLowerCase();
            var aria = (curr.getAttribute('aria-label') || '').toLowerCase();
            var text = (curr.textContent || '').trim().toLowerCase().replace(/\s+/g, ' ');

            // (1) 화면 공유 중지/종료(Stop) 액션이면 바이패스하지 않음 (정상적인 종료 보장)
            if (flx.includes('stop') || aria.includes('stop') || aria.includes('중지') || aria.includes('종료')) {
                return false;
            }

            // (2) 화면 공유 창/스트림 변경 (Change stream / Source switcher)
            if (flx.includes('change-stream') || flx.includes('change-action') || flx.includes('source-switcher') || flx.includes('active-screen-share-menu')) {
                if (!flx.includes('stop')) {
                    return true;
                }
            }
            if (text.includes('change stream') || text.includes('스트림 변경') || text.includes('화면 변경') || text.includes('창 변경') || text.includes('change window') || text.includes('switch stream')) {
                return true;
            }

            // (3) 화면 공유 시작 버튼 (Screen Share Button)
            if (flx.includes('screen-share') || aria.includes('screen') || aria.includes('화면')) {
                var cls = curr.className || '';
                var isAlreadyOn = (curr.getAttribute('aria-pressed') === 'true') ||
                                  (typeof cls === 'string' && (cls.includes('ScreenShareOn') || cls.includes('screenShareActive')));
                // 이미 켜진 상태의 일반 토글 클릭이 아니면 시작으로 판별
                if (!isAlreadyOn) {
                    return true;
                }
            }

            // (4) 아이콘 기반 판별 (Phosphor MonitorPlay, ChangeStream 아이콘 등)
            if (curr.querySelector && (
                curr.querySelector('svg.ph-monitor-play') ||
                curr.querySelector('[data-flx*="change-stream"]') ||
                curr.querySelector('[data-flx*="monitor-play"]') ||
                curr.querySelector('[data-flx*="media-icon--3"]')
            )) {
                if (!curr.querySelector('svg.ph-stop-circle') && !flx.includes('stop')) {
                    return true;
                }
            }

            curr = curr.parentElement;
            depth++;
        }

        return false;
    }

    window.addEventListener('click', function(e) {
        if (isScreenShareGesture(e.target)) {
            isScreenShareStarting = true;
            setTimeout(function() {
                isScreenShareStarting = false;
            }, 800);
        }
    }, true);

    // 미구현 Electron API 호출 시 예외 없이 안전하게 동작하도록 보호하는 Proxy 방어 계층
    var safeElectronProxy = (typeof Proxy !== 'undefined') ? new Proxy(electronApi, {
        get: function(target, prop, receiver) {
            if (prop in target) {
                return Reflect.get(target, prop, receiver);
            }
            if (typeof prop === 'string') {
                if (prop.startsWith('on') || prop.startsWith('handle')) {
                    return function() { return function() {}; };
                }
                if (prop.startsWith('get') || prop.startsWith('check') || prop.startsWith('request') || prop.startsWith('is') || prop.startsWith('fetch')) {
                    return function() { return Promise.resolve(null); };
                }
            }
            return function() { return Promise.resolve(); };
        }
    }) : electronApi;

    // window.electron 전역 주입 (화면 공유 시작 제스처 시 다이렉트 크로미움 피커 직결)
    try {
        Object.defineProperty(window, 'electron', {
            get: function() {
                if (isScreenShareStarting) {
                    return undefined;
                }
                return safeElectronProxy;
            },
            set: function(v) {},
            configurable: true
        });
    } catch (_) {
        window.electron = safeElectronProxy;
    }

    // 4. 타이틀바 Win32 무지연 네이티브 드래그 가로채기 (Event Capturing)
    window.addEventListener('mousedown', function(e) {
        if (e.button !== 0) return;

        const target = e.target;
        if (!target) return;

        const titlebar = target.closest('.fluxer-native-titlebar, [data-flx-titlebar="true"]');
        if (!titlebar) return;

        const isControl = target.closest('[data-window-control], [data-no-drag], button, a, input, textarea');
        if (isControl) return;

        sendRust('dragWindow');
    }, true);

    // 타이틀바 빈 영역 더블클릭 시 최대화/복원 토글
    window.addEventListener('dblclick', function(e) {
        const target = e.target;
        if (!target) return;

        const titlebar = target.closest('.fluxer-native-titlebar, [data-flx-titlebar="true"]');
        if (!titlebar) return;

        const isControl = target.closest('[data-window-control], [data-no-drag], button, a, input, textarea');
        if (isControl) return;

        sendRust('windowMaximize');
    }, true);

    // DOMContentLoaded 시점에 화면 표출 신호 자동 전송
    document.addEventListener('DOMContentLoaded', function() {
        requestAnimationFrame(function() {
            sendRust('firstContentPainted');
        });
    });

    console.info('[Fluxer Rust Shell] Native Electron Bridge (Phase 2 Full API) injected successfully.');
})();
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization_script_contents() {
        let script = get_initialization_script();
        assert!(script.contains("window.electron ="));
        assert!(script.contains(".fluxer-native-titlebar"));
        assert!(script.contains("dragWindow"));
        assert!(script.contains("windowMinimize"));
        assert!(script.contains("windowMaximize"));
        assert!(script.contains("windowClose"));
        assert!(script.contains("getDesktopInfo"));
        assert!(script.contains("getGpuInfo"));
        assert!(script.contains("getAppMetrics"));
        assert!(script.contains("openExternal"));
        assert!(script.contains("clipboardWriteText"));
        assert!(script.contains("clipboardReadText"));
        assert!(script.contains("desktopRuntimeConfig"));
        assert!(script.contains("desktopAccounts"));
        assert!(script.contains("desktopStorage"));
        assert!(script.contains("initialInput"));
        assert!(script.contains("desktopRuntimeResolve"));
        assert!(script.contains("onSpellcheckStateChanged"));
        assert!(script.contains("onNotificationClick"));
        assert!(script.contains("getInitialDeepLink"));
        assert!(script.contains("onDeepLink"));
        assert!(script.contains("getDesktopSources"));
        assert!(script.contains("navigator.mediaDevices.getDisplayMedia"));
        assert!(script.contains("nativeGatewayTransport"));
        assert!(script.contains("__fluxerNativeGatewayEmit"));
        assert!(script.contains("nativeAudio"));
        assert!(script.contains("nativeAudioGetAvailability"));
        assert!(script.contains("windowSetFullscreen"));
        assert!(script.contains("__fluxerEmitOpenSettings"));
        assert!(script.contains("__fluxerEmitTrayAction"));
        assert!(script.contains("popoutSetAlwaysOnTop"));
        assert!(script.contains("popoutFocus"));
    }
}
