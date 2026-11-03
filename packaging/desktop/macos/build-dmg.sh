#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Empaqueta el binario de escritorio ya compilado en «Atenea One.app» y un DMG. Solo construye: no instala nada.
# Firma ad hoc (`codesign --sign -`): identifica el paquete frente a manipulaciones accidentales pero NO es una firma de
# desarrollador ni está notarizado; Gatekeeper pedirá confirmación en otro Mac.
set -eu
if [ "$#" -ne 3 ]; then
    echo "Uso: $0 BINARIO VERSION DIRECTORIO_SALIDA" >&2
    exit 2
fi
binary=$1
version=$2
out=$3
case "$version" in
    [0-9]*) ;;
    *) echo 'Versión inválida' >&2; exit 2 ;;
esac
case "$version" in
    *[!0-9A-Za-z.+-]*) echo 'Versión inválida' >&2; exit 2 ;;
esac
[ -x "$binary" ] || { echo "Falta el ejecutable $binary" >&2; exit 1; }
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
mkdir -p "$out"
out=$(CDPATH= cd -- "$out" && pwd)
case "$(uname -m)" in
    arm64) arch=arm64 ;;
    x86_64) arch=amd64 ;;
    *) echo 'Arquitectura no admitida' >&2; exit 2 ;;
esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
app="$work/dmg/Atenea One.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 0755 "$binary" "$app/Contents/MacOS/atenea-desktop"
sed "s/@VERSION@/$version/g" "$repo/packaging/desktop/macos/Info.plist" > "$app/Contents/Info.plist"
# El icono sale del PNG de la aplicación; el conjunto es parcial (sin 512 ni 1024) porque el original mide 256 px.
iconset="$work/AteneaOne.iconset"
mkdir "$iconset"
icon="$repo/apps/desktop/icons/icon.png"
for size in 16 32 64 128 256; do
    sips -z "$size" "$size" "$icon" --out "$work/s$size.png" > /dev/null
done
cp "$work/s16.png" "$iconset/icon_16x16.png"
cp "$work/s32.png" "$iconset/icon_16x16@2x.png"
cp "$work/s32.png" "$iconset/icon_32x32.png"
cp "$work/s64.png" "$iconset/icon_32x32@2x.png"
cp "$work/s128.png" "$iconset/icon_128x128.png"
cp "$work/s256.png" "$iconset/icon_128x128@2x.png"
cp "$work/s256.png" "$iconset/icon_256x256.png"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AteneaOne.icns"
python3 "$repo/scripts/release/build-info.py" > "$app/Contents/Resources/BUILD-INFO.json"
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
ln -s /Applications "$work/dmg/Applications"
dmg="$out/atenea-desktop-$version-macOS-$arch.dmg"
rm -f "$dmg"
hdiutil create -quiet -volname "Atenea One" -srcfolder "$work/dmg" -fs HFS+ -format UDZO -ov "$dmg"
printf '%s\n' "$dmg"
