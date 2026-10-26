#!/usr/bin/env bash
# Horas decimales: 0.34 ensaya unos20min; 72 requiere piloto autorizado.
set -euo pipefail
[[ $# == 1 ]] || { echo 'uso: bash tests/load/soak.sh <horas>' >&2; exit 2; }
exec bash "$(dirname "$0")/stack.sh" soak "${1}h"
