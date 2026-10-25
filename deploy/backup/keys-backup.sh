#!/bin/sh
# Ejecutar dentro de la imagen postgres-backup: sh /src/deploy/backup/keys-backup.sh DIR SALIDA DESTINATARIO...
set -eu
[ "$#" -ge 3 ] || { echo 'Uso: keys-backup.sh DIR SALIDA DESTINATARIO...' >&2; exit 2; }
src=$1; out=$2; shift 2
umask 077
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
# Lista cerrada: una raíz privada nunca puede entrar en este archivo.
for name in issuer.key relay-signing.hex application.key oidc-client-secret; do
  if [ "$name" = application.key ] && [ ! -e "$src/$name" ] && [ ! -L "$src/$name" ]; then
    continue
  fi
  [ -f "$src/$name" ] && [ ! -L "$src/$name" ] || exit 1
  printf '%s\n' "$name" >> "$tmp/files"
done
tar -C "$src" -cf "$tmp/keys.tar" -T "$tmp/files"
for recipient in "$@"; do
  printf '%s\n' "$recipient" >> "$tmp/recipients"
done
[ ! -e "$out" ] || { echo 'La salida ya existe' >&2; exit 1; }
age -R "$tmp/recipients" -o "$tmp/keys.age" "$tmp/keys.tar"
cp "$tmp/keys.age" "$out"
