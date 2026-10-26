#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Laboratorio de la fase 7 (subredes, DNS, control local y restauración) en un contenedor privilegiado y desechable.
# Requiere Docker. Uso: tests/network-lab/subnet/run.sh
set -euo pipefail
cd "$(dirname "$0")/../../.."
docker build -q -t atenea-lab tests/network-lab >/dev/null
exec docker run --rm --privileged \
  -v "$PWD":/src:ro -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry -e CARGO_TARGET_DIR=/cache/target \
  -w /src atenea-lab bash tests/network-lab/subnet/lab.sh
