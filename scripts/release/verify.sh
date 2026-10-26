#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
if [ "$#" -ne 4 ]; then
    echo "Uso: $0 DIST CLAVE_PUBLICA COMMIT_ESPERADO VERSION_ESPERADA" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
export PYTHONDONTWRITEBYTECODE=1
exec python3 "$root/scripts/release/manifest.py" verify "$@"
