#!/bin/sh
# Conserva claves y configuración; la eliminación de identidad requiere decisión explícita.
set -eu
[ "$(id -u)" = 0 ] || { echo 'Se requiere root' >&2; exit 1; }
config='/Library/Application Support/Atenea/agent.toml'
launchctl bootout system/one.atenea.agent 2>/dev/null || :
if [ -f "$config" ]; then
    /Library/Atenea/bin/atenea-agent cleanup --config "$config"
fi
rm -f /Library/LaunchDaemons/one.atenea.agent.plist /etc/paths.d/atenea
rm -f /Library/Atenea/bin/atenea /Library/Atenea/bin/atenea-agent /Library/Atenea/bin/atenea-launchd.sh
rm -f /var/run/atenea/control.sock /var/run/atenea/status.sock
rmdir /var/run/atenea 2>/dev/null || :
pkgutil --forget one.atenea.client >/dev/null
rm -f /Library/Atenea/bin/atenea-uninstall.sh
rmdir /Library/Atenea/bin /Library/Atenea 2>/dev/null || :
