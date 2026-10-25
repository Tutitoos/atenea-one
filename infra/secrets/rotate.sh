#!/usr/bin/env bash
# Ejecutar en el contenedor de herramientas con SOPS_AGE_KEY_FILE montado, no en el nodo.
set -euo pipefail
[[ $# == 2 ]] || { echo 'Uso: rotate.sh fichero.sops.yaml config.sops.yaml' >&2; exit 2; }
file=$1
config=$2
[[ -f "$file" && -f "$config" ]] || exit 2
umask 077
tmp=$(mktemp --suffix=.sops.yaml "${file}.rotate.XXXXXX")
trap 'rm -f "$tmp"' EXIT
cp "$file" "$tmp"
# updatekeys cambia destinatarios; rotate reemplaza la clave de datos, no las claves de servicio.
sops --config "$config" updatekeys --yes "$tmp"
sops rotate --in-place "$tmp"
sops decrypt "$tmp" >/dev/null
mv "$tmp" "$file"
