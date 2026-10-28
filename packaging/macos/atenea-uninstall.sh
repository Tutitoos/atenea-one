#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Conserva claves y configuración; la eliminación de identidad requiere decisión explícita.
set -eu
[ "$(id -u)" = 0 ] || { echo 'Se requiere root' >&2; exit 1; }
# No interpretar plists ajenos ni cargar código desde una ruta sustituible.
for path in /Library /Library/Atenea /Library/Atenea/bin /Library/Atenea/bin/atenea-network.sh /Library/Atenea/bin/atenea-agent; do
    [ ! -L "$path" ] || exit 78
    set -- $(/usr/bin/stat -f '%u %Sp' "$path")
    case "$1 $2" in '0 '?????-??-*) ;; *) exit 78 ;; esac
done
. /Library/Atenea/bin/atenea-network.sh
for plist in /Library/LaunchDaemons/one.atenea.network.*.plist; do
    [ -e "$plist" ] || continue
    if ! network_plist "$plist"; then
        echo "No se toca un plist de red no reconocido: $plist" >&2
        continue
    fi
    launchctl bootout "system/$label" 2>/dev/null || :
    if [ -f "$config" ]; then
        /Library/Atenea/bin/atenea-agent cleanup --config "$config"
    fi
    rm -f "$plist"
    # Nunca recorrer un runtime sustituido ni borrar identidades.
    if [ -d "$runtime" ] && trusted_path "/private/var/run/atenea/$id8"; then
        rm -f "$runtime/control.sock" "$runtime/status.sock"
        rmdir "$runtime" 2>/dev/null || :
    fi
done
config='/Library/Application Support/Atenea/agent.toml'
launchctl bootout system/one.atenea.agent 2>/dev/null || :
if [ -f "$config" ]; then
    /Library/Atenea/bin/atenea-agent cleanup --config "$config"
fi
rm -f /Library/LaunchDaemons/one.atenea.agent.plist /etc/paths.d/atenea
rm -f /Library/Atenea/bin/atenea /Library/Atenea/bin/atenea-agent /Library/Atenea/bin/atenea-launchd.sh
rm -f /Library/Atenea/bin/atenea-network.sh
rm -f /var/run/atenea/control.sock /var/run/atenea/status.sock
rmdir /var/run/atenea 2>/dev/null || :
pkgutil --forget one.atenea.client >/dev/null 2>&1 || :
rm -f /Library/Atenea/bin/atenea-uninstall.sh
rmdir /Library/Atenea/bin /Library/Atenea 2>/dev/null || :
