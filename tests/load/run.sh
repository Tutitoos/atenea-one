#!/usr/bin/env bash
# Solo imágenes y binarios locales; un único contenedor de carga, sin egreso.
set -euo pipefail
cd "$(dirname "$0")/../.."
mode=${1:-tunnel}
case "$mode" in tunnel|handshake|relay) ;; *) echo 'uso: run.sh tunnel|handshake|relay' >&2; exit 2;; esac
name=atenea-load-$mode-$$
out=${ATENEA_LOAD_OUTPUT:-$PWD/tests/load/results}
mkdir -p "$out/bin"
cleanup() { docker ps -a; docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM
docker ps -a
docker image inspect atenea-lab >/dev/null
docker run --pull=never --rm --name "$name" --network none --privileged \
  -v "$PWD:/src:ro" -v "$out:/results" -v "$out/bin:/load-bin:ro" -v atenea-lab-target:/cache:ro \
  -e CARGO_NET_OFFLINE=true -e CARGO_TARGET_DIR=/cache/target -e CARGO_BUILD_JOBS=2 \
  -e ATENEA_LOAD_PATHS -e ATENEA_LOAD_BASELINE \
  -e ATENEA_LOAD_RETRY_AUDIT \
  -e ATENEA_LOAD_RELAY_CAPACITY_ONLY \
  -e ATENEA_LOAD_RETRY_RATES \
  -w /src atenea-lab python3 tests/load/setup.py "$mode" "${2:-0.34}"
