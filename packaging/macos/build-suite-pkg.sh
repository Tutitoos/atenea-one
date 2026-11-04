#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Construye el instalador único de macOS «Atenea One» (ADR-022): la aplicación de escritorio y el cliente (CLI y agente)
# en un solo `.pkg` de distribución, con una sola autorización y una sola versión. Solo construye: no instala nada y no
# requiere sudo.
#
#   build-suite-pkg.sh CLIENTE.pkg APP VERSION DIRECTORIO_SALIDA
#
# CLIENTE.pkg es el `atenea-cli-<v>-macos-<arq>.pkg` de build-pkg.sh; se reutiliza **su componente tal cual** (scripts,
# propietarios y `postinstall` incluidos). APP es `Atenea One.app` ya empaquetada y firmada ad hoc (o el `.dmg` que la
# contiene). El `.app` se instala en /Applications como propiedad de root: no queda una copia que un proceso del usuario
# pueda modificar y que el sistema ejecute después con privilegios.
set -eu
if [ "$#" -ne 4 ]; then
    echo "Uso: $0 CLIENTE.pkg APP|DMG VERSION DIRECTORIO_SALIDA" >&2
    exit 2
fi
client=$1
app=$2
version=$3
out=$4
case "$version" in
    [0-9]*) ;;
    *) echo 'Versión inválida' >&2; exit 2 ;;
esac
case "$version" in
    *[!0-9A-Za-z.+-]*) echo 'Versión inválida' >&2; exit 2 ;;
esac
[ -f "$client" ] || { echo "Falta el paquete del cliente $client" >&2; exit 1; }
case "$(uname -m)" in
    arm64) arch=arm64 ;;
    x86_64) arch=amd64 ;;
    *) echo 'Arquitectura no admitida' >&2; exit 2 ;;
esac
mkdir -p "$out"
out=$(CDPATH= cd -- "$out" && pwd)
work=$(mktemp -d)
mount=''
cleanup() {
    [ -z "$mount" ] || hdiutil detach -quiet "$mount" >/dev/null 2>&1 || :
    rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

# La aplicación: del directorio dado o del DMG.
stage="$work/app"
mkdir -p "$stage"
case "$app" in
    *.dmg)
        mount="$work/mnt"
        mkdir "$mount"
        hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$app"
        ditto --norsrc --noextattr --noqtn "$mount/Atenea One.app" "$stage/Atenea One.app"
        ;;
    *)
        [ "$(basename "$app")" = 'Atenea One.app' ] || { echo 'La aplicación debe llamarse «Atenea One.app»' >&2; exit 1; }
        ditto --norsrc --noextattr --noqtn "$app" "$stage/Atenea One.app"
        ;;
esac
[ -x "$stage/Atenea One.app/Contents/MacOS/atenea-desktop" ] || { echo 'Falta Contents/MacOS/atenea-desktop' >&2; exit 1; }
app_version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$stage/Atenea One.app/Contents/Info.plist")
[ "$app_version" = "$version" ] || { echo "La aplicación es la $app_version y se pidió la $version" >&2; exit 1; }
codesign --verify --deep --strict "$stage/Atenea One.app"

mkdir -p "$work/components"
# El componente del cliente sale del producto tal cual lo construyó build-pkg.sh.
pkgutil --expand "$client" "$work/client"
client_component=$(ls -d "$work"/client/*.pkg | head -n 1)
[ -d "$client_component" ] || { echo 'El paquete del cliente no trae un componente' >&2; exit 1; }
pkgutil --flatten "$client_component" "$work/components/client.pkg"

# El componente de la aplicación: NO reubicable (si no, el instalador actualizaría cualquier copia de la aplicación que
# encuentre en otro sitio, p. ej. en la carpeta del usuario) y sin comprobación de versión (siempre se instala la de
# este paquete, que es la que corresponde al cliente que lo acompaña).
pkgbuild --analyze --root "$stage" "$work/app.plist" >/dev/null
/usr/libexec/PlistBuddy -c 'Add :0:BundleIsRelocatable bool false' "$work/app.plist"
/usr/libexec/PlistBuddy -c 'Set :0:BundleIsVersionChecked false' "$work/app.plist"
pkgbuild --root "$stage" --component-plist "$work/app.plist" --identifier one.atenea.desktop.app --version "$version" \
    --install-location /Applications --ownership recommended "$work/components/desktop.pkg" >/dev/null

client_version=$(/usr/bin/xmllint --xpath 'string(/pkg-info/@version)' "$client_component/PackageInfo")
[ "$client_version" = "$version" ] || { echo "El cliente es el $client_version y se pidió la $version" >&2; exit 1; }
cat > "$work/distribution.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>Atenea One</title>
    <options customize="never" require-scripts="false" hostArchitectures="$(uname -m)"/>
    <domains enable_localSystem="true"/>
    <choices-outline>
        <line choice="default">
            <line choice="one.atenea.client"/>
            <line choice="one.atenea.desktop.app"/>
        </line>
    </choices-outline>
    <choice id="default"/>
    <choice id="one.atenea.client" visible="false"><pkg-ref id="one.atenea.client"/></choice>
    <choice id="one.atenea.desktop.app" visible="false"><pkg-ref id="one.atenea.desktop.app"/></choice>
    <pkg-ref id="one.atenea.client" version="$version" onConclusion="none">client.pkg</pkg-ref>
    <pkg-ref id="one.atenea.desktop.app" version="$version" onConclusion="none">desktop.pkg</pkg-ref>
</installer-gui-script>
EOF
pkg="$out/atenea-one-$version-macos-$arch.pkg"
rm -f "$pkg"
productbuild --distribution "$work/distribution.xml" --package-path "$work/components" "$pkg" >/dev/null
printf '%s\n' "$pkg"
