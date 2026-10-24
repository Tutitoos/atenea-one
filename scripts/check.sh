#!/usr/bin/env bash
# Mismas comprobaciones que CI.
set -euo pipefail
cd "$(dirname "$0")/.."

# Cargo falla en un workspace virtual sin miembros (fase 1); se omite hasta que exista el primero.
if cargo metadata --no-deps --format-version 1 --locked | grep -q '"packages":\[\]'; then
  echo "cargo: workspace sin miembros, omitido"
else
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo test --workspace --locked
fi

pnpm install --frozen-lockfile
pnpm -r --if-present run lint
pnpm -r --if-present run typecheck
pnpm -r --if-present run test
