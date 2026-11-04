// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { ClientInfo, Network } from './networks';

/** Dónde está el asistente (ADR-021). Se deriva siempre del estado del sistema: no hay nada guardado que se desfase. */
export type Phase = 'cargando' | 'error' | 'sistema' | 'cliente' | 'heredada' | 'unirse' | 'aprobacion' | 'listo';

export type Joined = { networkId: string; name: string; server: string; hasToken: boolean; fingerprint?: string };

export type WizardInput = {
  client?: ClientInfo;
  networks?: Network[];
  /** Hay una instalación heredada pendiente de importar. */
  legacy?: boolean;
  /** Fallo al consultar el cliente o las redes: no se sabe en qué paso se está. */
  failed?: boolean;
  /** Lo que acaba de unirse en esta sesión del asistente. */
  joined?: Joined;
  skippedLegacy?: boolean;
  skippedApproval?: boolean;
};

export function phaseOf(i: WizardInput): Phase {
  if (i.failed) return 'error';
  if (!i.client) return 'cargando';
  if (!i.client.supported) return 'sistema';
  if (!i.client.installed) return 'cliente';
  if (!i.networks) return 'cargando';
  if (i.joined) {
    const net = i.networks.find((n) => n.network_id === i.joined?.networkId);
    // Un código con token entra aprobado; uno de solicitud espera a que un administrador apruebe este equipo.
    const pending = !i.joined.hasToken && (!net || net.state === 'iniciada_sin_configuracion');
    return pending && !i.skippedApproval ? 'aprobacion' : 'listo';
  }
  if (i.networks.length > 0) return 'listo';
  if (i.legacy && !i.skippedLegacy) return 'heredada';
  return 'unirse';
}

/** Orden de instalación del cliente por sistema, para copiar (nunca se ejecuta desde la aplicación). Sin enlaces ni descargas. */
export function installCommand(os: string): string | undefined {
  if (os === 'darwin') return 'sudo installer -pkg atenea-cli-*-macos-*.pkg -target /';
  if (os === 'linux') return 'sudo apt install ./atenea-cli-*-linux-*.deb';
  return undefined;
}

/** Dónde vive el icono de la aplicación una vez cerrada la ventana. */
export function trayHint(os: string): string {
  if (os === 'darwin') return 'Atenea One queda en la barra de menús (arriba a la derecha): cerrar la ventana no la cierra.';
  return 'Atenea One queda en la bandeja del sistema: cerrar la ventana no la cierra.';
}
