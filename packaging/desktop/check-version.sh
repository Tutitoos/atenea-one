#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Comprueba que el ejecutable de la aplicación imprime su versión con la forma que lee el agente (ADR-025):
# `atenea-desktop <versión> (<commit>)`, por la salida estándar (aquí una tubería, como la que le abre el agente) y sin
# abrir ventana. En Windows el binario es del subsistema gráfico: esta prueba demuestra que aun así escribe en la tubería.
# Uso: check-version.sh EJECUTABLE   (con las mismas ATENEA_VERSION y ATENEA_BUILD_COMMIT que build-binary.sh)
set -euo pipefail
if [ "$#" -ne 1 ]; then
    echo "Uso: $0 EJECUTABLE" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
version=${ATENEA_VERSION:-$(awk -F'"' '/^version = /{print $2; exit}' "$root/apps/cli/Cargo.toml")}
commit=${ATENEA_BUILD_COMMIT:-unknown}
want="atenea-desktop $version ($commit)"
got=$("$1" --version | tr -d '\r')
if [ "$got" != "$want" ]; then
    echo "La aplicación imprimió «$got» y se esperaba «$want»" >&2
    exit 1
fi
# La marca que busca el agente antes de ejecutar la aplicación (`DESKTOP_MARKER` en apps/agent/src/versions.rs): sin ella
# el agente no la ejecuta ni informa su versión.
marker=ATENEA-DESKTOP-VERSION-FLAG-1
if ! grep -aqF "$marker" "$1"; then
    echo "El binario no lleva la marca $marker que busca el agente" >&2
    exit 1
fi
echo "VERSIÓN DE LA APLICACIÓN: OK ($want; marca presente)"
