#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# PostgreSQL real obligatorio; reutiliza el contenedor sin borrar datos ajenos.
set -euo pipefail
cd "$(dirname "$0")/../../.."

if [[ -z "${ATENEA_TEST_DATABASE_URL:-}" ]]; then
  command -v docker >/dev/null || { echo 'Falta ATENEA_TEST_DATABASE_URL y Docker no está instalado.' >&2; exit 1; }
  name=atenea-control-plane-tests-pg17
  if docker container inspect "$name" >/dev/null 2>&1; then
    image="$(docker inspect --format '{{.Config.Image}}' "$name")"
    [[ "$image" == postgres:17 ]] || { echo "El contenedor $name no usa postgres:17; no se modifica." >&2; exit 1; }
    docker start "$name" >/dev/null
  else
    docker run --detach --name "$name" --publish 127.0.0.1::5432 \
      --env POSTGRES_USER=atenea_test --env POSTGRES_PASSWORD=atenea_test \
      --env POSTGRES_DB=postgres postgres:17 >/dev/null
  fi
  ready=false
  for ((attempt=0; attempt<60; attempt++)); do
    if docker exec "$name" pg_isready --username atenea_test --dbname postgres >/dev/null 2>&1; then ready=true; break; fi
    sleep 1
  done
  [[ "$ready" == true ]] || { echo 'PostgreSQL 17 no está listo tras 60 segundos.' >&2; exit 1; }
  port="$(docker inspect --format '{{(index (index .NetworkSettings.Ports "5432/tcp") 0).HostPort}}' "$name")"
  export ATENEA_TEST_DATABASE_URL="postgres://atenea_test:atenea_test@127.0.0.1:${port}/postgres"
fi
: "${ATENEA_TEST_DATABASE_URL:?Se exige PostgreSQL real para ejecutar la integración}"
exec cargo test -p atenea-control-plane --tests -- "$@" --nocapture
