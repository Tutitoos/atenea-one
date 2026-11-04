#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Comprueba el instalador único de macOS ya construido (ADR-022) sin instalarlo: contenido, versión, propietarios y
# `postinstall` del cliente idéntico al del repositorio.
#
#   check-suite-pkg.sh PKG VERSION
set -eu
[ "$#" -eq 2 ] || { echo "Uso: $0 PKG VERSION" >&2; exit 2; }
pkg=$1
version=$2
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
pkgutil --check-signature "$pkg" >/dev/null 2>&1 || :  # sin firma de desarrollador (ADR-022): solo informativo
pkgutil --expand-full "$pkg" "$work/x"
for component in client.pkg desktop.pkg; do
    [ -d "$work/x/$component" ] || { echo "Falta el componente $component" >&2; exit 1; }
done
cmp "$work/x/client.pkg/Scripts/postinstall" "$repo/packaging/macos/postinstall" || { echo 'El postinstall del cliente no es el del repositorio' >&2; exit 1; }
[ -x "$work/x/client.pkg/Scripts/postinstall" ] || { echo 'El postinstall no es ejecutable' >&2; exit 1; }
app="$work/x/desktop.pkg/Payload/Atenea One.app"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")" = "$version" ] || { echo 'Versión de la aplicación distinta' >&2; exit 1; }
codesign --verify --deep --strict "$app"
xmllint --xpath 'string(/pkg-info/@install-location)' "$work/x/desktop.pkg/PackageInfo" | grep -qx /Applications || { echo 'La aplicación no se instala en /Applications' >&2; exit 1; }
xmllint --xpath 'string(/pkg-info/@relocatable)' "$work/x/desktop.pkg/PackageInfo" | grep -qx false || { echo 'La aplicación es reubicable' >&2; exit 1; }
# Todo lo que instala el paquete (salvo metadatos AppleDouble `._*`) debe ser de root.
for component in client.pkg desktop.pkg; do
    bad=$(lsbom "$work/x/$component/Bom" | awk -F'\t' '$1 !~ /(^|\/)\._/ && $3 != "" && $3 !~ /^0\/0/ {print $1}')
    [ -z "$bad" ] || { echo "Ficheros que no son de root en $component: $bad" >&2; exit 1; }
done
echo "OK $(basename "$pkg")"
