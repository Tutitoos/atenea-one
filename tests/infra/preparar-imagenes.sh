#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Prepara en el Docker local las imágenes que `tests/infra/run.sh` exige ya existentes (run.sh no construye ni descarga):
#   - atenea-infra:local: el nodo de ensayo y el controlador (tests/infra/Dockerfile).
#   - atenea/{control-plane,coordinator,relay}:local y atenea/tools:local: construidas desde ESTE árbol (deploy/images).
#   - haproxy, caddy, postgres y dex: los digests fijados en deploy/compose/prod/compose.yaml, con los alias que usa run.sh.
# Descarga y compila, así que necesita red; es lo que ejecuta el job `infra` de la CI.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
commit=${ATENEA_BUILD_COMMIT:-$(git rev-parse HEAD)}

# Referencia fijada (con digest) de la primera imagen del compose que empieza por el prefijo dado.
pinned() {
    local ref
    ref=$(awk -v prefix="$1" '$1 == "image:" && index($2, prefix) == 1 { print $2; exit }' deploy/compose/prod/compose.yaml)
    [[ $ref == *@sha256:* ]] || { echo "preparar-imagenes: sin imagen fijada por digest para $1" >&2; exit 1; }
    printf '%s\n' "$ref"
}
haproxy=$(pinned haproxy:)
caddy=$(pinned caddy:)
postgres=$(pinned postgres:)
dex=$(pinned ghcr.io/dexidp/dex:)
for ref in "$haproxy" "$caddy" "$postgres" "$dex"; do docker pull -q "$ref" >/dev/null; done
# Docker no pone etiqueta a una imagen descargada por digest: run.sh las referencia por estos nombres.
docker tag "$haproxy" atenea-infra-pin/haproxy:pinned
docker tag "$caddy" atenea-infra-pin/caddy:pinned
docker tag "$postgres" postgres:17
docker tag "$dex" "${dex%@*}"

docker build -t atenea-infra:local tests/infra
for service in control-plane coordinator relay; do
    docker build --target runtime --build-arg "ATENEA_BUILD_COMMIT=$commit" \
        -t "atenea/$service:local" -f "deploy/images/$service/Dockerfile" .
done
docker build --target tools --build-arg "ATENEA_BUILD_COMMIT=$commit" \
    -t atenea/tools:local -f deploy/images/control-plane/Dockerfile .
echo "Imágenes de ensayo listas para tests/infra/run.sh"
