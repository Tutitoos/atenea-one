#!/usr/bin/env bash
# Ensayo del stack de producción, sin publicar puertos del host ni servicios externos.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
PROJECT=atenea-stack-$$
export ATENEA_RUNTIME_DIR="$ROOT/deploy/compose/prod/.runtime-$PROJECT"
ENV_FILE="$ATENEA_RUNTIME_DIR/stack.env"
COMPOSE=(docker compose -p "$PROJECT" --env-file "$ENV_FILE" -f deploy/compose/prod/compose.yaml -f tests/stack/compose.yaml)
cleanup() {
  rc=$?
  docker rm -f "$PROJECT-run" >/dev/null 2>&1 || true
  if [[ -e "$ENV_FILE" ]]; then
    if (( rc != 0 )); then "${COMPOSE[@]}" logs --no-color; fi
    "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  docker run --rm --network none -v "$ATENEA_RUNTIME_DIR:/runtime" atenea-panel-lab bash -c 'rm -rf /runtime/*' >/dev/null 2>&1 || true
  rmdir "$ATENEA_RUNTIME_DIR" 2>/dev/null || true
  exit "$rc"
}
trap cleanup EXIT
bash deploy/compose/prod/bootstrap.sh
# Solo backchannel OIDC interno: el cliente del servicio usa raíces públicas, no la CA efímera.
docker run --rm --network none --user 0 -v "$ATENEA_RUNTIME_DIR:/runtime" atenea-panel-lab python3 -c 'from pathlib import Path; p=Path("/runtime/secrets/dex.yaml");p.chmod(0o600);p.write_text(p.read_text().replace("issuer: https://panel.stack.test/dex", "issuer: http://dex:5556/dex"));p.chmod(0o400)'
"${COMPOSE[@]}" config --quiet
echo 'PASS docker compose config'
START=$SECONDS
"${COMPOSE[@]}" up -d --wait postgres
# migrate.sh usa el mismo nombre de proyecto que esta ejecución.
export COMPOSE_PROJECT_NAME="$PROJECT"
bash deploy/compose/prod/migrate.sh "$ENV_FILE"
"${COMPOSE[@]}" up -d --wait --wait-timeout 150
echo "PASS stack saludable en $((SECONDS-START)) s (incluye migraciones)"
EDGE_NETWORK="$PROJECT"_edge
EDGE_IP=$(docker inspect "$(${COMPOSE[@]} ps -q haproxy)" --format "{{(index .NetworkSettings.Networks \"$EDGE_NETWORK\").IPAddress}}")
CP_IP=$(docker inspect "$(${COMPOSE[@]} ps -q control-plane)" --format "{{(index .NetworkSettings.Networks \"$EDGE_NETWORK\").IPAddress}}")
COORD_IP=$(docker inspect "$(${COMPOSE[@]} ps -q coordinator)" --format "{{(index .NetworkSettings.Networks \"$EDGE_NETWORK\").IPAddress}}")
export EDGE_IP CP_IP COORD_IP
docker run -d --rm --name "$PROJECT-run" --privileged --network "$EDGE_NETWORK" \
  --add-host "panel.stack.test:$EDGE_IP" --add-host "relay.stack.test:$EDGE_IP" \
  -e EDGE_IP -e CP_IP -e COORD_IP -e LAB_ROOT=/src \
  -v "$ROOT:/src:ro" -v "$ATENEA_RUNTIME_DIR:/runtime:ro" \
  -v atenea-lab-target:/cache \
  atenea/stack-runner:local sleep infinity >/dev/null
RUNNER_IP=$(docker inspect "$PROJECT-run" --format "{{(index .NetworkSettings.Networks \"$EDGE_NETWORK\").IPAddress}}")
# Retorno a las NAT simuladas sin un segundo SNAT que falsee los candidatos STUN.
for service in coordinator haproxy; do
  docker run --rm --privileged --network "container:$(${COMPOSE[@]} ps -q "$service")" \
    atenea-panel-lab ip route add 198.51.100.0/24 via "$RUNNER_IP"
done
docker exec "$PROJECT-run" bash /src/tests/stack/in-container.sh
