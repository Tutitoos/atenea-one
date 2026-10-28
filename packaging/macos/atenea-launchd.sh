#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# launchd puede reintentar; nunca arrancar sin configuración privada explícita.
set -eu
config='/Library/Application Support/Atenea/agent.toml'
runtime=/var/run/atenea
argc=$#
uuid=${1:-}
network_root=${2:-'/Library/Application Support/Atenea/networks'}
[ "$argc" -le 2 ] || exit 78
if [ "$argc" = 0 ]; then [ -f "$config" ] || exit 0; fi
bin=/Library/Atenea/bin

# El agente corre como root: toda la ruta hasta el binario debe ser de root y sin escritura de
# grupo/otros (renombrar dentro de un directorio ajeno basta para sustituir el ejecutable).
for path in /Library /Library/Atenea "$bin" "$bin/atenea-agent" "$bin/atenea-launchd.sh"; do
    set -- $(/usr/bin/stat -L -f '%u %Sp' "$path")
    case "$1 $2" in
    '0 '?????-??-*) ;;
    *) echo "ruta insegura para ejecutar como root: $path" >&2; exit 78 ;;
    esac
done

if [ "$argc" != 0 ]; then
    helper="$bin/atenea-network.sh"
    [ ! -L "$helper" ] || exit 78
    set -- $(/usr/bin/stat -f '%u %Sp' "$helper")
    case "$1 $2" in '0 '?????-??-*) ;; *) exit 78 ;; esac
    . "$helper"
    network_paths "$uuid" "$network_root" || { echo 'UUID o raíz de red insegura' >&2; exit 78; }
    [ -f "$config" ] || exit 0
    # El padre compartido debe seguir siendo atravesable sin cambiar el legado.
    if [ ! -d /var/run/atenea ]; then install -d -o root -g wheel -m 0755 /var/run/atenea; fi
    [ ! -L /var/run/atenea ] || exit 78
    metadata=$(/usr/bin/stat -f '%u %Sp' /var/run/atenea)
    case "$metadata" in '0 '?????-??-*) ;; *) exit 78 ;; esac
    [ ! -L "$runtime" ] || exit 78
fi

# Con `status_gid` en la configuración, ese grupo atraviesa el directorio (0750) para llegar al socket
# de estado (solo lectura); sin él, solo root. El agente nunca cambia permisos del directorio.
gid=$(/usr/bin/awk -F= '$1 ~ /^[ \t]*status_gid[ \t]*$/ { gsub(/[ \t]/, "", $2); print $2; exit }' "$config")
case "$gid" in
'' | *[!0-9]*) install -d -o root -g wheel -m 0700 "$runtime" ;;
*) install -d -o root -g "$gid" -m 0750 "$runtime" ;;
esac
"$bin/atenea-agent" cleanup --config "$config"
exec "$bin/atenea-agent" run --config "$config"
