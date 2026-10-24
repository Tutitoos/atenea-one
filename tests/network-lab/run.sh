#!/usr/bin/env bash
# Ejecuta el laboratorio de red en un contenedor Linux privilegiado y desechable.
# Requiere Docker. Uso: tests/network-lab/run.sh [lab.sh|shell]
set -euo pipefail
cd "$(dirname "$0")/../.."

docker build -q -t atenea-lab tests/network-lab >/dev/null
exec docker run --rm --privileged \
  -v "$PWD":/src:ro \
  -v atenea-lab-target:/cache \
  -e CARGO_TARGET_DIR=/cache/target \
  -w /src atenea-lab \
  bash "tests/network-lab/${1:-lab.sh}"
