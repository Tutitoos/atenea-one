#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Instala en Linux las dependencias de sistema de la aplicación de escritorio (WebKitGTK, xvfb, dbus) en la CI y en
# el workflow de release. Uso: scripts/install-linux-desktop-deps.sh [DIRECTORIO_CACHE_APT]
#
# - Cada orden de apt tiene plazo y las descargas se reintentan; el conjunto se repite hasta 3 veces (3 x 250 s como
#   máximo, dentro del tope de 15 min del paso). A partir del segundo intento se usa archive.ubuntu.com en vez del
#   espejo que la imagen del runner haya elegido (la lista /etc/apt/apt-mirrors.txt varía de un runner a otro:
#   azure.archive.ubuntu.com o us-east-2.ec2.archive.ubuntu.com). Dos pasos fallaron seguidos (2026-11-19) con el espejo
#   de EC2: `apt-get update` pasó de 60 s y las descargas de los paquetes amd64v3 quedaron en `Ign`, mientras otros
#   runners con el espejo de Azure terminaban en segundos; el cambio anterior solo reconocía Azure y no hacía nada con
#   EC2, así que los tres intentos usaban el mismo espejo roto. Mismo índice y mismas firmas: apt verifica los paquetes
#   igual.
# - Con un directorio de caché, apt guarda y reutiliza allí los .deb (la CI lo conserva con actions/cache). apt
#   comprueba cada .deb contra los hashes del índice firmado antes de usarlo, así que un fichero alterado no se instala.
# - Mientras corre, una muestra cada 10 s (memoria, disco, carga y procesos en estado D) va al log; permite distinguir
#   un espejo lento de un equipo colgado.
set -eu

packages="libgtk-4-dev libwebkitgtk-6.0-dev libwayland-dev build-essential pkg-config xvfb x11-xserver-utils
    at-spi2-core xdg-desktop-portal-gtk dbus-x11"
cache=${1:-}

set -- -o Acquire::Retries=3 -o Acquire::http::Timeout=30
if [ -n "$cache" ]; then
    mkdir -p "$cache/partial"
    # Las descargas las hace el usuario _apt (sandbox de apt); sin esto avisa de que descarga como root.
    sudo chown _apt "$cache/partial" 2>/dev/null || true
    set -- "$@" -o "Dir::Cache::archives=$cache"
fi

sample() {
    while :; do
        {
            printf '[diag %s] ' "$(date +%T)"
            free -m | awk '/^Mem:/ {printf "memoria libre %s MB, disponible %s MB; ", $4, $7} /^Swap:/ {printf "swap usada %s MB; ", $3}'
            df -m / | awk 'NR == 2 {printf "disco libre %s MB; ", $4}'
            printf 'carga %s; ' "$(cut -d' ' -f1-3 /proc/loadavg)"
            ps -eo stat=,comm= | awk '$1 ~ /^D/ {d = d " " $2} END {print "estado D:" (d == "" ? " ninguno" : d)}'
        } || true
        sleep 10
    done
}
sample &
sampler=$!
trap 'kill "$sampler" 2>/dev/null || true' EXIT HUP INT TERM

# Respaldo del espejo regional; idempotente y sin efecto si el equipo ya usa archive.ubuntu.com. En las imágenes de los
# runners el espejo no está en ubuntu.sources (que apunta a `mirror+file:/etc/apt/apt-mirrors.txt`) sino en ese fichero.
# Reconoce cualquier `<región>.archive.ubuntu.com`; `security.ubuntu.com` no se toca.
use_canonical_mirror() {
    for f in /etc/apt/apt-mirrors.txt /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list; do
        [ -f "$f" ] || continue
        if grep -Eq '//([a-z0-9-]+\.)+archive\.ubuntu\.com/' "$f"; then
            echo "[diag] cambio de espejo en $f: $(grep -Eo '//([a-z0-9-]+\.)+archive\.ubuntu\.com/' "$f" | sort -u | tr '\n' ' ')-> //archive.ubuntu.com/"
            sudo sed -E -i 's|//([a-z0-9-]+\.)+archive\.ubuntu\.com/|//archive.ubuntu.com/|g' "$f"
        fi
    done
}

echo "[diag] inicio: $(date +%T), caché de apt: ${cache:-sin caché}"
for attempt in 1 2 3; do
    [ "$attempt" -lt 2 ] || use_canonical_mirror
    # shellcheck disable=SC2086 # la lista de paquetes se separa por palabras a propósito
    if sudo timeout 60 apt-get "$@" update -qq &&
        sudo timeout 180 apt-get "$@" install -y --no-install-recommends $packages; then
        # La caché la guarda el usuario del runner, no root.
        [ -z "$cache" ] || sudo chown -R "$(id -u):$(id -g)" "$cache"
        echo "[diag] listo en intento $attempt: $(date +%T)"
        exit 0
    fi
    echo "::warning::apt falló o superó el plazo (intento $attempt de 3)"
    sleep 10
done
exit 1
