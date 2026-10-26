#!/usr/bin/env bash
# bash tests/load/stack.sh control | soak 30m | soak 2h (entero sin sufijo: minutos).
# Prerrequisitos: imágenes locales, volumen atenea-lab-target RO y binarios nuevos
# tests/load/results/bin/{atenea,atenea-agent,atenea-load-sign}; no instala/compila/descarga.
# Contrato del runner (/src RO, /runtime RO, /results y /out RW, cwd /src):
# EDGE_IP, CP_IP, COORD_IP; CP_URL=http://CP_IP:8080, COORD_URL=http://COORD_IP:8081;
# DATABASE_URL=postgres://postgres:<secreto efímero>@postgres/atenea (solo laboratorio),
# ATENEA_LOAD_ROOT_KEY=/runtime/offline/root.key, ATENEA_LOAD_ISSUER_KEY=/runtime/secrets/issuer.key.
# control.sh invoca control.py con sus perfiles; ambos deben fallar con código no cero.
# Se reenvían LOAD_CONTROL_{COUNTS,SECONDS,PER_NETWORK,EVENTS,WS,RATE,POLL_CAPACITY,ORIGIN}.
# soak reutiliza api.py (ATENEA_STACK_LOAD=soak) para crear/aprobar/iniciar dos agentes;
# después ejecuta traffic.py soak ATENEA_SOAK_HOURS, con ATENEA_LOAD_PEER_IP real.
# traffic.py bloquea durante la carga y escribe /results/soak.jsonl; el host añade
# services.jsonl, states.jsonl y processes.jsonl cada 10 s durante soak.
set -euo pipefail
cd "$(dirname "$0")/../.."
export ROOT=$PWD
usage() { echo 'Uso: bash tests/load/stack.sh control | soak <minutos[m]|horas[h]> (p.ej. 30m, 2h, 0.5h)' >&2; exit 2; }
MODE=${1:-}
ATENEA_SOAK_HOURS=0
case "$MODE" in
  control) [[ $# == 1 ]] || usage ;;
  soak)
    [[ $# == 2 ]] || usage
    # Decimal evita redondear silenciosamente una duración inválida o aceptar NaN/inf.
    ATENEA_SOAK_HOURS=$(python3 - "$2" <<'PY'
from decimal import Decimal
import re
import sys
value = re.fullmatch(r'([0-9]+(?:\.[0-9]+)?)([mh]?)', sys.argv[1])
if not value:
    raise SystemExit('Duración inválida: usa minutos (30m) u horas (2h).')
number, unit = value.groups()
seconds = Decimal(number) * (3600 if unit == 'h' else 60)
if seconds < 60 or seconds != seconds.to_integral_value():
    raise SystemExit('Duración mínima: un minuto; debe equivaler a segundos enteros.')
print(seconds / 3600)
PY
    )
    ;;
  *) usage ;;
esac
# macOS no trae coreutils timeout; equivalente local sin instalar dependencias.
timeout() { python3 "$ROOT/tests/load/timeout.py" "$@"; }
if [[ "$MODE" == soak && "${ATENEA_LOAD_DEADLINE:-}" != 1 ]]; then
  DEADLINE=$(python3 -c 'import math,sys; print(math.ceil(float(sys.argv[1])*3600)+600)' "$ATENEA_SOAK_HOURS")
  export ATENEA_LOAD_DEADLINE=1
  exec python3 "$ROOT/tests/load/timeout.py" "$DEADLINE" bash "$0" "$@"
fi
command -v docker >/dev/null
command -v python3 >/dev/null
[[ -f tests/load/control.sh && -f tests/load/control.py && -f tests/load/traffic.py ]] || {
  echo 'Faltan los workers de tests/load (control.sh, control.py, traffic.py)' >&2; exit 2;
}
RUNNER_OPTIONS=(--workdir /src -v "$ROOT/tests/load/results/bin:/load-bin:ro")
for binary in atenea atenea-agent; do
  [[ -x "tests/load/results/bin/$binary" ]] || { echo "Falta tests/load/results/bin/$binary compilado offline" >&2; exit 2; }
  # Los helpers anteponen /cache: el bind RO selecciona el binario nuevo sin escribir el volumen.
  RUNNER_OPTIONS+=(-v "$ROOT/tests/load/results/bin/$binary:/cache/target/release/$binary:ro")
done
if [[ "$MODE" == control ]]; then
  [[ -x tests/load/results/bin/atenea-load-sign ]] || { echo 'Falta tests/load/results/bin/atenea-load-sign compilado offline' >&2; exit 2; }
  RUNNER_OPTIONS+=(-e ATENEA_LOAD_SIGNER=/load-bin/atenea-load-sign)
fi
export PROJECT=atenea-load-$$
export PANEL_HOST=panel.stack.test RELAY_HOST=relay.stack.test TOOLS_IMAGE=atenea/tools:local
export ATENEA_RUNTIME_DIR="$ROOT/deploy/compose/prod/.runtime-$PROJECT"
export COMPOSE_PROJECT_NAME="$PROJECT"
ENV_FILE="$ATENEA_RUNTIME_DIR/stack.env"
COMPOSE=(docker compose -p "$PROJECT" --env-file "$ENV_FILE" -f deploy/compose/prod/compose.yaml -f tests/stack/compose.yaml -f tests/load/stack.compose.yaml)
RESULTS="$ROOT/tests/load/out/$PROJECT"
[[ ! -e "$ATENEA_RUNTIME_DIR" && ! -e "$RESULTS" ]] || { echo 'Ya existe el runtime o la salida propia; no se sobrescribe' >&2; exit 2; }
mkdir -p "$ROOT/tests/load/out"
mkdir -m 700 "$RESULTS"
LOAD_PID=
SAMPLE_PID=
docker() {
  if [[ "$1" == run ]]; then command docker ps -a >&2; fi
  command docker "$@"
}
cleanup() {
  local rc=$? id
  trap - EXIT INT TERM
  set +e
  [[ -z "$SAMPLE_PID" ]] || { kill "$SAMPLE_PID" 2>/dev/null; wait "$SAMPLE_PID" 2>/dev/null; }
  [[ -z "$LOAD_PID" ]] || kill "$LOAD_PID" 2>/dev/null
  if [[ "$MODE" == soak ]]; then
    timeout 15 docker exec "$PROJECT-run" python3 -c '
from collections import deque
from pathlib import Path
for p in Path("/tmp/lab").glob("*.log"):
    with p.open(errors="replace") as stream:
        Path("/results/agent-"+p.name).write_text("".join(deque(stream, maxlen=2000)))
' >"$RESULTS/preserve.log" 2>&1
    for id in coordinator relay; do
      timeout 15 docker logs --timestamps --tail 2000 "$PROJECT-$id-1" >"$RESULTS/$id.log" 2>&1
    done
  fi
  if ((rc)); then
    [[ ! -f "$RESULTS/$MODE.log" ]] || cat "$RESULTS/$MODE.log" >&2
    docker logs "$PROJECT-run" >"$RESULTS/runner.log" 2>&1
    if [[ -f "$ENV_FILE" ]]; then
      "${COMPOSE[@]}" logs --no-color >"$RESULTS/stack.log" 2>&1
      cat "$RESULTS/stack.log" >&2
    fi
  fi
  timeout 30 docker rm -f "$PROJECT-run" "$PROJECT-sampler" >/dev/null 2>&1
  if [[ -n "$LOAD_PID" ]]; then wait "$LOAD_PID" 2>/dev/null; fi
  # Etiqueta exacta propia; jamás prune, recursos globales ni el cache compartido.
  for id in $(docker ps -aq --filter "label=atenea.load.project=$PROJECT"); do docker rm -f "$id" >/dev/null 2>&1 || rc=1; done
  if [[ -f "$ENV_FILE" ]]; then "${COMPOSE[@]}" down -v --remove-orphans >"$RESULTS/cleanup.log" 2>&1 || { cat "$RESULTS/cleanup.log" >&2; rc=1; }; fi
  if [[ -d "$ATENEA_RUNTIME_DIR" ]]; then
    docker run --pull=never --rm --network none --name "$PROJECT-cleanup" --user 0 \
      -v "$ATENEA_RUNTIME_DIR:/runtime" atenea-panel-lab bash -c 'rm -rf /runtime/*' >/dev/null 2>&1 || rc=1
    rmdir "$ATENEA_RUNTIME_DIR" 2>/dev/null || { echo "No se pudo eliminar $ATENEA_RUNTIME_DIR (contiene secretos)" >&2; rc=1; }
  fi
  docker ps -a
  docker network ls
  docker volume ls --filter "label=com.docker.compose.project=$PROJECT"
  local containers networks volumes
  containers=$(docker ps -aq --filter "name=^/${PROJECT}[-_]" | wc -l)
  networks=$(docker network ls -q --filter "label=com.docker.compose.project=$PROJECT" | wc -l)
  volumes=$(docker volume ls -q --filter "label=com.docker.compose.project=$PROJECT" | wc -l)
  echo "LOAD recursos propios restantes: contenedores=$containers redes=$networks volúmenes=$volumes"
  if ((containers || networks || volumes)); then rc=1; fi
  echo "LOAD project=$PROJECT exit=$rc resultados=$RESULTS"
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# Inventario anterior a cualquier creación de contenedores o redes.
docker ps -a
docker network ls
for image in atenea-panel-lab atenea/tools:local atenea/stack-runner:local atenea/control-plane:local atenea/coordinator:local atenea/relay:local \
  'postgres:17@sha256:b8faeff7712663c2773a190181e1246e37e364b887611c04658c28b44e2408fe' \
  'haproxy:3.2-alpine@sha256:5893840576071e6217cd52ebdcc14f454c6f7a631056fa6f4e0f40d3cac4215b' \
  'caddy:2.10-alpine@sha256:4c6e91c6ed0e2fa03efd5b44747b625fec79bc9cd06ac5235a779726618e530d' \
  'ghcr.io/dexidp/dex:v2.41.1@sha256:bc7cfce7c17f52864e2bb2a4dc1d2f86a41e3019f6d42e81d92a301fad0c8a1d'; do
  docker image inspect "$image" --format '{{.Id}}' >/dev/null
done
docker volume inspect atenea-lab-target >/dev/null
# Los scripts de producción se reutilizan intactos. Su docker run queda sin red ni pull;
# migrate.sh usa redes ya creadas (todas internas) y compose run --pull never.
offline() (
  docker() {
    local operation=$1 argument
    shift
    case "$operation" in
      run) command docker ps -a >&2; command docker run --pull=never --network none --label "atenea.load.project=$PROJECT" --name "$PROJECT-aux-$BASHPID-$RANDOM" "$@" ;;
      compose)
        local args=()
        for argument in "$@"; do
          if [[ "$argument" == run ]]; then
            args+=(-f "$ROOT/tests/stack/compose.yaml" -f "$ROOT/tests/load/stack.compose.yaml" run --pull never)
          else
            args+=("$argument")
          fi
        done
        command docker ps -a >&2
        command docker compose "${args[@]}"
        ;;
      *) command docker "$operation" "$@" ;;
    esac
  }
  export -f docker
  bash "$@"
)
offline deploy/compose/prod/bootstrap.sh
docker run --pull=never --rm --network none --name "$PROJECT-prepare" --label "atenea.load.project=$PROJECT" --user 0 \
  -v "$ATENEA_RUNTIME_DIR:/runtime" atenea-panel-lab python3 -c 'from pathlib import Path; p=Path("/runtime/secrets/dex.yaml");p.chmod(0o600);p.write_text(p.read_text().replace("issuer: https://panel.stack.test/dex", "issuer: http://dex:5556/dex"));p.chmod(0o400)'
