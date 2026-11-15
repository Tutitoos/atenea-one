#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Compila la aplicación de escritorio para el sistema donde se ejecuta (CGO: se enlaza con las bibliotecas nativas,
# así que no hay compilación cruzada). Los mismos indicadores para CI, release y pruebas locales.
# La interfaz debe estar compilada antes (`pnpm --filter @atenea/desktop run build`): se embebe en el binario.
set -euo pipefail
if [ "$#" -ne 1 ]; then
    echo "Uso: $0 SALIDA" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
[ -f "$root/apps/desktop/build/client/index.html" ] || { echo 'Falta la interfaz compilada (apps/desktop/build/client)' >&2; exit 1; }
ldflags='-s -w'
# Versión y commit (ADR-025: el agente del equipo los lee con `--version`). Sin ATENEA_VERSION, la del cliente en el
# árbol; sin ATENEA_BUILD_COMMIT, `unknown` (como los binarios de Rust). Se validan: van a una línea de ldflags.
version=${ATENEA_VERSION:-$(awk -F'"' '/^version = /{print $2; exit}' "$root/apps/cli/Cargo.toml")}
commit=${ATENEA_BUILD_COMMIT:-unknown}
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || { echo "Versión inválida: $version" >&2; exit 1; }
[[ "$commit" =~ ^([0-9a-f]{7,40}|unknown)$ ]] || { echo "Commit inválido: $commit" >&2; exit 1; }
pkg=github.com/Tutitoos/atenea-one/apps/desktop/internal/buildinfo
ldflags="$ldflags -X $pkg.Version=$version -X $pkg.Commit=$commit"
# En Windows, sin esto la aplicación abre una consola junto a la ventana.
if [ "$(cd "$root/apps/desktop" && go env GOOS)" = windows ]; then ldflags="$ldflags -H=windowsgui"; fi
out=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
cd "$root/apps/desktop"
CGO_ENABLED=1 go build -tags production -trimpath -buildvcs=false -ldflags "$ldflags" -o "$out" .
