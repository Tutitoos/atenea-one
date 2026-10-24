#!/usr/bin/env bash
# Uso: fuzz/run.sh [segundos=60] [objetivo]   (sin objetivo: todos)
# Código de salida ≠ 0 si algún objetivo falla (crash, aserción o error de compilación).
set -euo pipefail

SECS="${1:-60}"
ONLY="${2:-}"
DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$DIR")"
IMAGE=atenea-fuzz

docker build -q -t "$IMAGE" -f "$DIR/Dockerfile" "$DIR" >/dev/null

# Dentro del contenedor: copia las semillas al corpus (sin pisar) y ejecuta cada objetivo.
INNER='
set -u
rc=0
targets="${TARGET:-$(cargo +nightly fuzz list)}"
for t in $targets; do
  mkdir -p "corpus/$t" "artifacts/$t"
  [ -d "seeds/$t" ] && cp -n seeds/"$t"/* "corpus/$t"/ 2>/dev/null
  echo "=== $t (${SECS}s) ==="
  cargo +nightly fuzz run "$t" "corpus/$t" -- -max_total_time="$SECS" -print_final_stats=1 || rc=1
done
exit $rc
'

docker run --rm \
  -e SECS="$SECS" -e TARGET="$ONLY" \
  -v "$ROOT":/work \
  -v atenea-fuzz-target:/work/fuzz/target \
  -v atenea-fuzz-cargo:/usr/local/cargo/registry \
  "$IMAGE" bash -c "$INNER"
