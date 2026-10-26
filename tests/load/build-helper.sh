#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Compila clientes/agentes de carga actuales; cachés preexistentes solo lectura.
set -euo pipefail
cd "$(dirname "$0")/../.."
name=atenea-load-build-$$
output=${ATENEA_LOAD_OUTPUT:-$PWD/tests/load/results}
mkdir -p "$output/bin"
cleanup() { docker ps -a; docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM
docker ps -a
docker run --pull=never --rm --name "$name" --network none \
  -v "$PWD:/src:ro" -v "$output:/results" \
  -v atenea-cargo-registry:/usr/local/cargo/registry:ro \
  -e RUSTUP_TOOLCHAIN=1.99.1 -e CARGO_BUILD_JOBS=2 -e CARGO_NET_OFFLINE=true \
  -e CARGO_TARGET_DIR=/results/build -e ATENEA_BUILD_COMMIT="$(git rev-parse HEAD)" \
  -w /src atenea-lab bash -ec 'cargo build --offline --locked --release -p atenea-agent -p atenea-cli -p atenea-testpeer; cp /results/build/release/atenea /results/build/release/atenea-agent /results/build/release/atenea-testpeer /results/build/release/atenea-load-sign /results/bin/; rm -rf /results/build'
