// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState, type ReactNode } from 'react';
import { Link } from 'react-router';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { openPanel, readClient, readLegacy, readNetworks, stateText } from '../lib/networks';
import { installCommand, phaseOf, trayHint, type Joined, type Phase } from '../lib/wizard';
import { Join, Legacy } from '../join';
import { Autostart } from '../preferences';
import { button, CopyButton, primary } from '../ui';

const steps: { phase: Phase[]; label: string }[] = [
  { phase: ['cargando', 'error', 'sistema', 'cliente'], label: 'Sistema y cliente' },
  { phase: ['heredada'], label: 'Instalación heredada' },
  { phase: ['unirse'], label: 'Unirse a una red' },
  { phase: ['aprobacion'], label: 'Aprobación' },
  { phase: ['listo'], label: 'Listo' },
];

function Step({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title} className="rounded-[10px] bg-card px-3.5 py-3 text-sm">
      <h2 className="mb-2 text-[15px] font-semibold">{title}</h2>
      {children}
    </section>
  );
}

const noJoinText = 'En Windows todavía no se puede unir ni gestionar redes desde la aplicación; puedes ver el estado de la red ya instalada.';

function Command({ value }: { value: string }) {
  return <p className="mt-2 flex items-center rounded-lg border border-line bg-bg px-2.5 py-1.5"><code className="select-text break-all font-mono text-xs">{value}</code><CopyButton value={value} label="orden de instalación" icon="terminal" /></p>;
}

/**
 * Asistente de primera ejecución (ADR-021). Es una guía sobre las operaciones que la aplicación ya tiene: no añade
 * ninguna operación con privilegios, no instala nada y no abre enlaces. Con privilegios solo ocurre lo que el usuario
 * confirma con «Unirme a esta red» o «Importar» (cada uno pide la autorización del sistema).
 */
