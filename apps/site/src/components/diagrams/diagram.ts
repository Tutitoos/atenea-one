// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Piezas de los diagramas de la documentación. Cada una devuelve un fragmento de
// SVG como texto, con colores que son siempre tokens del tema, para que un
// diagrama sea una lista de formas y no un montón de atributos repetidos. Los
// textos se escriben sin `<`, `>` ni `&`: no se escapan.

export type Tone = "plain" | "ok" | "no" | "relay" | "gold";

const stroke: Record<Tone, string> = {
  plain: "var(--color-rule-strong)",
  ok: "var(--color-ok)",
  no: "var(--color-no)",
  relay: "var(--color-accent-200)",
  gold: "var(--color-graph-exact)",
};

const ink: Record<Tone, string> = {
  plain: "var(--color-gray-100)",
  ok: "var(--color-ok)",
  no: "var(--color-no)",
  relay: "var(--color-accent-200)",
  gold: "var(--color-graph-exact)",
};

const line: Record<Tone, string> = {
  plain: "var(--color-gray-400)",
  ok: "var(--color-ok)",
  no: "var(--color-no)",
  relay: "var(--color-accent-200)",
  gold: "var(--color-graph-exact)",
};

const markerId: Record<Tone, string> = {
  plain: "d-arrow",
  ok: "d-arrow-ok",
  no: "d-arrow-no",
  relay: "d-arrow-relay",
  gold: "d-arrow-gold",
};

/** Los `<marker>` que usan las flechas; el marco los pone en cada SVG. */
export const defs = (Object.keys(markerId) as Tone[])
  .map(
    (tone) =>
      `<marker id="${markerId[tone]}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 10 5 0 10z" style="fill: ${line[tone]}"/></marker>`,
  )
  .join("");

/** Un recuadro con título y, opcionalmente, una línea de detalle. */
export function box(
  x: number,
  y: number,
  w: number,
  h: number,
  title: string,
  sub = "",
  tone: Tone = "plain",
): string {
  const cy = y + h / 2;
  const fill = tone === "plain" ? "var(--color-raise)" : "var(--color-panel)";
  return (
    `<rect x="${x}" y="${y}" width="${w}" height="${h}" style="fill: ${fill}; stroke: ${stroke[tone]}; stroke-width: 1"/>` +
    `<text x="${x + w / 2}" y="${sub ? cy - 2 : cy + 5}" text-anchor="middle" font-weight="500" style="fill: ${ink[tone]}">${title}</text>` +
    (sub
      ? `<text x="${x + w / 2}" y="${cy + 14}" text-anchor="middle" font-size="11.5" style="fill: var(--color-gray-400)">${sub}</text>`
      : "")
  );
}

/** Una zona: un contorno discontinuo con su etiqueta arriba a la izquierda. */
export function zone(
  x: number,
  y: number,
  w: number,
  h: number,
  label: string,
  tone: Tone = "plain",
): string {
  return (
    `<rect x="${x}" y="${y}" width="${w}" height="${h}" style="fill: none; stroke: ${stroke[tone]}; stroke-width: 1" stroke-dasharray="4 5"/>` +
    `<text x="${x + 12}" y="${y + 20}" font-size="11.5" style="fill: var(--color-gray-400)">${label}</text>`
  );
}

/** Una flecha. `d` es un trazo SVG; `both` pone punta en los dos extremos. */
export function arrow(
  d: string,
  tone: Tone = "plain",
  options: { dash?: boolean; both?: boolean; width?: number } = {},
): string {
  const { dash = false, both = false, width = 1.5 } = options;
  return (
    `<path d="${d}" fill="none" style="stroke: ${line[tone]}; stroke-width: ${width}"` +
    (dash ? ` stroke-dasharray="${tone === "relay" ? "8 6" : "4 5"}"` : "") +
    ` marker-end="url(#${markerId[tone]})"` +
    (both ? ` marker-start="url(#${markerId[tone]})"` : "") +
    "/>"
  );
}

/** Un texto suelto. */
export function label(
  x: number,
  y: number,
  text: string,
  options: {
    anchor?: "start" | "middle" | "end";
    tone?: Tone;
    size?: number;
  } = {},
): string {
  const { anchor = "middle", tone = "plain", size = 12 } = options;
  const color = tone === "plain" ? "var(--color-gray-300)" : ink[tone];
  return `<text x="${x}" y="${y}" text-anchor="${anchor}" font-size="${size}" style="fill: ${color}">${text}</text>`;
}
