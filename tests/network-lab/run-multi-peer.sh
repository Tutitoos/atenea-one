#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Imágenes y binarios ya locales; no build/pull. Salida JSON con estado completo y métricas.
set -euo pipefail
cd "$(dirname "$0")/../.."
if (($# == 0)); then set -- 3 4 6 8; fi
container=
trap 'if [[ -n "$container" ]]; then docker rm -f "$container" >/dev/null 2>&1 || true; fi' EXIT
for n; do
  for mode in open blocked; do
    docker ps -a
    container="atenea-multi-$$-$n-$mode"
    docker run --pull=never --rm --name "$container" --network none --privileged \
      -e ATENEA_RELAY_MAX_PER_IP="${ATENEA_RELAY_MAX_PER_IP:-32}" \
      -v "$PWD:/src:ro" -v atenea-lab-target:/cache:ro -w /src \
      atenea-lab python3 tests/network-lab/multi-peer.py "$n" "$mode"
  done
done
docker ps -a
