#!/usr/bin/env bash
# Simulacros reales, secuenciales, sin pull ni compilación. bash run.sh [lab|pilot]
set -euo pipefail
cd "$(dirname "$0")/../../.."
ROOT=$PWD
PROFILE=${1:-lab}
case "$PROFILE" in lab) export DRILL_LEASE=60 DRILL_REISSUE=20;; pilot) export DRILL_LEASE=300 DRILL_REISSUE=60;; *) exit 2;; esac
export DRILL_PROFILE=$PROFILE
PROJECT=atenea-drill-$$
export ATENEA_RUNTIME_DIR="$ROOT/deploy/compose/prod/.runtime-$PROJECT"
ENV_FILE="$ATENEA_RUNTIME_DIR/stack.env"
COMPOSE=(docker compose -p "$PROJECT" --env-file "$ENV_FILE" -f deploy/compose/prod/compose.yaml -f tests/stack/compose.yaml -f tests/stack/drills/compose.yaml)
cleanup() {
  rc=$?
  trap - EXIT
  docker rm -f "$PROJECT-run" >/dev/null 2>&1 || true
  if [[ -f "$ENV_FILE" ]]; then
    if ((rc)); then "${COMPOSE[@]}" logs --no-color; fi
    "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  docker run --pull=never --rm --network none -v "$ATENEA_RUNTIME_DIR:/runtime" atenea-panel-lab bash -c 'rm -rf /runtime/*' >/dev/null 2>&1 || true
  rmdir "$ATENEA_RUNTIME_DIR" 2>/dev/null || true
  docker ps -a
  docker network ls
  docker volume ls --filter "name=$PROJECT"
  echo "DRILL cleanup project=$PROJECT exit=$rc; pulls=0 (pull_policy=never/--pull=never; imágenes precomprobadas)"
  exit "$rc"
}
trap cleanup EXIT
# Inventario antes de cada ensayo; no se borra nada preexistente.
docker ps -a
docker network ls
# Todas las imágenes del bootstrap y de Compose deben existir antes de crear recursos.
for image in atenea-panel-lab atenea/tools:local atenea/stack-runner:local atenea/control-plane:local atenea/coordinator:local atenea/relay:local \
  'postgres:17@sha256:b8faeff7712663c2773a190181e1246e37e364b887611c04658c28b44e2408fe' \
  'haproxy:3.2-alpine@sha256:5893840576071e6217cd52ebdcc14f454c6f7a631056fa6f4e0f40d3cac4215b' \
  'caddy:2.10-alpine@sha256:4c6e91c6ed0e2fa03efd5b44747b625fec79bc9cd06ac5235a779726618e530d' \
  'ghcr.io/dexidp/dex:v2.41.1@sha256:bc7cfce7c17f52864e2bb2a4dc1d2f86a41e3019f6d42e81d92a301fad0c8a1d'; do
  docker image inspect "$image" --format '{{.Id}}' >/dev/null
done
bash deploy/compose/prod/bootstrap.sh
docker run --pull=never --rm --network none --user 0 -v "$ATENEA_RUNTIME_DIR:/runtime" -v "$ROOT:/src:ro" atenea-panel-lab python3 /src/tests/stack/drills/prepare.py
"${COMPOSE[@]}" up --pull never -d --wait postgres
export COMPOSE_PROJECT_NAME="$PROJECT"
bash deploy/compose/prod/migrate.sh "$ENV_FILE"
"${COMPOSE[@]}" up --pull never -d --wait --wait-timeout 150
EDGE_NETWORK=${PROJECT}_edge
getip() { docker inspect "$("${COMPOSE[@]}" ps -q "$1")" --format "{{(index .NetworkSettings.Networks \"$EDGE_NETWORK\").IPAddress}}"; }
EDGE_IP=$(getip netns)
CP_IP=$(getip control-plane)
COORD_IP=$(getip coordinator)
RELAY2_IP=$(getip relay2)
export EDGE_IP CP_IP COORD_IP
docker run --pull=never -d --rm --name "$PROJECT-run" --privileged --network "$EDGE_NETWORK" \
  --add-host "panel.stack.test:$EDGE_IP" --add-host "relay.stack.test:$EDGE_IP" --add-host "relay2:$RELAY2_IP" \
  -e EDGE_IP -e CP_IP -e COORD_IP -e DRILL_PROFILE -e DRILL_LEASE -e DRILL_REISSUE -e LAB_ROOT=/src -e ATENEA_STACK_DRILLS=1 \
  -v "$ROOT:/src:ro" -v "$ATENEA_RUNTIME_DIR:/runtime:ro" -v atenea-lab-target:/cache \
  atenea/stack-runner:local sleep infinity >/dev/null
RUNNER_IP=$(docker inspect "$PROJECT-run" --format "{{(index .NetworkSettings.Networks \"$EDGE_NETWORK\").IPAddress}}")
for service in coordinator netns relay2; do
  docker run --pull=never --rm --privileged --network "container:$("${COMPOSE[@]}" ps -q "$service")" atenea-panel-lab ip route add 198.51.100.0/24 via "$RUNNER_IP"
done
docker exec "$PROJECT-run" bash /src/tests/stack/lab.sh setup
python3 tests/stack/drills/drive.py "$PROJECT-run" "${COMPOSE[@]}"
