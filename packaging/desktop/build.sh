#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Empaqueta la aplicación de escritorio para el sistema operativo donde se ejecuta y escribe en SALIDA los paquetes,
# su inventario Go (SBOM CycloneDX) y su identidad (`desktop-<SO>.build-info.json`). Es el mismo script para CI y release.
#
#   Linux:   atenea-desktop-<versión>-linux-<arq>.deb y .rpm   (necesita `wails3` en el PATH)
#   macOS:   atenea-desktop-<versión>-macos-<arq>.dmg con «Atenea One.app» (firma ad hoc)
#   Windows: atenea-desktop-<versión>-windows-<arq>.exe (NSIS, por usuario; necesita `makensis` y `pwsh`)
#
# Entorno: ATENEA_VERSION y ATENEA_BUILD_COMMIT (obligatorios). ATENEA_DESKTOP_BINARY usa un binario ya compilado
# (y probado) en vez de compilar. La interfaz (`apps/desktop/build/client`) debe estar compilada antes.
# Los paquetes salen SIN firma de desarrollador ni notarización.
set -euo pipefail
if [ "$#" -ne 1 ]; then
    echo "Uso: $0 DIRECTORIO_SALIDA" >&2
    exit 2
fi
: "${ATENEA_VERSION:?Falta ATENEA_VERSION}" "${ATENEA_BUILD_COMMIT:?Falta ATENEA_BUILD_COMMIT}"
root=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$1"
out=$(cd "$1" && pwd)
case "$(uname -s)" in
    Linux) system=Linux ;;
    Darwin) system=macOS ;;
    MINGW* | MSYS* | CYGWIN*) system=Windows ;;
    *) echo "Sistema no admitido: $(uname -s)" >&2; exit 2 ;;
esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
binary=${ATENEA_DESKTOP_BINARY:-}
if [ -z "$binary" ]; then
    binary="$work/atenea-desktop"
    [ "$system" = Windows ] && binary="$binary.exe"
    "$root/packaging/desktop/build-binary.sh" "$binary"
fi
[ -f "$binary" ] || { echo "Falta el binario $binary" >&2; exit 1; }
case "$system" in
    Linux) "$root/packaging/desktop/linux/build.sh" "$binary" "$ATENEA_VERSION" "$(dpkg --print-architecture)" "$out" ;;
    macOS) "$root/packaging/desktop/macos/build-dmg.sh" "$binary" "$ATENEA_VERSION" "$out" ;;
    Windows) pwsh -NoProfile -File "$root/packaging/desktop/windows/build-installer.ps1" -Binary "$binary" -Version "$ATENEA_VERSION" -OutDir "$out" ;;
esac
# Inventario: los módulos que se enlazan en este binario para este sistema (`go list` no descarga nada si hay caché).
(cd "$root/apps/desktop" && go list -deps -tags production -f '{{with .Module}}{{if not .Main}}{{.Path}}	{{.Version}}{{end}}{{end}}' .) | awk 'NF' | sort -u > "$work/golist.txt"
python3 "$root/scripts/release/sbom.py" go "$work/golist.txt" "$out/go-desktop-$system.cdx.json" --version "$ATENEA_VERSION" \
    --commit "$ATENEA_BUILD_COMMIT" --target "$(cd "$root/apps/desktop" && go env GOOS GOARCH | paste -sd/ -)" --gosum "$root/apps/desktop/go.sum"
python3 "$root/scripts/release/build-info.py" > "$out/desktop-$system.build-info.json"