"${COMPOSE[@]}" config --quiet
echo 'PASS docker compose config'
START=$SECONDS
"${COMPOSE[@]}" up --pull never -d --wait postgres
offline deploy/compose/prod/migrate.sh "$ENV_FILE"
"${COMPOSE[@]}" up --pull never -d --wait --wait-timeout 150
echo "PASS stack saludable en $((SECONDS-START)) s (incluye migraciones)"
EDGE_NETWORK=${PROJECT}_edge
getip() { docker inspect "$("${COMPOSE[@]}" ps -q "$1")" --format "{{(index .NetworkSettings.Networks \"$EDGE_NETWORK\").IPAddress}}"; }
EDGE_IP=$(getip netns)
CP_IP=$(getip control-plane)
COORD_IP=$(getip coordinator)
[[ -n "$EDGE_IP" && -n "$CP_IP" && -n "$COORD_IP" ]] || { echo 'Falta una IP interna del stack' >&2; exit 1; }
export EDGE_IP CP_IP COORD_IP ATENEA_SOAK_HOURS
export CP_URL="http://$CP_IP:8080" COORD_URL="http://$COORD_IP:8081"
docker run --pull=never -d --rm --name "$PROJECT-run" --privileged --network "$EDGE_NETWORK" \
  --label "atenea.load.project=$PROJECT" "${RUNNER_OPTIONS[@]}" \
  --add-host "panel.stack.test:$EDGE_IP" --add-host "relay.stack.test:$EDGE_IP" \
  -e EDGE_IP -e CP_IP -e COORD_IP -e CP_URL -e COORD_URL -e LAB_ROOT=/src \
  -e LOAD_CONTROL_COUNTS -e LOAD_CONTROL_SECONDS -e LOAD_CONTROL_PER_NETWORK \
  -e LOAD_CONTROL_PHASE \
  -e LOAD_CONTROL_EVENTS -e LOAD_CONTROL_WS -e LOAD_CONTROL_RATE -e LOAD_CONTROL_POLL_CAPACITY -e LOAD_CONTROL_ORIGIN \
  -e ATENEA_SOAK_HOURS -e ATENEA_STACK_LOAD="$MODE" \
  -e ATENEA_LOAD_ROOT_KEY=/runtime/offline/root.key -e ATENEA_LOAD_ISSUER_KEY=/runtime/secrets/issuer.key \
  -v "$ROOT:/src:ro" -v "$ATENEA_RUNTIME_DIR:/runtime:ro" -v atenea-lab-target:/cache:ro \
  -v "$RESULTS:/results" -v "$RESULTS:/out" atenea/stack-runner:local sleep infinity >/dev/null
