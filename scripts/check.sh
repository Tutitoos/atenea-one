#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Mismas comprobaciones que CI.
set -euo pipefail
cd "$(dirname "$0")/.."

python3 scripts/check-headers.py

# Cargo falla en un workspace virtual sin miembros (fase 1); se omite hasta que exista el primero.
if cargo metadata --no-deps --format-version 1 --locked | grep -q '"packages":\[\]'; then
  echo "cargo: workspace sin miembros, omitido"
else
  cargo fmt --all --check
  # Workspace propio (Tauri): solo el formato es comprobable sin WebKitGTK; el resto va en los jobs de macOS y Windows.
  cargo fmt --manifest-path apps/desktop/Cargo.toml --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo test --workspace --locked
fi

pnpm install --frozen-lockfile
pnpm -r --if-present run lint
pnpm -r --if-present run typecheck
pnpm -r --if-present run test
pnpm --filter @atenea/web run build
pnpm --filter @atenea/desktop run build
