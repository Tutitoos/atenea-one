// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef, useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { deviceFingerprint, inspectCode, joinNetwork, migrateLegacy, type CodeInfo } from './lib/networks';
import type { Joined } from './lib/wizard';
import { button, Card, CopyButton, primary, Row } from './ui';

/** Banner con la importación de una instalación heredada (un `agent.toml` único) como una red más. */
export function Legacy({ onDone }: { onDone: () => void }) {
  const [asking, setAsking] = useState(false);
  const [name, setName] = useState('');
  const migrate = useMutation({ mutationFn: migrateLegacy, onSuccess: () => { setAsking(false); onDone(); } });
  return (
    <section aria-label="Instalación heredada" className="mb-4 rounded-[10px] bg-warn/15 px-3.5 py-3 text-sm">
      <p>Hay una instalación heredada pendiente de importar: se detectó la configuración de una versión anterior de Atenea One (un único agente). Impórtala para gestionarla desde aquí.</p>
      {!asking && <button type="button" className={`${primary} mt-2`} onClick={() => setAsking(true)}>Importar red existente</button>}
      {asking && (
        <div role="alertdialog" aria-label="Importar red existente" className="mt-2">
          <p>Se parará la instancia actual de este equipo y se importará como una red más. El túnel se cortará unos segundos.</p>
          <label htmlFor="legacy-name" className="mt-2 block">Nombre de la red (opcional: si lo dejas vacío se usa el nombre del servidor)</label>
          <input
            id="legacy-name"
            aria-describedby="legacy-name-rule"
            maxLength={64}
            autoComplete="off"
            spellCheck={false}
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="mt-1 w-64 select-text rounded-lg border border-line bg-bg px-2.5 py-1 text-fg"
          />
          <span id="legacy-name-rule" className="ml-2 text-xs text-mut">Letras, números, espacio, punto, guion y guion bajo.</span>
          <div className="mt-2 flex items-center gap-2">
            <button type="button" className={primary} disabled={migrate.isPending} onClick={() => migrate.mutate(name)}>Importar</button>
            <button type="button" className={button} disabled={migrate.isPending} onClick={() => { setAsking(false); migrate.reset(); }}>Cancelar</button>
            {migrate.isPending && <span role="status" className="text-mut">Esperando la autorización del sistema…</span>}
          </div>
        </div>
      )}
      {migrate.error && <p role="alert" className="mt-2 text-bad">{migrate.error.message}</p>}
    </section>
  );
}

/**
 * Unirse con un código en dos pasos: «Revisar código» (sin privilegios ni efectos) y «Unirme a esta red» (el diálogo de
 * autorización del sistema). Nada se ejecuta con privilegios hasta esa pulsación.
 */
