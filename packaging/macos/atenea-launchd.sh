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

install -d -o root -g wheel -m 0700 /var/run/atenea
"$bin/atenea-agent" cleanup --config "$config"
exec "$bin/atenea-agent" run --config "$config"
