#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Firma SHA256SUMS del snapshot con una clave Ed25519 del responsable (misma
# operación que scripts/release/manifest.py: openssl pkeyutl -rawin).
# La clave privada NO debe estar dentro del repositorio ni pasar por terceros.
set -euo pipefail
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Uso: $0 CLAVE_PRIVADA_ED25519.pem [DIRECTORIO_SNAPSHOT]" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
key=$1
dir=${2:-$root/dist/audit}
[ -f "$key" ] || { echo "No existe la clave: $key" >&2; exit 2; }
case "$(cd "$(dirname "$key")" && pwd)/" in
    "$root"/*) echo "La clave privada no puede estar dentro del repositorio" >&2; exit 2 ;;
esac
[ "$(openssl pkey -in "$key" -noout -text 2>/dev/null | awk 'NR==1')" = "ED25519 Private-Key:" ] \
    || { echo "La clave no es Ed25519" >&2; exit 2; }
bash "$root/scripts/audit/verify-snapshot.sh" "$dir" >/dev/null
openssl pkeyutl -sign -rawin -inkey "$key" -in "$dir/SHA256SUMS" -out "$dir/SHA256SUMS.sig"
openssl pkey -in "$key" -pubout -out "$dir/signer-public.pem"
fp=$(openssl pkey -pubin -in "$dir/signer-public.pem" -outform DER | openssl dgst -sha256 -r)
echo "Firmado: $dir/SHA256SUMS.sig"
echo "Huella SHA-256 de la clave pública (SPKI DER): ${fp%% *}"
echo "Entregue esta huella al auditor por un canal distinto al del tarball."
