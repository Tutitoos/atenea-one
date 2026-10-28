#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Requiere Docker Linux con cgroup v2; no ejecutar sobre el host Proxmox.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
if [ "${1:-}" = "--release" ]; then
    exec bash "$root/tests/release/install-deb.sh" "$2" "$3"
fi
name="atenea-packaging-$$"
image=${ATENEA_PACKAGING_IMAGE:-atenea-packaging}
cleanup() {
    docker rm -f "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT
[ "$(docker info --format '{{.CgroupVersion}}')" = 2 ] || { echo 'Se requiere cgroup v2' >&2; exit 1; }
# Reutiliza la imagen/volumen del laboratorio; no instala toolchains en el anfitrión.
docker image inspect atenea-lab >/dev/null
docker image inspect "$image" >/dev/null
docker volume inspect atenea-lab-target >/dev/null
docker volume inspect atenea-cargo-registry >/dev/null
docker run --rm --pull=never --network none -e CARGO_BUILD_JOBS=2 -v "$root:/src:ro" -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/cache/target -w /src atenea-lab:latest \
    cargo build --release --locked --offline -p atenea-cli -p atenea-agent
docker run -d --pull=never --network none --name "$name" --privileged --cgroupns=host \
    --tmpfs /run --tmpfs /run/lock --tmpfs /tmp \
    -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
    -e ATENEA_PACKAGING_NO_DNS="${ATENEA_PACKAGING_NO_DNS:-0}" \
    -v "$root:/src:ro" -v atenea-lab-target:/cache:ro "$image" /lib/systemd/systemd >/dev/null
for ((i=0; i<60; i++)); do
    if docker exec "$name" systemctl show --property=Version --value >/dev/null 2>&1; then
        break
    fi
    sleep 1
done
docker exec "$name" bash /src/tests/packaging/in-container.sh
