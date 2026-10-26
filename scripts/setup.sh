#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Prepara el entorno de desarrollo sin modificar trabajo existente.
set -euo pipefail
cd "$(dirname "$0")/.."

for c in cargo node pnpm; do
  command -v "$c" >/dev/null || { echo "falta: $c" >&2; exit 1; }
done

want=$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)
if command -v rustup >/dev/null; then
  rustup show active-toolchain >/dev/null # instala la versión de rust-toolchain.toml
elif [ "$(rustc --version | cut -d' ' -f2)" != "$want" ]; then
  echo "aviso: rustc distinto de $want (sin rustup no se puede fijar)" >&2
fi

pnpm install --frozen-lockfile
echo "entorno listo"