export default function Welcome() {
  const queries = useQueryClient();
  const client = useQuery({ queryKey: ['client'], queryFn: readClient });
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000, enabled: client.data?.supported === true && client.data.installed });
  const legacy = useQuery({ queryKey: ['legacy'], queryFn: readLegacy, enabled: client.data?.supported === true && client.data.installed });
  const [joined, setJoined] = useState<Joined>();
  const [skippedLegacy, setSkippedLegacy] = useState(false);
  const [skippedApproval, setSkippedApproval] = useState(false);
  const [rechecked, setRechecked] = useState(false);
  const panel = useMutation({ mutationFn: (id: string) => openPanel(id) });
  const refresh = () => { for (const key of ['networks', 'agent', 'legacy']) void queries.invalidateQueries({ queryKey: [key] }); };

  const phase = phaseOf({
    client: client.data,
    networks: networks.data,
    legacy: legacy.data,
    failed: !!(client.error ?? networks.error),
    joined,
    skippedLegacy,
    skippedApproval,
  });
  const os = client.data?.os ?? '';
  const joinedNet = networks.data?.find((n) => n.network_id === joined?.networkId);

  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h1 className="mb-1 text-xl font-semibold">Primeros pasos</h1>
      <p className="mb-3 text-sm text-mut">Te guía por lo que ya hace la aplicación. Nada se cambia en este equipo sin que lo confirmes y sin la autorización del sistema.</p>
      <ol aria-label="Pasos del asistente" className="mb-4 flex flex-wrap gap-x-4 gap-y-1 text-xs text-mut">
        {steps.filter((s) => s.phase[0] !== 'heredada' || legacy.data || phase === 'heredada').map((s, i) => {
          const current = s.phase.includes(phase);
          return <li key={s.label} aria-current={current ? 'step' : undefined} className={current ? 'font-semibold text-fg' : ''}>{i + 1}. {s.label}</li>;
        })}
      </ol>

      {phase === 'cargando' && <p role="status" className="text-mut">Comprobando…</p>}

      {phase === 'error' && (
        <Step title="No se pudo comprobar el estado">
          <p role="alert" className="text-bad">{(client.error ?? networks.error)?.message}</p>
          <button type="button" className={`${button} mt-2`} onClick={() => { void client.refetch(); void networks.refetch(); }}>Reintentar</button>
        </Step>
      )}

      {phase === 'sistema' && (
        <Step title={os === 'windows' ? 'Sin red que ver en este equipo' : 'Este sistema todavía no está soportado'}>
          <p>{os === 'windows' ? noJoinText : `La aplicación no puede gestionar redes en este sistema (${os}). Puedes seguir viendo el estado de lo que ya esté unido.`}</p>
        </Step>
      )}

      {phase === 'cliente' && (
        <Step title="Instala el cliente de Atenea One">
          {os === 'windows'
            ? <p>No se detecta el agente de este equipo. Instala y arranca el servicio AteneaAgent siguiendo la guía de instalación de Windows y vuelve aquí. La aplicación no lo instala ni lo descarga.</p>
            : <p>Falta el programa que une este equipo a las redes (<code>atenea</code>). La aplicación no lo instala ni lo descarga: instálalo tú desde el paquete de la misma versión y vuelve aquí.</p>}
          {installCommand(os) ? <><p className="mt-2">Desde la carpeta donde descargaste el paquete:</p><Command value={installCommand(os) ?? ''} /></> : null}
          <div className="mt-3 flex items-center gap-2">
            <button type="button" className={primary} disabled={client.isFetching} onClick={() => { setRechecked(false); void client.refetch().then(() => setRechecked(true)); }}>Comprobar de nuevo</button>
            {client.isFetching && <span role="status" className="text-mut">Comprobando…</span>}
          </div>
          {rechecked && !client.isFetching && !client.data?.installed && <p role="status" className="mt-2 text-mut">Todavía no se detecta el cliente.</p>}
        </Step>
      )}

      {phase === 'heredada' && (
        <Step title="Hay una instalación anterior">
          <p>Este equipo ya tiene una instalación heredada. Puedes importarla como una red más antes de unirte a otra.</p>
          <div className="mt-2"><Legacy onDone={refresh} /></div>
          <button type="button" className={button} onClick={() => setSkippedLegacy(true)}>Unirme a otra red en su lugar</button>
        </Step>
      )}

      {phase === 'unirse' && (
        <Step title="Únete a una red">
          <p className="mb-2">Pega el código que te pasó quien administra la red. Primero verás a qué red te unes y, al confirmar, el sistema te pedirá autorización.</p>
          <Join onDone={refresh} onJoined={setJoined} />
        </Step>
      )}

      {phase === 'aprobacion' && joined && (
        <Step title={`Esperando la aprobación de «${joined.name}»`}>
          <p>Un administrador debe aprobar este equipo. Dile esta huella del dispositivo para que la compare por otro canal:</p>
          {joined.fingerprint
            ? <p className="mt-2"><span className="select-text font-mono">{joined.fingerprint}</span><CopyButton value={joined.fingerprint} label="huella del dispositivo" /></p>
            : <p className="mt-2 text-mut">El cliente no ha devuelto la huella; consúltala con <code>atenea network list</code>.</p>}
          <p role="status" className="mt-2 text-mut">Estado: {joinedNet ? stateText[joinedNet.state] : 'enviada la solicitud'}. Se actualiza solo.</p>
          <div className="mt-3 flex items-center gap-2">
            <button type="button" className={button} disabled={panel.isPending} onClick={() => panel.mutate(joined.networkId)}>Abrir en el panel</button>
            <button type="button" className={button} onClick={() => setSkippedApproval(true)}>Continuar sin esperar</button>
          </div>
          {panel.error && <p role="alert" className="mt-2 text-bad">{panel.error.message}</p>}
        </Step>
      )}

      {phase === 'listo' && (
        <Step title={joined ? 'Listo' : 'Ya tienes redes unidas'}>
          <p>{joined ? `«${joined.name}» está en este equipo.` : client.data?.can_join ? 'Este equipo ya está unido a una o más redes.' : 'Puedes ver el estado de la red ya instalada en este equipo.'} {trayHint(os)}</p>
          {!client.data?.can_join && <p className="mt-2 text-mut">{noJoinText}</p>}
          <Autostart />
          <div className="mt-3 flex items-center gap-2">
            <Link to="/devices" className={primary}>Ver dispositivos</Link>
            {!joined && client.data?.can_join && <Link to="/networks" className={button}>Unirme a otra red</Link>}
          </div>
        </Step>
      )}
    </main>
  );
}
