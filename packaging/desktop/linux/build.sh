#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Empaqueta el binario de escritorio ya compilado en .deb y .rpm con `wails3 tool package` (nfpm).
# No compila ni descarga nada: `wails3` debe estar en el PATH con la versión que fija apps/desktop/go.mod.
set -euo pipefail
if [ "$#" -ne 4 ]; then
    echo "Uso: $0 BINARIO VERSION amd64|arm64 DIRECTORIO_SALIDA" >&2
    exit 2
fi
binary=$1
version=$2
arch=$3
out=$4
commit=${ATENEA_BUILD_COMMIT:-unknown}
[ -x "$binary" ] || { echo "Falta el ejecutable $binary" >&2; exit 1; }
[[ "$commit" =~ ^([0-9a-f]{40}|unknown)$ ]] || { echo 'Commit inválido' >&2; exit 2; }
case "$arch" in amd64 | arm64) ;; *) echo 'Arquitectura no admitida' >&2; exit 2 ;; esac
[[ "$version" =~ ^[0-9][a-zA-Z0-9.+~\-]*$ ]] || { echo 'Versión inválida' >&2; exit 2; }
root=$(cd "$(dirname "$0")/../../.." && pwd)
want=$(awk '/wailsapp\/wails\/v3 v/ {print $NF; exit}' "$root/apps/desktop/go.mod")
have=$(wails3 version 2>&1 | tail -1 | tr -d '[:space:]')
[ "$have" = "$want" ] || { echo "wails3 $have no coincide con go.mod ($want)" >&2; exit 1; }
mkdir -p "$out"
out=$(cd "$out" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
install -m 0755 "$binary" "$stage/atenea-desktop"
install -m 0644 "$root/packaging/desktop/linux/atenea-desktop.desktop" "$stage/atenea-desktop.desktop"
install -m 0644 "$root/apps/desktop/icons/icon.png" "$stage/atenea-desktop.png"
export ATENEA_ARCH=$arch ATENEA_VERSION=$version ATENEA_BUILD_COMMIT=$commit
for format in deb rpm; do
    (cd "$stage" && wails3 tool package -format "$format" -config "$root/packaging/desktop/linux/nfpm.yaml" -name atenea-desktop -out "$stage")
done
mv "$stage/atenea-desktop.deb" "$out/atenea-desktop-${version}-linux-${arch}.deb"
mv "$stage/atenea-desktop.rpm" "$out/atenea-desktop-${version}-linux-${arch}.rpm"
printf '%s\n' "$out/atenea-desktop-${version}-linux-${arch}.deb" "$out/atenea-desktop-${version}-linux-${arch}.rpm"
