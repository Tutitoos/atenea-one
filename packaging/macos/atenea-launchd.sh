#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# launchd puede reintentar; nunca arrancar sin configuración privada explícita.
set -eu
config='/Library/Application Support/Atenea/agent.toml'
[ -f "$config" ] || exit 0
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

# Con `status_gid` en la configuración, ese grupo atraviesa el directorio (0750) para llegar al socket
# de estado (solo lectura); sin él, solo root. El agente nunca cambia permisos del directorio.
gid=$(/usr/bin/awk -F= '$1 ~ /^[ \t]*status_gid[ \t]*$/ { gsub(/[ \t]/, "", $2); print $2; exit }' "$config")
case "$gid" in
'' | *[!0-9]*) install -d -o root -g wheel -m 0700 /var/run/atenea ;;
*) install -d -o root -g "$gid" -m 0750 /var/run/atenea ;;
esac
"$bin/atenea-agent" cleanup --config "$config"
exec "$bin/atenea-agent" run --config "$config"
