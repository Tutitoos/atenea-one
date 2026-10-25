#!/bin/sh
# Instala pnpm verificando la integridad SHA-512 que npm publica para la versión fijada.
# Al subir de versión: `npm view pnpm@X dist.integrity` (base64) y convertir a hex.
set -eu
version=11.23.0
sha512=f00082e5b283a199b74e079da28d155c008fe232f44c8a06ea7ddfa014ecf719fc362f790ecc67b28106ddac2afb24c49a9a079159da560b2ce7d1e98efd11af

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
grep -q "\"packageManager\": \"pnpm@$version\"" "$root/package.json" || { echo "package.json no fija pnpm@$version" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
cd "$work"
npm pack "pnpm@$version" --silent >/dev/null
actual=$(sha512sum "pnpm-$version.tgz" | cut -d' ' -f1)
[ "$actual" = "$sha512" ] || { echo "SHA-512 de pnpm@$version incorrecto" >&2; exit 1; }
npm install --global --ignore-scripts "./pnpm-$version.tgz"
