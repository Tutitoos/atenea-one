#!/usr/bin/env bash
# E2E del panel (fase 6): Playwright (Chromium) maneja el panel real con control plane + PostgreSQL 17 +
# Dex (OIDC) y dos agentes Linux con TUN tras NAT. Todo en contenedores desechables; requiere Docker.
# Uso: tests/e2e-panel/run.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

NETNAME=atenea-panel-routes
cleanup() {
  local rc=$?
  docker rm -f atenea-panel-routes-pg atenea-panel-routes-dex atenea-panel-routes-run >/dev/null 2>&1 || true
  docker network rm $NETNAME >/dev/null 2>&1 || true
  return $rc
}
trap cleanup EXIT
docker rm -f atenea-panel-routes-pg atenea-panel-routes-dex atenea-panel-routes-run >/dev/null 2>&1 || true
docker network rm $NETNAME >/dev/null 2>&1 || true

# El panel se construye en el anfitrión (salida estática independiente de la plataforma).
pnpm install --frozen-lockfile >/dev/null
pnpm --filter ./apps/web run build

docker build -q -t atenea-lab tests/network-lab >/dev/null
docker build -q -t atenea-panel-lab -f tests/e2e-panel/Dockerfile tests/e2e-panel >/dev/null
docker network create $NETNAME >/dev/null
docker run -d --name atenea-panel-routes-pg --network $NETNAME --network-alias postgres \
  -e POSTGRES_PASSWORD=pg -e POSTGRES_DB=atenea postgres:17 >/dev/null
docker run -d --name atenea-panel-routes-dex --network $NETNAME --network-alias dex \
  -v "$PWD/deploy/compose/dev/dex.yaml":/etc/dex/config.yaml:ro \
  ghcr.io/dexidp/dex:v2.41.1 dex serve /etc/dex/config.yaml >/dev/null

mkdir -p apps/web/test-results
docker run --rm --name atenea-panel-routes-run --privileged --network $NETNAME \
  -v "$PWD":/src:ro -v "$PWD/apps/web/test-results":/src/apps/web/test-results \
  -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry -e CARGO_TARGET_DIR=/cache/target \
  -v atenea-panel-pnpm:/pnpm-store -v atenea-panel-modules:/work \
  -w /src atenea-panel-lab bash tests/e2e-panel/in-container.sh
