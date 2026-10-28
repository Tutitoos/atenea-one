// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { Network, NetState } from './networks';

/** URL elimina puertos estándar; no se mezclan subdominios ni puertos no estándar. */
export function serverHost(url: string): string {
  return new URL(url.includes('://') ? url : `https://${url}`).host.toLowerCase();
}

export type Server = { host: string; networks: Network[]; state: NetState };
const severity: Record<NetState, number> = { conectada: 0, detenida: 1, iniciada_sin_configuracion: 2, desconocida: 3, conflicto: 4 };

/** Peor estado de las redes; detenida sigue siendo neutra, no un fallo. */
export function groupServers(networks: Network[]): Server[] {
  const groups = new Map<string, Server>();
  for (const net of networks) {
    const host = serverHost(net.url);
    const group = groups.get(host);
    if (!group) groups.set(host, { host, networks: [net], state: net.state });
    else {
      group.networks.push(net);
      if (severity[net.state] > severity[group.state]) group.state = net.state;
    }
  }
  return [...groups.values()];
}
