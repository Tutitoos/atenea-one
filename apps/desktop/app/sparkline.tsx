// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
const W = 48;
const H = 14;

/** Tráfico reciente (bajada + subida) sin ejes. Sin puntos o sin tráfico solo hay un guion tenue: nunca una línea inventada. */
export function Sparkline({ points }: { points?: { down: number; up: number }[] }) {
  const totals = (points ?? []).map((p) => p.down + p.up);
  const max = Math.max(0, ...totals);
  if (totals.length < 2 || max <= 0) return <span aria-hidden="true" className="block w-12 text-center text-xs leading-[14px] opacity-40">—</span>;
  const line = totals.map((v, i) => `${(i / (totals.length - 1) * W).toFixed(1)},${(H - 1 - v / max * (H - 2)).toFixed(1)}`).join(' ');
  return (
    <svg aria-hidden="true" data-testid="sparkline" width={W} height={H} viewBox={`0 0 ${W} ${H}`} className="block">
      <polyline points={line} fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}
