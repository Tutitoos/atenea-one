#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Instala en Linux las dependencias de sistema de la aplicación de escritorio (WebKitGTK, xvfb, dbus) en la CI y en
# el workflow de release. Uso: scripts/install-linux-desktop-deps.sh [DIRECTORIO_CACHE_APT]
#
# - Cada orden de apt tiene plazo y las descargas se reintentan; el conjunto se repite hasta 3 veces.
# - Con un directorio de caché, apt guarda y reutiliza allí los .deb (la CI lo conserva con actions/cache). apt
#   comprueba cada .deb contra los hashes del índice firmado antes de usarlo, así que un fichero alterado no se instala.
# - Mientras corre, una muestra cada 10 s (memoria, disco, carga y procesos en estado D) va al log. El paso se colgó
#   hasta 29 min sin dejar rastro y los plazos de dentro del runner no lo cortaron (indica un cuelgue del propio
#   equipo): esas líneas, junto con el último instante que llegue al log, son lo que permite diagnosticarlo.
set -eu

packages="libgtk-4-dev libwebkitgtk-6.0-dev libwayland-dev build-essential pkg-config xvfb x11-xserver-utils
    at-spi2-core xdg-desktop-portal-gtk dbus-x11"
cache=${1:-}

set -- -o Acquire::Retries=3 -o Acquire::http::Timeout=30
if [ -n "$cache" ]; then
    mkdir -p "$cache/partial"
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

echo "[diag] inicio: $(date +%T), caché de apt: ${cache:-sin caché}"
for attempt in 1 2 3; do
    # shellcheck disable=SC2086 # la lista de paquetes se separa por palabras a propósito
    if sudo timeout 90 apt-get "$@" update -qq &&
        sudo timeout 240 apt-get "$@" install -y --no-install-recommends $packages; then
        # La caché la guarda el usuario del runner, no root.
        [ -z "$cache" ] || sudo chown -R "$(id -u):$(id -g)" "$cache"
        echo "[diag] listo en intento $attempt: $(date +%T)"
        exit 0
    fi
    echo "::warning::apt falló o superó el plazo (intento $attempt de 3)"
    sleep 10
done
exit 1
