// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
const $ = (id) => document.getElementById(id);
const NAMES = 'atenea-names';
const names = JSON.parse(localStorage.getItem(NAMES) || '{}');
const prev = new Map();
const bytes = (n) => { const u = ['B','KiB','MiB','GiB','TiB']; let i = 0; while (n >= 1024 && i < 4) { n /= 1024; i++; } return n.toFixed(i ? 1 : 0) + ' ' + u[i]; };
const rate = (b) => b < 1 ? '0 B/s' : bytes(b) + '/s';
const ago = (s) => s == null ? '—' : s < 120 ? s + ' s' : s < 7200 ? Math.round(s / 60) + ' min' : Math.round(s / 3600) + ' h';
function el(tag, text, cls) { const e = document.createElement(tag); if (text != null) e.textContent = text; if (cls) e.className = cls; return e; }
function field(label, value) { const d = el('div'); d.append(el('div', label, 'k'), el('div', value, 'v')); return d; }

function render(s, dt) {
  const alerts = [];
  const cfg = s.config || {};
  const left = cfg.valid_until_unix ? cfg.valid_until_unix - Date.now() / 1000 : null;
  if (!cfg.lease_active) alerts.push('Sin lease activo: el agente no cursa tráfico hasta renovar la configuración.');
  if (cfg.last_error) alerts.push('Error de configuración: ' + cfg.last_error);
  if (s.sync && s.sync.last_error) alerts.push('Sincronización con el control plane: ' + s.sync.last_error);
  if (s.coordinator && s.coordinator.last_error) alerts.push('Coordinator: ' + s.coordinator.last_error);
  $('alerts').replaceChildren(...alerts.map((a) => el('div', a, 'card alert')));
  $('dot').style.background = alerts.length ? 'var(--bad)' : 'var(--ok)';

  const nat = s.nat || {};
  $('self').replaceChildren(
    field('Mis direcciones', (s.addrs || []).join(', ') || '—'),
    field('Clave pública', (s.public_key || '').slice(0, 16) + '…'),
    field('Lease', left == null ? '—' : left > 0 ? 'válido ' + ago(Math.round(left)) : 'caducado'),
    field('Configuración', 'v' + (cfg.version ?? '?')),
    field('NAT', (nat.mapping || '—') + (nat.reflexive ? ' · ' + nat.reflexive : '')),
    field('Control plane', s.sync && s.sync.last_ok_unix ? 'ok hace ' + ago(Math.round(Date.now() / 1000 - s.sync.last_ok_unix)) : '—'),
    field('Coordinator', s.coordinator && s.coordinator.last_announce_ok_unix ? 'ok hace ' + ago(Math.round(Date.now() / 1000 - s.coordinator.last_announce_ok_unix)) : '—'),
    field('Relays asignados', String((s.relay && s.relay.allocations) ?? 0)),
  );

  const rows = (s.peers || []).map((p) => {
    const key = p.public_key, old = prev.get(key);
    const down = old && dt > 0 ? Math.max(0, p.rx_bytes - old.rx) / dt : null;
    const up = old && dt > 0 ? Math.max(0, p.tx_bytes - old.tx) / dt : null;
    prev.set(key, { rx: p.rx_bytes, tx: p.tx_bytes });
    const tr = el('tr');
    const who = el('td');
    const nm = el('span', names[key] || (p.addrs || [])[0] || key.slice(0, 8), 'name');
    nm.title = 'Cambiar nombre';
    nm.onclick = () => { const v = prompt('Nombre para este dispositivo (vacío = ninguno)', names[key] || ''); if (v === null) return; if (v.trim()) names[key] = v.trim(); else delete names[key]; localStorage.setItem(NAMES, JSON.stringify(names)); nm.textContent = names[key] || (p.addrs || [])[0] || key.slice(0, 8); };
    who.append(nm, el('div', [...(p.addrs || []), key.slice(0, 8)].join(' · '), 'sub'));
    const path = el('td'); path.append(el('span', p.path, 'tag ' + p.path));
    const alt = (p.paths || []).filter((x) => x.ready && x.kind !== p.path).map((x) => x.kind);
    if (alt.length) path.append(el('div', 'también: ' + alt.join(', '), 'sub'));
    const used = (p.paths || []).find((x) => x.kind === p.path);
    tr.append(who, path,
      el('td', used && used.rtt_ms != null ? used.rtt_ms + ' ms' : '—'),
      el('td', bytes(p.rx_bytes) + ' / ' + bytes(p.tx_bytes) + (down == null ? '' : '  (' + rate(down) + ' / ' + rate(up) + ')')),
      el('td', p.session ? 'hace ' + ago(p.last_handshake_secs_ago) : 'sin sesión', p.session ? '' : 'mut'),
      el('td', p.endpoint || '—'));
    return tr;
  });
  $('peers').replaceChildren(...rows);
  $('empty').hidden = rows.length > 0;
}

let last = 0;
async function tick() {
  try {
    const now = Date.now() / 1000;
    render(JSON.parse(await window.__TAURI__.core.invoke('status')), last ? now - last : 0);
    last = now;
    $('upd').textContent = 'actualizado ' + new Date().toLocaleTimeString();
  } catch (e) {
    $('dot').style.background = 'var(--bad)';
    $('alerts').replaceChildren(el('div', 'No se puede leer el estado del agente: ' + (e.message || e), 'card alert'));
  }
}
tick(); setInterval(tick, 2000);
