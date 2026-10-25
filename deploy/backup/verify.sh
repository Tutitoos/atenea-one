#!/bin/sh
# pgBackRest 2.59 verify puede devolver 0 con status: error; examinar también el informe.
set -eu
report=$(mktemp)
trap 'rm -f "$report"' EXIT HUP INT TERM
if ! pgbackrest --stanza=atenea "$@" verify > "$report" 2>&1; then
  cat "$report"
  exit 1
fi
cat "$report"
if grep -Eq 'status: (error|invalid)|invalid checksum|missing file' "$report"; then
  exit 1
fi
