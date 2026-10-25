#!/bin/sh
# launchd puede reintentar; nunca arrancar sin configuración privada explícita.
set -eu
config='/Library/Application Support/Atenea/agent.toml'
[ -f "$config" ] || exit 0
install -d -o root -g wheel -m 0700 /var/run/atenea
/usr/local/bin/atenea-agent cleanup --config "$config"
exec /usr/local/bin/atenea-agent run --config "$config"
