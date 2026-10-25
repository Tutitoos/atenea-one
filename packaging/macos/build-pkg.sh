#!/bin/sh
# Solo construye: no instala, no crea interfaces y no requiere sudo.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
bins=${ATENEA_BIN_DIR:-"$repo/target/release"}
out="$repo/target/macos-package"
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
root="$work/root"
# Lo que se ejecuta como root va bajo /Library (root:wheel), nunca bajo /usr/local (puede ser de un usuario).
mkdir -p "$root/Library/Atenea/bin" "$root/private/etc/paths.d" "$root/Library/LaunchDaemons" "$work/scripts"
for bin in atenea atenea-agent; do
    install -m 0755 "$bins/$bin" "$root/Library/Atenea/bin/$bin"
done
install -m 0755 "$repo/packaging/macos/atenea-uninstall.sh" "$root/Library/Atenea/bin/atenea-uninstall.sh"
install -m 0755 "$repo/packaging/macos/atenea-launchd.sh" "$root/Library/Atenea/bin/atenea-launchd.sh"
install -m 0644 "$repo/packaging/macos/one.atenea.agent.plist" "$root/Library/LaunchDaemons/one.atenea.agent.plist"
install -m 0755 "$repo/packaging/macos/postinstall" "$work/scripts/postinstall"
install -m 0644 /dev/stdin "$root/private/etc/paths.d/atenea" <<'EOF'
/Library/Atenea/bin
EOF
mkdir -p "$root/usr/local/share/atenea"
python3 "$repo/scripts/release/build-info.py" > "$root/usr/local/share/atenea/BUILD-INFO.json"
pkgbuild --root "$root" --scripts "$work/scripts" --identifier one.atenea.client --version "${ATENEA_VERSION:-0.1.0}" --install-location / --ownership recommended "$work/component.pkg"
productbuild --package "$work/component.pkg" "$out/atenea-client-unsigned.pkg"
printf '%s\n' "$out/atenea-client-unsigned.pkg"
