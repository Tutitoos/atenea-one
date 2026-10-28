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
