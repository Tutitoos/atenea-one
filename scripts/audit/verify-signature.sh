#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Verifica la firma de SHA256SUMS y luego el snapshot. La huella esperada debe
# llegar por un canal independiente: la clave pública del directorio no es confianza.
set -euo pipefail
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Uso: $0 HUELLA_SHA256_ESPERADA [DIRECTORIO_SNAPSHOT]" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
expected=$1
dir=${2:-$root/dist/audit}
fp=$(openssl pkey -pubin -in "$dir/signer-public.pem" -outform DER | openssl dgst -sha256 -r)
fp=${fp%% *}
[ "$fp" = "$expected" ] || { echo "La huella de la clave ($fp) no coincide con la esperada" >&2; exit 1; }
openssl pkeyutl -verify -pubin -inkey "$dir/signer-public.pem" -rawin \
    -in "$dir/SHA256SUMS" -sigfile "$dir/SHA256SUMS.sig"
bash "$root/scripts/audit/verify-snapshot.sh" "$dir"
echo "Firma válida de la clave con huella $fp. Prueba de autoría solo si la huella llegó por un canal independiente."
