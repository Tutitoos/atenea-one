// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Spike de ADR-019 en el webview real. S1: la CSP bloquea lo que debe y deja pasar al propio puente.
// S2: qué globales inyecta el runtime y qué métodos Go puede invocar la página.
import * as rt from './runtime.js';

const out = { env: { userAgent: navigator.userAgent, href: location.href }, s1: {}, s2: {} };
const violations = [];
document.addEventListener('securitypolicyviolation', (e) => {
  violations.push(`${e.violatedDirective} ${String(e.blockedURI).slice(0, 80)}`);
});

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const withTimeout = (p, ms) => Promise.race([p, new Promise((r) => setTimeout(() => r('timeout'), ms))]);

// Cada sonda anota el resultado y si el navegador emitió una violación de CSP de esa directiva. Con una
// CSP que no se aplicase, una petición a un sitio real podría fallar por otras causas (404, CORS): la
// violación es la prueba de que fue la CSP.
async function probe(name, directive, fn) {
  const before = violations.length;
  let outcome;
  try {
    outcome = await withTimeout(fn(), 6000);
  } catch (e) {
    outcome = 'throw: ' + String(e).slice(0, 80);
  }
  await sleep(150);
  out.s1[name] = { outcome, violation: violations.slice(before).some((v) => v.startsWith(directive)) };
}

async function s1() {
  await probe('remoteFetch', 'connect-src', () =>
    fetch('https://example.com/', { mode: 'no-cors' }).then(() => 'resolved', () => 'rejected'));
  await probe('remoteWebSocket', 'connect-src', () => new Promise((res) => {
    try {
      const w = new WebSocket('wss://example.com/');
      w.onopen = () => { w.close(); res('open'); };
      w.onerror = () => res('error');
    } catch (e) { res('throw: ' + String(e).slice(0, 60)); }
  }));
  await probe('remoteImage', 'img-src', () => new Promise((res) => {
    const i = new Image();
    i.onload = () => res('load');
    i.onerror = () => res('error');
    i.src = 'https://example.com/favicon.ico?' + Date.now();
  }));
  await probe('remoteScript', 'script-src', () => new Promise((res) => {
    const s = document.createElement('script');
    s.src = 'https://example.com/spike.js';
    s.onload = () => res('load');
    s.onerror = () => res('error');
    document.head.appendChild(s);
  }));
  await probe('inlineScript', 'script-src', async () => {
    window.__inline = false;
    const s = document.createElement('script');
    s.textContent = 'window.__inline = true';
    document.head.appendChild(s);
    await sleep(100);
    return window.__inline ? 'ran' : 'not-run';
  });
  await probe('eval', 'script-src', async () => {
    try { return 'ran: ' + new Function('return 1+1')(); } catch (e) { return 'blocked: ' + String(e).slice(0, 60); }
  });
  try {
    const r = await fetch(location.href);
    out.s1.sameOriginFetchOk = r.ok;
    out.s1.cspHeader = r.headers.get('content-security-policy');
  } catch (e) {
    out.s1.sameOriginFetchOk = false;
    out.s1.sameOriginError = String(e).slice(0, 80);
  }
  out.s1.violationsAll = violations.slice(0, 40);
}

async function callByName(name) {
  try {
    const v = await rt.Call.ByName(name);
    return { ok: true, value: String(v).slice(0, 40) };
  } catch (e) {
    return { ok: false, error: String(e).slice(0, 160) };
  }
}

async function s2() {
  let baseline = new Set();
  try {
    const f = document.createElement('iframe');
    document.body.appendChild(f);
    baseline = new Set(Object.getOwnPropertyNames(f.contentWindow));
    f.remove();
  } catch (e) {
    out.s2.iframeError = String(e).slice(0, 80);
  }
  out.s2.baselineSize = baseline.size;
  out.s2.extraGlobals = Object.getOwnPropertyNames(window).filter((k) => !baseline.has(k)).slice(0, 80);
  out.s2.native = {
    webkitMessageHandlers: !!(window.webkit && window.webkit.messageHandlers),
    chromeWebview: !!(window.chrome && window.chrome.webview),
    tauriGlobal: typeof window.__TAURI__,
    wailsGlobal: typeof window.wails,
  };
  out.s2.runtimeExports = Object.keys(rt).slice(0, 60);
  out.s2.calls = {};
  for (const m of ['main.SpikeService.Ping', 'main.SpikeService.NotIntended', 'main.SpikeService.private',
    'main.SpikeService.Nope', 'main.Hidden.Do']) {
    out.s2.calls[m] = await callByName(m);
  }
}

async function main() {
  for (const [name, fn] of [['s1', s1], ['s2', s2]]) {
    try { await fn(); } catch (e) { out[name].fatal = String(e).slice(0, 200); }
  }
  const payload = JSON.stringify(out);
  document.getElementById('out').textContent = payload.slice(0, 2000);
  try {
    await rt.Call.ByName('main.SpikeService.Report', payload);
    return;
  } catch (e) {
    out.reportViaBindingError = String(e).slice(0, 200);
  }
  // Respaldo: el puente falló; el servidor de recursos del spike acepta el informe por HTTP del mismo origen.
  await fetch('/spike-report', { method: 'POST', body: JSON.stringify(out) });
}

main();
