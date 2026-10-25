#!/bin/sh
set -eu
# El secreto se carga del fichero, nunca se incorpora a la imagen o Compose.
export PGBACKREST_REPO1_CIPHER_PASS="$(cat /run/secrets/pgbackrest_cipher)"
[ -n "$PGBACKREST_REPO1_CIPHER_PASS" ] || exit 1
if [ "$1" = postgres ]; then
  exec docker-entrypoint.sh "$@"
fi
exec "$@"
