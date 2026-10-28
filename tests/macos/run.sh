#!/bin/bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# SOLO runner macOS desechable: modifica red y /etc/resolver como root.
set -euo pipefail
[[ $(uname -s) == Darwin && $(id -u) == 0 && ${GITHUB_ACTIONS:-} == true ]] || {
  echo 'Solo se ejecuta como root en una VM macOS de GitHub Actions' >&2; exit 1;
}
cd "$(dirname "$0")/../.."
# /private/tmp es atravesable por nobody; el TMPDIR de root puede tener padres 0700.
work=$(mktemp -d /private/tmp/atenea-macos.XXXXXXXX)
smoke=''
watchdog=''
finish() {
  result=$?
  trap - EXIT INT TERM
  if [[ -n "$watchdog" ]]; then kill "$watchdog" 2>/dev/null || :; fi
  if [[ -n "$smoke" ]]; then kill -KILL "$smoke" 2>/dev/null || :; fi
  # Independiente del finally de Python: logs/estado y cleanup incluso tras un bloqueo.
  if ! python3 -u tests/macos/smoke.py --diagnostics "$work"; then
    echo 'Falló el diagnóstico o la limpieza (límite global 90s)' >&2
    if [[ $result == 0 ]]; then result=1; fi
  fi
  rm -rf "$work"
  exit "$result"
}
trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
python3 -u tests/macos/smoke.py "$work" &
smoke=$!
# Segunda barrera si ni siquiera se procesa la alarma/señal de Python (p. ej. spawn bloqueado).
(
  sleep 900
  kill -TERM "$smoke" 2>/dev/null || exit 0
  sleep 5
  kill -KILL "$smoke" 2>/dev/null || :
) </dev/null >/dev/null 2>&1 &
watchdog=$!
wait "$smoke"
smoke=''