export function Join({ onDone, onJoined }: { onDone: () => void; onJoined?: (joined: Joined) => void }) {
  const [code, setCode] = useState('');
  // La huella que se confirma y el código con el que se une son SIEMPRE los de la misma inspección.
  const [inspected, setInspected] = useState<{ code: string; info: CodeInfo }>();
  const info = inspected?.info;
  // Identifica la última inspección pedida: una respuesta de otra anterior (código editado o cancelada) se descarta.
  const latest = useRef(0);
  const confirmBox = useRef<HTMLDivElement>(null);
  // Al cancelar la confirmación el foco vuelve al campo del código (el control que llevó hasta ella).
  const refocusCode = useRef(false);
  useEffect(() => { if (info) confirmBox.current?.focus(); }, [info]);
  const check = useMutation({ mutationFn: inspectCode });
  const join = useMutation({
    mutationFn: joinNetwork,
    onSuccess: (output) => {
      onDone();
      if (info) onJoined?.({ networkId: info.network_id, name: info.name, server: info.server, hasToken: info.has_token, fingerprint: deviceFingerprint(output) });
    },
  });
  const inspect = (text: string) => {
    const id = ++latest.current;
    check.mutate(text, { onSuccess: (data) => { if (id === latest.current) setInspected({ code: text, info: data }); } });
  };
  const reset = () => { latest.current++; setInspected(undefined); check.reset(); join.reset(); };
  const cancel = () => { refocusCode.current = true; reset(); };

  if (join.isSuccess && info) {
    const fp = deviceFingerprint(join.data);
    return (
      <div role="status" className="rounded-[10px] bg-card p-3.5 text-sm">
        <p>{info.has_token ? `Equipo unido a «${info.name}».` : `Solicitud enviada a «${info.name}»: un administrador debe aprobar este equipo.`}</p>
        {fp && <p className="mt-1">Huella del dispositivo (la compara el administrador): <span className="select-text font-mono">{fp}</span><CopyButton value={fp} label="huella del dispositivo" /></p>}
        <button type="button" className={`${button} mt-2`} onClick={() => { reset(); setCode(''); }}>Unirme a otra red</button>
      </div>
    );
  }

  if (info) {
    return (
      <div ref={confirmBox} tabIndex={-1} role="group" aria-label="Confirmar unión" className="rounded-[10px] bg-card px-3.5 py-3">
        <Card>
          <Row label="Red">{info.name}</Row>
          <Row label="Servidor">{info.server}</Row>
          <Row label="Id de la red">{info.network_id}</Row>
          <Row label="Huella de la raíz">{info.root_fingerprint}<CopyButton value={info.root_fingerprint} label="huella de la raíz" /></Row>
          <Row label="Tipo de código">{info.has_token ? 'Con token de un solo uso' : 'De solicitud: un administrador debe aprobar este equipo'}</Row>
          {info.connectivity && <Row label="Conectividad">{`STUN ${info.connectivity.stun_host}; CA del relay: ${info.connectivity.relay_ca_sha256 ? `SHA-256 ${info.connectivity.relay_ca_sha256.slice(0, 16)}…` : 'pública (el código no lleva hash)'}`}</Row>}
        </Card>
        <p className="mt-3 text-sm">Compara el nombre, el servidor y la huella con los que te dio quien te pasó el código, <strong>por otro canal</strong>. Una red puede instalar rutas privadas y DNS para dominios concretos en este equipo.</p>
        <div className="mt-3 flex items-center gap-2">
          <button type="button" className={primary} disabled={join.isPending} onClick={() => join.mutate(inspected!.code)}>Unirme a esta red</button>
          <button type="button" className={button} disabled={join.isPending} onClick={cancel}>Cancelar</button>
          {join.isPending && <span role="status" className="text-sm text-mut">Esperando la autorización del sistema…</span>}
        </div>
        {join.error && <p role="alert" className="mt-2 text-sm text-bad">{join.error.message}</p>}
      </div>
    );
  }

  return (
    <form className="rounded-[10px] bg-card px-3.5 py-3" onSubmit={(e) => { e.preventDefault(); if (code.trim()) inspect(code); }}>
      <label htmlFor="join-code" className="block text-sm">Código de red</label>
      <textarea
        id="join-code"
        ref={(el) => { if (el && refocusCode.current) { refocusCode.current = false; el.focus(); } }}
        rows={3}
        spellCheck={false}
        autoComplete="off"
        placeholder="ATENEA1-…"
        value={code}
        onChange={(e) => { setCode(e.target.value); latest.current++; check.reset(); }}
        className="mt-1 w-full select-text rounded-lg border border-line bg-bg px-2.5 py-1.5 font-mono text-xs text-fg"
      />
      <div className="mt-2 flex items-center gap-2">
        <button type="submit" className={primary} disabled={!code.trim() || check.isPending}>Revisar código</button>
        <span className="text-xs text-mut">Todavía no se cambia nada: primero verás a qué red te unes.</span>
      </div>
      {check.error && <p role="alert" className="mt-2 text-sm text-bad">{check.error.message}</p>}
    </form>
  );
}
