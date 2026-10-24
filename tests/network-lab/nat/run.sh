#!/usr/bin/env bash
# Uso: tests/network-lab/nat/run.sh [verify|shell]
set -euo pipefail
cd "$(dirname "$0")/../../.."
case "${1:-verify}" in
  verify) command=(bash tests/network-lab/nat/verify-nat.sh); interactive=();;
  shell) command=(bash); interactive=(-it);;
  *) echo 'Uso: run.sh [verify|shell]' >&2; exit 2;;
esac
docker build -q -t atenea-lab tests/network-lab >/dev/null
exec docker run --rm --privileged "${interactive[@]}" \
  -v "$PWD":/src:ro -w /src atenea-lab "${command[@]}"