# PostgreSQL solo existe en backend; no se publica ni se cambia su autenticación.
docker network connect "${PROJECT}_backend" "$PROJECT-run"
# El runner no descarga nada (imágenes locales, --pull never, compilación offline); el filtrado de salida con
# nftables cortaba las redes simuladas del laboratorio (10.x, 192.168.x, 2001:db8::), así que no se aplica.
RUNNER_IP=$(docker inspect "$PROJECT-run" --format "{{(index .NetworkSettings.Networks \"$EDGE_NETWORK\").IPAddress}}")
for service in coordinator netns; do
  docker run --pull=never --rm --privileged --name "$PROJECT-route-$service" --label "atenea.load.project=$PROJECT" \
    --network "container:$("${COMPOSE[@]}" ps -q "$service")" atenea-panel-lab ip route add 198.51.100.0/24 via "$RUNNER_IP"
done
# El secreto queda dentro del runner; nunca se imprime ni pasa por argv del host.
runner() {
  docker exec "$PROJECT-run" bash -c 'set -euo pipefail; export PATH=/load-bin:/cache/target/release:$PATH; export DATABASE_URL="postgres://postgres:$(cat /runtime/secrets/postgres-password)@postgres/atenea"; command -v python3 >/dev/null; exec "$@"' bash "$@"
}
SAMPLE_INTERVAL=10
if [[ "$MODE" == control ]]; then
  SAMPLE_INTERVAL=2
  runner bash -euc 'command -v psql >/dev/null; command -v atenea-load-sign >/dev/null; exec bash /src/tests/load/control.sh' >"$RESULTS/control.log" 2>&1 &
