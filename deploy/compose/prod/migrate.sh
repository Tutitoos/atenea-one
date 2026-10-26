#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Ejecutar antes de arrancar/reemplazar control-plane, nunca con el rol de aplicación.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ENV_FILE=${1:?Uso: migrate.sh /ruta/stack.env}
set -a
source "$ENV_FILE"
set +a
docker compose --env-file "$ENV_FILE" -f "$HERE/compose.yaml" run --rm --no-deps \
  -v "$ATENEA_RUNTIME_DIR/secrets/migration-database-url:/run/secrets/migration-database-url:ro" \
  -e ATENEA_CP_DATABASE_URL_FILE=/run/secrets/migration-database-url \
  -e ATENEA_CP_APPLICATION_ROLE=atenea_app control-plane --migrate
