#!/usr/bin/env bash
# Requiere imagen de herramientas preaprovisionada; jamás construye ni descarga.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
name="atenea-infra-$$"
image=${INFRA_IMAGE:-atenea-infra:local}
start=$SECONDS
cleanup() {
    rc=$?
    trap - EXIT
    if (( rc != 0 )) && [[ ${KEEP_INFRA:-0} == 1 ]]; then
        echo "Conservado para diagnóstico: $name (KEEP_INFRA=1); código=$rc"
        exit "$rc"
    fi
    if docker container inspect "$name" >/dev/null 2>&1; then
        if ! docker rm -fv "$name"; then
            echo "ERROR: no se pudo eliminar $name; revisar sus volúmenes anónimos" >&2
            rc=1
        fi
    fi
    echo "Ensayo terminado: $((SECONDS-start)) s; código=$rc; red externa deshabilitada; imagen de herramientas preexistente conservada"
    exit "$rc"
}
trap cleanup EXIT
[[ $(docker info --format '{{.CgroupVersion}}') == 2 ]] || { echo 'Se requiere cgroup v2' >&2; exit 1; }
docker image inspect "$image" >/dev/null 2>&1 || {
    echo "Falta $image: preaprovisionar tests/infra/Dockerfile con autorización de red; no se descarga automáticamente." >&2
    exit 1
}
# Un digest sin etiqueta no se importa en el Docker anidado: se etiqueta (solo metadatos locales) antes de save.
docker tag haproxy@sha256:5893840576071e6217cd52ebdcc14f454c6f7a631056fa6f4e0f40d3cac4215b atenea-infra-pin/haproxy:pinned
docker tag caddy@sha256:4c6e91c6ed0e2fa03efd5b44747b625fec79bc9cd06ac5235a779726618e530d atenea-infra-pin/caddy:pinned
images=(atenea/control-plane:local atenea/coordinator:local atenea/relay:local atenea/tools:local postgres:17
    atenea-infra-pin/haproxy:pinned
    atenea-infra-pin/caddy:pinned
    ghcr.io/dexidp/dex:v2.41.1)
docker image inspect "${images[@]}" >/dev/null
docker run -d --pull never --network none --name "$name" --privileged --cgroupns=host \
    --tmpfs /run --tmpfs /run/lock --tmpfs /tmp \
    -v /sys/fs/cgroup:/sys/fs/cgroup:rw -v "$root:/src:ro" "$image" >/dev/null
for ((i=0; i<60; i++)); do
    if docker exec "$name" systemctl show --property=Version --value >/dev/null 2>&1; then break; fi
    sleep 1
done
docker exec "$name" systemctl start docker
# save/load transfiere exclusivamente bytes locales, sin registro ni socket del host.
docker save "${images[@]}" | docker exec -i "$name" docker load
for service in control-plane coordinator relay tools postgres haproxy caddy dex; do
    case "$service" in
        control-plane|coordinator|relay|tools) ref="atenea/$service:local" ;;
        postgres) ref=postgres:17 ;;
        haproxy) ref=${images[5]} ;;
        caddy) ref=${images[6]} ;;
        dex) ref=${images[7]} ;;
    esac
    # El ID del Docker anidado (almacén clásico) no coincide con el del host (containerd): se consulta dentro.
    printf '%s %s\n' "$service" "$(docker exec "$name" docker image inspect "$ref" --format '{{.Id}}')"
done | docker exec -i "$name" sh -c 'cat > /tmp/infra-images'
docker exec "$name" bash /src/tests/infra/in-container.sh
