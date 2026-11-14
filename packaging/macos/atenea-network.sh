#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Solo se carga tras comprobar la cadena de confianza de /Library/Atenea/bin.
trusted_path() {
    checked=$1
    while :; do
        [ ! -L "$checked" ] || return 1
        metadata=$(/usr/bin/stat -f '%u %Sp' "$checked") || return 1
        case "$metadata" in '0 '?????-??-*) ;; *) return 1 ;; esac
        [ "$checked" != / ] || break
        checked=${checked%/*}
        [ -n "$checked" ] || checked=/
    done
}
# Directorio real (no enlace) del propietario indicado y no escribible por grupo ni por otros. Es la regla que el lanzador
# aplica a /var/run/atenea: ahí no sirve `trusted_path`, que sube hasta / y en macOS encuentra /private/var/run con
# root:daemon 0775. El uid es parámetro solo para poder probarlo sin root.
trusted_dir() {
    [ ! -L "$1" ] && [ -d "$1" ] || return 1
    case "$(/usr/bin/stat -f '%u %Sp' "$1")" in "${2:-0} "d????-??-*) ;; *) return 1 ;; esac
}
network_paths() {
    network=$1
    case "$network" in
    ????????-????-????-????-????????????) ;;
    *) return 1 ;;
    esac
    case "$network" in *[!0123456789abcdef-]*) return 1 ;; esac
    # Los guiones solo pueden ocupar las cuatro posiciones canónicas.
    hex=$(printf '%s' "$network" | /usr/bin/tr -d '-')
    [ ${#hex} = 32 ] || return 1
    root=${2:-'/Library/Application Support/Atenea/networks'}
    case "$root" in /*) ;; *) return 1 ;; esac
    case "$root/" in *//*|*/./*|*/../*) return 1 ;; esac
    trusted_path "$root" || return 1
    [ -d "$root" ] || return 1
    id8=${network##*-}
    id8=$(printf '%s' "$id8" | /usr/bin/cut -c 5-12)
    config="$root/$network/device/agent.toml"
    runtime="/var/run/atenea/$id8"
    # Si aún no existe configuración no se arranca, pero tampoco se sigue un symlink.
    if [ -e "$config" ] || [ -L "$config" ]; then
        trusted_path "$config" || return 1
    fi
}
network_plist() {
    plist=$1
    trusted_path "$plist" || return 1
    label=$(/usr/libexec/PlistBuddy -c 'Print :Label' "$plist") || return 1
    launcher=$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist") || return 1
    [ "$launcher" = /Library/Atenea/bin/atenea-launchd.sh ] || return 1
    uuid=$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:1' "$plist") || return 1
    network_root=$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:2' "$plist" 2>/dev/null) || network_root='/Library/Application Support/Atenea/networks'
    if /usr/libexec/PlistBuddy -c 'Print :ProgramArguments:3' "$plist" >/dev/null 2>&1; then return 1; fi
    network_paths "$uuid" "$network_root" || return 1
    [ "$label" = "one.atenea.network.$id8" ] && [ "$plist" = "/Library/LaunchDaemons/$label.plist" ]
}
# Directorio de runtime compartido (/var/run/atenea): lo crea con 0755 SOLO si falta y nunca cambia los permisos de uno
# que ya existe. Esos permisos los fijan los lanzadores (el heredado lo deja 0700 o 0750 según `status_gid`; el
# de una red necesita el padre atravesable) y el kernel los hace efectivos con el agente en marcha: bajarlos aquí cortaba
# a los usuarios sin privilegios el acceso al socket de estado de todas las redes unidas (la CLI y la aplicación
# las veían «detenidas») hasta reiniciar. Propietario y grupo son parámetros solo para poder probarlo sin root.
ensure_run_dir() {
    [ -d "$1" ] || install -d -o "${2:-root}" -g "${3:-wheel}" -m 0755 "$1"
}
# Repara el defecto de la 0.1.13: su `postinstall` dejó /var/run/atenea en 0700 aunque solo hubiera redes por instancia, y los
# usuarios sin privilegios perdieron el acceso a todos los sockets de estado. Solo sube de 0700 a 0755 y solo si es
# un directorio real de root, NO hay instalación heredada (su lanzador es quien fija 0700/0750 con `agent.toml`) y existe al
# menos una red por instancia (cuyo padre debe ser atravesable). Nunca baja permisos ni toca 0750 (`status_gid`).
# Argumentos solo para poder probarlo sin root: directorio, agent.toml heredado, directorio de plists y uid del propietario.
repair_run_dir() {
    dir=$1
    legacy=${2:-'/Library/Application Support/Atenea/agent.toml'}
    plists=${3:-/Library/LaunchDaemons}
    owner=${4:-0}
    [ -d "$dir" ] && [ ! -L "$dir" ] || return 0
    [ "$(/usr/bin/stat -f '%u %Lp' "$dir")" = "$owner 700" ] || return 0
    [ ! -e "$legacy" ] && [ ! -L "$legacy" ] || return 0
    for plist in "$plists"/one.atenea.network.*.plist; do
        [ -e "$plist" ] || continue
        chmod 0755 "$dir"
        return 0
    done
}
# `bootout` es asíncrono: el servicio sigue cargado hasta que su proceso sale. El `cleanup` fuera de línea que viene a continuación
# competiría con la limpieza que hace el propio agente al pararse (retiran los mismos ficheros de resolver y sockets) y fallaba
# a veces, abortando el desinstalador. Espera hasta 20 s (200 × 0,1 s; `STOP_SERVICE_TRIES` solo existe para probarlo); si el
# servicio no se descarga, avisa y sigue: no bloquea la desinstalación.
stop_service() {
    launchctl bootout "system/$1" 2>/dev/null || :
    stop_waited=0
    while launchctl print "system/$1" >/dev/null 2>&1; do
        stop_waited=$((stop_waited + 1))
        [ "$stop_waited" -le "${STOP_SERVICE_TRIES:-200}" ] || { echo "$1 sigue cargado tras bootout" >&2; return 0; }
        sleep 0.1
    done
}
