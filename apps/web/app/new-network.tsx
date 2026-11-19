// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { Button, Field } from './ui';
import { z } from 'zod';
import { nameSchema } from './domain';

/** Bloques IPv4 que se ofrecen al crear una red (como el selector sencillo de ZeroTier): nadie escribe un CIDR a mano. */
export const IPV4_BLOCKS = [
  '10.147.17.0/24', '10.147.18.0/24', '10.147.19.0/24', '10.147.20.0/24',
  '10.144.0.0/16', '10.241.0.0/16', '10.242.0.0/16', '10.243.0.0/16',
  '10.244.0.0/16', '172.22.0.0/16', '172.23.0.0/16', '172.24.0.0/16',
  '172.25.0.0/16', '172.26.0.0/16', '172.27.0.0/16', '172.28.0.0/16',
  '172.29.0.0/16', '172.30.0.0/16', '192.168.191.0/24', '192.168.192.0/24',
  '192.168.193.0/24', '192.168.194.0/24', '192.168.195.0/24', '192.168.196.0/24',
] as const;

const label = (cidr: string) => cidr.replace(/\/24$/, '.*').replace(/\.0\.0\/16$/, '.*.*');

const ipv4 = z.ipv4();
const order = (address: string) => address.split('.').reduce((n, part) => n * 256 + Number(part), 0);

export type NewNetworkBody = { name: string; ipv4_cidr?: string; ipv4_pool?: { start: string; end: string }; ipv6?: false };

/**
 * Nombre y direcciones, como el selector de ZeroTier. «Fácil»: «Automático» (el servidor elige un bloque libre) o uno de la lista.
 * «Avanzado»: inicio y fin del rango del que reciben dirección los equipos; la red es el bloque más pequeño que lo contiene.
 * IPv6 se puede desactivar: la red queda solo IPv4.
 */
export function NewNetworkForm({ create }: { create: (body: NewNetworkBody) => Promise<unknown> }) {
  const [name, setName] = useState('');
  const [block, setBlock] = useState<string>();
  const [mode, setMode] = useState<'facil' | 'avanzado'>('facil');
  const [range, setRange] = useState({ start: '', end: '' });
  const [ipv6, setIpv6] = useState(true);
  const [message, setMessage] = useState('');
  const [pending, setPending] = useState(false);
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); setMessage(''); setPending(true);
    try {
      const body: NewNetworkBody = { name: nameSchema.parse(name), ...(ipv6 ? {} : { ipv6: false }) };
      const [start, end] = [range.start.trim(), range.end.trim()];
      if (mode === 'facil') { if (block) body.ipv4_cidr = block; }
      else if (start || end) {
        if (!ipv4.safeParse(start).success || !ipv4.safeParse(end).success) throw new Error('Escribe el inicio y el fin como direcciones IPv4, por ejemplo 10.244.0.1 y 10.244.0.254.');
        if (order(start) > order(end)) throw new Error('El inicio del rango debe ser menor o igual que el fin.');
        body.ipv4_pool = { start, end };
      }
      await create(body);
      setMessage('Acción completada.');
    } catch (error) { setMessage(error instanceof Error ? error.message : 'No se pudo completar la acción.'); }
    finally { setPending(false); }
  };
  return <form className="action-form" onSubmit={event => void submit(event)}>
    <div className="fields">
      <Field label="Nombre">{props => <input {...props} type="text" required value={name} onChange={e => setName(e.target.value)}/>}</Field>
      <div role="tablist" aria-label="Modo de direcciones" className="mode-tabs">
        <button type="button" role="tab" aria-selected={mode === 'facil'} onClick={() => setMode('facil')}>Fácil</button>
        <button type="button" role="tab" aria-selected={mode === 'avanzado'} onClick={() => setMode('avanzado')}>Avanzado</button>
      </div>
      {mode === 'facil' ? <div role="radiogroup" aria-label="Direcciones IPv4">
        <p className="muted">Direcciones de los equipos</p>
        <button type="button" role="radio" aria-checked={!block} className="block-auto" onClick={() => setBlock(undefined)}>
          Automático (recomendado)
        </button>
        <div className="blocks">{IPV4_BLOCKS.map(cidr =>
          <button key={cidr} type="button" role="radio" aria-checked={block === cidr} title={cidr}
            onClick={() => setBlock(cidr)}>{label(cidr)}</button>)}
        </div>
      </div> : <div className="range">
        <Field label="Inicio del rango" help="Vacío: el servidor elige un bloque libre.">{props =>
          <input {...props} type="text" inputMode="decimal" placeholder="10.244.0.1" value={range.start}
            onChange={e => setRange({ ...range, start: e.target.value })}/>}</Field>
        <Field label="Fin del rango">{props =>
          <input {...props} type="text" inputMode="decimal" placeholder="10.244.0.254" value={range.end}
            onChange={e => setRange({ ...range, end: e.target.value })}/>}</Field>
      </div>}
      <label className="check"><input type="checkbox" checked={ipv6} onChange={e => setIpv6(e.target.checked)}/> Asignar también direcciones IPv6</label>
    </div>
    <Button type="submit" variant="primary" loading={pending}>Crear red</Button><p role="status">{message}</p>
  </form>;
}
