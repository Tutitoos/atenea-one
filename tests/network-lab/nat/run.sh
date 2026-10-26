#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Uso: tests/network-lab/nat/run.sh [verify|relay|shell]
set -euo pipefail
cd "$(dirname "$0")/../../.."
case "${1:-verify}" in
  verify) command=(bash tests/network-lab/nat/verify-nat.sh); interactive=();;
  relay) command=(bash tests/network-lab/nat/verify-relay-lab.sh); interactive=();;
  shell) command=(bash); interactive=(-it);;
  *) echo 'Uso: run.sh [verify|relay|shell]' >&2; exit 2;;
esac
docker build -q -t atenea-lab tests/network-lab >/dev/null
exec docker run --rm --privileged "${interactive[@]}" \
  -v "$PWD":/src:ro -w /src atenea-lab "${command[@]}"
