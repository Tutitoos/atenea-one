// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

const KEY = 'atenea.section';
/** Secciones que se recuerdan; la primera es la de por defecto. No incluye la bienvenida ni dispositivos concretos. */
const SECTIONS = ['/devices', '/servers', '/networks', '/diagnostics'];

/** Última sección visitada, validada: un valor guardado desconocido vale como la de por defecto. */
export const readSection = () => SECTIONS.find((s) => s === localStorage.getItem(KEY)) ?? SECTIONS[0];

/** Guarda la sección de una ruta (`/devices/abc` → `/devices`); fuera de las secciones recordables no hace nada. */
export function saveSection(pathname: string) {
  const section = `/${pathname.split('/')[1]}`;
  if (SECTIONS.includes(section)) localStorage.setItem(KEY, section);
}
