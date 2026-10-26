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
image="atenea-packaging:$$"
cleanup() {
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker image rm "$image" >/dev/null 2>&1 || true
}
trap cleanup EXIT
[ "$(docker info --format '{{.CgroupVersion}}')" = 2 ] || { echo 'Se requiere cgroup v2' >&2; exit 1; }
# Reutiliza la imagen/volumen del laboratorio; no instala toolchains en el anfitrión.
docker image inspect atenea-lab >/dev/null
docker volume inspect atenea-lab-target >/dev/null
docker run --rm -v "$root:/src:ro" -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/cache/target -w /src atenea-lab \
    cargo build --release --locked -p atenea-cli -p atenea-agent
docker build -t "$image" "$root/tests/packaging"
docker run -d --name "$name" --privileged --cgroupns=host \
    --tmpfs /run --tmpfs /run/lock --tmpfs /tmp \
    -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
    -v "$root:/src:ro" -v atenea-lab-target:/cache:ro "$image" >/dev/null
for ((i=0; i<60; i++)); do
    if docker exec "$name" systemctl show --property=Version --value >/dev/null 2>&1; then
        break
    fi
    sleep 1
done
docker exec "$name" bash /src/tests/packaging/in-container.sh
