#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Comprueba el paquete que `build.sh` dejó en DIRECTORIO instalándolo de verdad y arrancando la aplicación instalada con la
# prueba de humo. SOLO para un runner desechable o un equipo de pruebas: Linux instala y desinstala con apt (sudo) y
# Windows toca el perfil y HKCU; macOS solo monta el DMG en solo lectura.
set -euo pipefail
if [ "$#" -ne 1 ]; then
    echo "Uso: $0 DIRECTORIO" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
dir=$(cd "$1" && pwd)
smoke="$root/apps/desktop/scripts/smoke.py"
case "$(uname -s)" in
    Linux)
        deb=$(ls "$dir"/atenea-desktop-*-linux-*.deb)
        sudo apt-get install -y --no-install-recommends "$deb"
        for f in /usr/bin/atenea-desktop /usr/share/applications/atenea-desktop.desktop /usr/share/icons/hicolor/256x256/apps/atenea-desktop.png; do
            [ -f "$f" ] || { echo "Falta $f tras instalar" >&2; exit 1; }
        done
        grep -q '^Exec=atenea-desktop' /usr/share/applications/atenea-desktop.desktop
        # El runner no tiene bandeja ni sandbox de usuario: mismas variables que la prueba de humo de CI.
        WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1 \
            python3 "$smoke" 120 xvfb-run -a dbus-run-session -- /usr/bin/atenea-desktop
        sudo apt-get remove -y atenea-desktop
        [ ! -e /usr/bin/atenea-desktop ] || { echo 'El binario sigue instalado tras desinstalar' >&2; exit 1; }
        ;;
    Darwin)
        dmg=$(ls "$dir"/atenea-desktop-*-macos-*.dmg)
        mount=$(mktemp -d)
        trap 'hdiutil detach -quiet "$mount" || true' EXIT
        hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$dmg"
        app="$mount/Atenea One.app"
        plutil -lint "$app/Contents/Info.plist"
        codesign --verify --deep --strict "$app"
        python3 "$smoke" 120 "$app/Contents/MacOS/atenea-desktop"
        ;;
    MINGW* | MSYS* | CYGWIN*)
        pwsh -NoProfile -File "$root/packaging/desktop/windows/check-installer.ps1" -Installer "$(cygpath -w "$(ls "$dir"/atenea-desktop-*-windows-*.exe)")"
        ;;
    *) echo "Sistema no admitido: $(uname -s)" >&2; exit 2 ;;
esac
echo "PAQUETE: OK ($(uname -s))"
