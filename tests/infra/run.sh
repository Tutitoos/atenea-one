#!/usr/bin/env bash
# Solo Docker efímero; no instala herramientas ni monta el socket Docker del anfitrión.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
name="atenea-infra-$$"
image="atenea-infra:$$"
cleanup() {
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker image rm "$image" >/dev/null 2>&1 || true
}
trap cleanup EXIT
[[ $(docker info --format '{{.CgroupVersion}}') == 2 ]] || { echo 'Se requiere cgroup v2' >&2; exit 1; }
docker build -t "$image" "$root/tests/infra"
docker run -d --name "$name" --privileged --cgroupns=host     --tmpfs /run --tmpfs /run/lock --tmpfs /tmp     -v /sys/fs/cgroup:/sys/fs/cgroup:rw -v "$root:/src:ro" "$image" >/dev/null
for ((i=0; i<60; i++)); do
    if docker exec "$name" systemctl show --property=Version --value >/dev/null 2>&1; then break; fi
    sleep 1
done
docker exec "$name" bash /src/tests/infra/in-container.sh
