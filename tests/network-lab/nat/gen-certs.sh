#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Material de prueba exclusivamente; nunca escribirlo en el repositorio.
set -euo pipefail
[[ $# == 1 ]] || { echo 'Uso: gen-certs.sh <directorio>' >&2; exit 2; }
umask 077
mkdir -p "$1"
out=$(cd "$1" && pwd)
repo=$(cd "$(dirname "$0")/../../.." && pwd)
case "$out/" in "$repo/"*) echo 'El directorio debe estar fuera del repositorio' >&2; exit 2;; esac
for file in ca.pem relay.pem relay.key; do
  [[ ! -e "$out/$file" ]] || { echo "Ya existe: $out/$file" >&2; exit 1; }
done
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -sha256 \
  -subj '/CN=Atenea laboratorio CA' -keyout "$work/ca.key" -out "$out/ca.pem" \
  -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' 2>/dev/null
openssl req -new -newkey rsa:2048 -nodes -subj '/CN=relay.atenea.one' \
  -keyout "$out/relay.key" -out "$work/relay.csr" 2>/dev/null
cat >"$work/extensions" <<'EXT'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:relay.atenea.one,IP:198.51.100.10,IP:2001:db8:100::10
EXT
openssl x509 -req -in "$work/relay.csr" -CA "$out/ca.pem" -CAkey "$work/ca.key" \
  -set_serial 1 -days 2 -sha256 -extfile "$work/extensions" -out "$out/relay.pem" 2>/dev/null
chmod 600 "$out/ca.pem" "$out/relay.pem" "$out/relay.key"
printf '%s\n' "$out/ca.pem" "$out/relay.pem" "$out/relay.key"
