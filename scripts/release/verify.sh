#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Sin --published, DIST es el dist/ completo del release. Con --published, DIST es lo que se descarga del release publicado
# (instaladores, SHA256SUMS, manifiesto y firma): se verifica lo presente contra el manifiesto firmado.
set -euo pipefail
expected=4
if [ "${1:-}" = "--published" ]; then
    expected=5
fi
if [ "$#" -ne "$expected" ]; then
    echo "Uso: $0 [--published] DIST CLAVE_PUBLICA COMMIT_ESPERADO VERSION_ESPERADA" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
export PYTHONDONTWRITEBYTECODE=1
exec python3 "$root/scripts/release/manifest.py" verify "$@"
