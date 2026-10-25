#!/bin/bash
# SOLO runner macOS desechable: modifica red y /etc/resolver como root.
set -euo pipefail
[[ $(uname -s) == Darwin && $(id -u) == 0 && ${GITHUB_ACTIONS:-} == true ]] || {
  echo 'Solo se ejecuta como root en una VM macOS de GitHub Actions' >&2; exit 1;
}
cd "$(dirname "$0")/../.."
python3 tests/macos/smoke.py
