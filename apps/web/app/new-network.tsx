// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { RadioGroup, Tabs } from 'radix-ui';
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

const label = (cidr: string) => cidr.replace(/\.0\/24$/, '.*').replace(/\.0\.0\/16$/, '.*.*');
// El elegido lleva las clases del botón primario: su color lo decide la piel del panel, no este fichero.
const chosen = (on: boolean) => on ? 'button primary' : undefined;

const ipv4 = z.ipv4();
const order = (address: string) => address.split('.').reduce((n, part) => n * 256 + Number(part), 0);
/** Primera y última dirección de un CIDR IPv4. */
const span = (cidr: string): [number, number] => {
  const [address, bits] = cidr.split('/');
  const size = 2 ** (32 - Number(bits));
  const first = Math.floor(order(address) / size) * size;
  return [first, first + size - 1];
};
const overlaps = (a: [number, number], b: [number, number]) => a[0] <= b[1] && b[0] <= a[1];

export type NewNetworkBody = { name: string; ipv4_cidr?: string; ipv4_pool?: { start: string; end: string }; ipv6?: false };

/**
 * Nombre y direcciones, como el selector de ZeroTier. «Fácil»: «Automático» (el servidor elige un bloque libre) o uno de la lista.
 * «Avanzado»: inicio y fin del rango del que reciben dirección los equipos; la red es el bloque más pequeño que lo contiene.
 * IPv6 se puede desactivar: la red queda solo IPv4.
 */
export function NewNetworkForm({ create, taken = [] }: { create: (body: NewNetworkBody) => Promise<unknown>; taken?: readonly string[] }) {
  const used = taken.map(span);
  const inUse = (cidr: string) => used.some(other => overlaps(span(cidr), other));
  const [name, setName] = useState('');
  const [block, setBlock] = useState('');
  const [mode, setMode] = useState('facil');
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
        if (used.some(other => overlaps([order(start), order(end)], other))) throw new Error('Ese rango se solapa con otra red de la organización. Elige otro.');
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
      <Tabs.Root value={mode} onValueChange={setMode}>
        <Tabs.List className="mode-tabs" aria-label="Modo de direcciones">
          <Tabs.Trigger value="facil" className={chosen(mode === 'facil')}>Fácil</Tabs.Trigger>
          <Tabs.Trigger value="avanzado" className={chosen(mode === 'avanzado')}>Avanzado</Tabs.Trigger>
        </Tabs.List>
        <Tabs.Content value="facil">
          <p className="muted">Direcciones de los equipos</p>
          <RadioGroup.Root aria-label="Direcciones IPv4" value={block} onValueChange={setBlock}>
            <RadioGroup.Item value="" className={`block-auto ${chosen(!block) ?? ''}`}>Automático (recomendado)</RadioGroup.Item>
            <div className="blocks">{IPV4_BLOCKS.map(cidr => {
              const busy = inUse(cidr);
              return <RadioGroup.Item key={cidr} value={cidr} disabled={busy} title={busy ? `${cidr}: ya lo usa otra red de la organización` : cidr}
                className={chosen(block === cidr)}>{label(cidr)}</RadioGroup.Item>;
            })}
            </div>
          </RadioGroup.Root>
        </Tabs.Content>
        <Tabs.Content value="avanzado">
          <div className="range">
            <Field label="Inicio del rango">{props => <input {...props} type="text" placeholder="10.244.0.1" value={range.start}
              onChange={e => setRange({ ...range, start: e.target.value })}/>}</Field>
            <Field label="Fin del rango">{props => <input {...props} type="text" placeholder="10.244.0.254" value={range.end}
              onChange={e => setRange({ ...range, end: e.target.value })}/>}</Field>
          </div>
          <p className="muted field-help">Vacíos: el servidor elige un bloque libre.</p>
        </Tabs.Content>
      </Tabs.Root>
      <Field label="Asignar también direcciones IPv6">{props =>
        <input {...props} type="checkbox" checked={ipv6} onChange={e => setIpv6(e.target.checked)}/>}</Field>
    </div>
    <Button type="submit" variant="primary" loading={pending}>Crear red</Button><p role="status">{message}</p>
  </form>;
}
