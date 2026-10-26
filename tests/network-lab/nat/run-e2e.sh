#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# E2E de la fase 4: coordinator + STUN propios y agentes reales tras NAT emulados (nftables).
# Todo en un contenedor privilegiado y desechable. Uso: tests/network-lab/nat/run-e2e.sh
set -euo pipefail
cd "$(dirname "$0")/../../.."
docker build -q -t atenea-lab tests/network-lab >/dev/null
exec docker run --rm --privileged \
  -v "$PWD":/src:ro -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry -e CARGO_TARGET_DIR=/cache/target \
  -w /src atenea-lab bash tests/network-lab/nat/e2e.sh
