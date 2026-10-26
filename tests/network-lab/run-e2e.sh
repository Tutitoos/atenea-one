#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# E2E de la fase 3B: control plane real + PostgreSQL 17 + Dex (OIDC) + agentes con TUN en namespaces.
# Todo en contenedores desechables; no toca la red del host. Requiere Docker.
set -euo pipefail
cd "$(dirname "$0")/../.."

NETNAME=atenea-e2e
cleanup() {
  local rc=$?
  if [ $rc -ne 0 ]; then docker logs --tail 15 atenea-e2e-dex 2>&1 | sed 's/^/[dex] /' || true; fi
  docker rm -f atenea-e2e-pg atenea-e2e-dex >/dev/null 2>&1 || true
  docker network rm $NETNAME >/dev/null 2>&1 || true
  return $rc
}
trap cleanup EXIT
docker rm -f atenea-e2e-pg atenea-e2e-dex >/dev/null 2>&1 || true
docker network rm $NETNAME >/dev/null 2>&1 || true
docker build -q -t atenea-lab tests/network-lab >/dev/null
docker network create $NETNAME >/dev/null
docker run -d --name atenea-e2e-pg --network $NETNAME --network-alias postgres \
  -e POSTGRES_PASSWORD=pg -e POSTGRES_DB=atenea postgres:17 >/dev/null
docker run -d --name atenea-e2e-dex --network $NETNAME --network-alias dex \
  -v "$PWD/deploy/compose/dev/dex.yaml":/etc/dex/config.yaml:ro \
  ghcr.io/dexidp/dex:v2.41.1 dex serve /etc/dex/config.yaml >/dev/null

docker run --rm --privileged --network $NETNAME \
  -v "$PWD":/src:ro -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry -e CARGO_TARGET_DIR=/cache/target \
  -w /src atenea-lab bash tests/network-lab/e2e-cp.sh