else
  runner bash -euc 'command -v atenea >/dev/null; bash /src/tests/stack/lab.sh setup; exec python3 /src/tests/stack/api.py' >"$RESULTS/soak.log" 2>&1 &
fi
LOAD_PID=$!
SERVICE_IDS=()
while IFS= read -r id; do [[ -z "$id" ]] || SERVICE_IDS+=("$id"); done < <("${COMPOSE[@]}" ps -q)
[[ ${#SERVICE_IDS[@]} -gt 0 ]] || { echo 'No hay servicios que muestrear' >&2; exit 1; }
docker run --pull=never -d --rm --network none --pid host --user 0 \
  --name "$PROJECT-sampler" --label "atenea.load.project=$PROJECT" atenea-panel-lab sleep infinity >/dev/null
PROCESS_TARGETS=()
for service in control-plane coordinator relay postgres; do
  id=$("${COMPOSE[@]}" ps -q "$service")
  PROCESS_TARGETS+=("$service:$(docker inspect --format '{{.State.Pid}}' "$id")")
done
sample() {
  local stamp
  stamp=$(date +%s)
  timeout 15 docker stats --no-stream --format '{"sample_unix":'"$stamp"',"stats":{{json .}}}' "${SERVICE_IDS[@]}" >>"$RESULTS/services.jsonl"
  timeout 15 docker inspect --format '{"sample_unix":'"$stamp"',"name":{{json .Name}},"restarts":{{.RestartCount}},"state":{{json .State}}}' "${SERVICE_IDS[@]}" >>"$RESULTS/states.jsonl"
  timeout 15 docker exec "$PROJECT-sampler" python3 -c '
import json, os, sys
from pathlib import Path
for target in sys.argv[2:]:
    service, pid = target.split(":")
    p = Path("/proc")/pid
    print(json.dumps(dict(sample_unix=int(sys.argv[1]), service=service, pid=int(pid),
        name=(p/"comm").read_text().strip(), clock_ticks=os.sysconf("SC_CLK_TCK"),
        status=(p/"status").read_text(), stat=(p/"stat").read_text(),
        fds=len(list((p/"fd").iterdir())))), flush=True)
' "$stamp" "${PROCESS_TARGETS[@]}" >>"$RESULTS/processes.jsonl"
}
(
  while kill -0 "$LOAD_PID" 2>/dev/null; do
    NEXT=$((SECONDS + SAMPLE_INTERVAL))
    sample || { echo 'Fallo del muestreador' >&2; exit 1; }
    while ((SECONDS < NEXT)) && kill -0 "$LOAD_PID" 2>/dev/null; do sleep 1; done
  done
) >"$RESULTS/sampler.log" 2>&1 &
SAMPLE_PID=$!
set +e
wait "$LOAD_PID"
LOAD_RC=$?
set -e
LOAD_PID=
kill "$SAMPLE_PID" 2>/dev/null || true
wait "$SAMPLE_PID" 2>/dev/null || true
SAMPLE_PID=
if ((LOAD_RC)); then echo "ERROR $MODE: runner terminó con código $LOAD_RC" >&2; exit "$LOAD_RC"; fi
sample
cat "$RESULTS/$MODE.log"
"${COMPOSE[@]}" logs --no-color >"$RESULTS/stack.log" 2>&1
if [[ "$MODE" == soak ]]; then
  python3 tests/load/summarize.py soak "$RESULTS"
  python3 - "$RESULTS/soak.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1])]
summary = next(row for row in reversed(rows) if row['kind'] == 'soak-summary')
print(f"soak completado en {summary['elapsed_s']:.0f} s: {summary['samples']} muestras")
PY
fi
