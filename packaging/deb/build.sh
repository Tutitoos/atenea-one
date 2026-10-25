#!/usr/bin/env bash
# Empaqueta binarios Linux ya compilados; no compila ni descarga nada.
set -euo pipefail
if [ "$#" -ne 4 ]; then
    echo "Uso: $0 DIRECTORIO_BINARIOS VERSION amd64|arm64 SALIDA.deb" >&2
    exit 2
fi
bin=$(cd "$1" && pwd)
version=$2
arch=$3
commit=${ATENEA_BUILD_COMMIT:-unknown}
[[ "$commit" =~ ^([0-9a-f]{40}|unknown)$ ]] || { echo 'Commit inválido' >&2; exit 2; }
output=$4
case "$arch" in amd64|arm64) ;; *) echo 'Arquitectura no admitida' >&2; exit 2 ;; esac
[[ "$version" =~ ^[0-9][a-zA-Z0-9.+:~\-]*$ ]] || { echo 'Versión Debian inválida' >&2; exit 2; }
for name in atenea atenea-agent; do
    [ -x "$bin/$name" ] || { echo "Falta el ejecutable $bin/$name" >&2; exit 1; }
done
root=$(cd "$(dirname "$0")/../.." && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
install -d "$stage/DEBIAN" "$stage/usr/bin" "$stage/usr/lib/systemd/system" "$stage/usr/share/doc/atenea"
install -m 0755 "$bin/atenea" "$bin/atenea-agent" "$stage/usr/bin/"
install -m 0644 "$root/packaging/systemd/atenea-agent.service" "$stage/usr/lib/systemd/system/"
install -m 0644 "$root/packaging/deb/agent.toml.example" "$root/packaging/deb/99-atenea-router.conf.example" "$stage/usr/share/doc/atenea/"
chmod 0755 "$stage"
install -m 0644 "$root/docs/operations/instalacion-linux.md" "$root/docs/operations/router-de-subred.md" "$stage/usr/share/doc/atenea/"
for script in postinst prerm postrm; do
    install -m 0755 "$root/packaging/deb/$script" "$stage/DEBIAN/"
done
cat > "$stage/DEBIAN/control" <<EOF
Package: atenea
Version: $version
Architecture: $arch
X-Atenea-Commit: $commit
Maintainer: Equipo Atenea One <maintainers@example.invalid>
Section: net
Priority: optional
Depends: libc6 (>= 2.39), libgcc-s1, libssl3t64, systemd, passwd, nftables, iproute2, ca-certificates
Recommends: systemd-resolved
Description: Agente y CLI de Atenea One
 Red privada con configuración firmada, rutas aprobadas y DNS dividido.
EOF
dpkg-deb --root-owner-group --compression=xz --build "$stage" "$output"
