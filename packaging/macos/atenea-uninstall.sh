#!/bin/sh
# Conserva claves y configuración; la eliminación de identidad requiere decisión explícita.
set -eu
[ "$(id -u)" = 0 ] || { echo 'Se requiere root' >&2; exit 1; }
config='/Library/Application Support/Atenea/agent.toml'
launchctl bootout system/one.atenea.agent 2>/dev/null || :
if [ -f "$config" ]; then
    /usr/local/bin/atenea-agent cleanup --config "$config"
fi
rm -f /Library/LaunchDaemons/one.atenea.agent.plist /usr/local/libexec/atenea-launchd.sh
rm -f /usr/local/bin/atenea /usr/local/bin/atenea-agent
rm -f /var/run/atenea/control.sock /var/run/atenea/status.sock
rmdir /var/run/atenea 2>/dev/null || :
pkgutil --forget one.atenea.client >/dev/null
rm -f /usr/local/bin/atenea-uninstall.sh
