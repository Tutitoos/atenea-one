// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { execFileSync } from 'node:child_process';

// Levanta la topología, los servicios y las claves una sola vez para todas las pruebas (tests/e2e-panel/lab.sh).
export default function globalSetup() {
  const lab = `${process.env.LAB_ROOT ?? '/src'}/tests/e2e-panel/lab.sh`;
  execFileSync('bash', [lab, 'setup'], { stdio: 'inherit', env: process.env });
}
