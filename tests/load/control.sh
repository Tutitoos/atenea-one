#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Entry del runner aislado de tests/load/stack.sh; nunca lanza Docker.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${LOAD_CONTROL_OUT:-/results/control}
SIGNER=${ATENEA_LOAD_SIGNER:-$(command -v atenea-load-sign || true)}
: "${DATABASE_URL:?se requiere PostgreSQL desechable migrado (rol administrador)}"
: "${CP_URL:?URL privada del control-plane}"
: "${COORD_URL:?URL privada del coordinator}"
: "${ATENEA_LOAD_ROOT_KEY:?raíz offline exclusiva del laboratorio}"
: "${ATENEA_LOAD_ISSUER_KEY:?emisor del control-plane de laboratorio}"
[[ -n "$SIGNER" && -x "$SIGNER" ]] || { echo 'Falta atenea-load-sign compilado offline' >&2; exit 2; }
command -v psql >/dev/null
mkdir -p "$OUT"
umask 077
FAILED=0
run() {
  local name=$1 rc
  shift
  set +e
  python3 "$ROOT/tests/load/control.py" "$@" --fixtures "$FIXTURES" --signer "$SIGNER" \
    2>"$OUT/$name.stderr" | tee "$OUT/$name.jsonl"
  rc=${PIPESTATUS[0]}
  set -e
  printf '{"kind":"exit","phase":"%s","status":%d}\n' "$name" "$rc" | tee -a "$OUT/$name.jsonl"
  if (( rc != 0 )); then FAILED=1; fi
}
# Primero toda la matriz coordinator, después toda la matriz PostgreSQL/control-plane.
if [[ ${LOAD_CONTROL_PHASE:-all} != control-plane ]]; then
for count in ${LOAD_CONTROL_COUNTS:-1000 5000 10000}; do
  FIXTURES="$OUT/fixtures-$count"
  [[ ! -e "$FIXTURES" ]] || { echo "Ya existe $FIXTURES; usa un LOAD_CONTROL_OUT nuevo" >&2; exit 2; }
  "$SIGNER" fixtures "$count" "${LOAD_CONTROL_PER_NETWORK:-128}" "$FIXTURES"
  normal_rate=$(( 2 * ((count + ${LOAD_CONTROL_PER_NETWORK:-128} - 1) / ${LOAD_CONTROL_PER_NETWORK:-128}) ))
  for repetition in 1 2; do
    run "$count-r$repetition-coordinator" coordinator --url "$COORD_URL" --concurrency 200 \
      --duration "${LOAD_CONTROL_SECONDS:-10}" --rate "${LOAD_CONTROL_RATE:-$normal_rate}"
    sleep 20
    run "$count-r$repetition-quotas" coordinator --url "$COORD_URL" --concurrency 200 --quota \
      --duration 5 --rate 100 --poll-capacity "${LOAD_CONTROL_POLL_CAPACITY:-256}"
    # TTL y cubos no se heredan entre repeticiones ni escalas.
    sleep 65
  done
done
fi
for count in ${LOAD_CONTROL_COUNTS:-1000 5000 10000}; do
  FIXTURES="$OUT/fixtures-$count"
  if [[ ! -d "$FIXTURES" ]]; then "$SIGNER" fixtures "$count" "${LOAD_CONTROL_PER_NETWORK:-128}" "$FIXTURES"; fi
  run "$count-seed" seed --disposable --events "${LOAD_CONTROL_EVENTS:-10080}"
  [[ -s "$FIXTURES/session.json" ]] || exit 1
  for repetition in 1 2; do
    for concurrency in 50 200; do
      for workload in read issuance; do
        run "$count-r$repetition-$workload-$concurrency" control-plane --url "$CP_URL" \
          --concurrency "$concurrency" --workload "$workload" --duration "${LOAD_CONTROL_SECONDS:-10}"
        # El siguiente ensayo no hereda la ventana de cuota por dispositivo.
        sleep 65
      done
    done
    run "$count-r$repetition-websocket" websocket --url "$CP_URL" --origin "${LOAD_CONTROL_ORIGIN:-https://panel.stack.test}" \
      --duration "${LOAD_CONTROL_SECONDS:-10}" --connections "${LOAD_CONTROL_WS:-64}"
    sleep 65
  done
  run "$count-cleanup" cleanup --disposable
done
exit "$FAILED"
