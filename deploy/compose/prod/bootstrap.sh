#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Solo estación administrativa: la raíz se genera offline y jamás se monta en servicios.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
RUNTIME=${ATENEA_RUNTIME_DIR:-$ROOT/deploy/compose/prod/.runtime}
[[ "$RUNTIME" = /* ]] || { echo 'ATENEA_RUNTIME_DIR debe ser absoluto' >&2; exit 2; }
[[ ! -e "$RUNTIME" ]] || { echo 'El directorio runtime ya existe; no se sobrescriben claves' >&2; exit 2; }
mkdir -m 700 -p "$RUNTIME"
TOOLS_IMAGE=${TOOLS_IMAGE:-atenea/tools:local}
export PANEL_HOST=${PANEL_HOST:-panel.stack.test}
export RELAY_HOST=${RELAY_HOST:-relay.stack.test}
CADDY_IMAGE=caddy:2.10-alpine@sha256:4c6e91c6ed0e2fa03efd5b44747b625fec79bc9cd06ac5235a779726618e530d
# El hash bcrypt lo genera Caddy; la contraseña existe únicamente en el runtime privado.
docker run --rm --user 0 -v "$RUNTIME:/runtime" -v "$ROOT:/src:ro" \
  -e PANEL_HOST -e RELAY_HOST atenea-panel-lab python3 /src/deploy/compose/prod/bootstrap.py
PASSWORD=$(docker run --rm --user 0 -v "$RUNTIME:/runtime:ro" atenea-panel-lab cat /runtime/offline/dex-password)
HASH=$(docker run --rm "$CADDY_IMAGE" caddy hash-password --plaintext "$PASSWORD")
unset PASSWORD
docker run --rm --user 0 -v "$RUNTIME:/runtime" -e DEX_HASH="$HASH" atenea-panel-lab python3 -c 'import os,pathlib; p=pathlib.Path("/runtime/secrets/dex.yaml"); p.chmod(0o600); p.write_text(p.read_text().replace("BCRYPT_RUNTIME",os.environ["DEX_HASH"])); os.chown(p,65532,65532); p.chmod(0o400)'
unset HASH
docker run --rm --user 0 -v "$RUNTIME:/runtime" "$TOOLS_IMAGE" root-init --out /runtime/offline/root.key > "$RUNTIME/config/root.pub"
RELAY_PUB=$(docker run --rm --user 0 -v "$RUNTIME:/runtime:ro" -e ATENEA_COORD_RELAY_SIGNING_KEY_FILE=/runtime/secrets/relay-signing.hex --entrypoint /usr/local/bin/atenea-coordinator "$TOOLS_IMAGE" --print-relay-key)
docker run --rm --user 0 -v "$RUNTIME:/runtime" -v "$ROOT:/src:ro" -e RELAY_PUB="$RELAY_PUB" -e PANEL_HOST -e RELAY_HOST atenea-panel-lab python3 /src/deploy/compose/prod/bootstrap.py config
for entry in CONTROL_PLANE:control-plane COORDINATOR:coordinator RELAY:relay; do
  variable=${entry%%:*}; service=${entry#*:}
  printf '%s_IMAGE=%s\n' "$variable" "$(docker image inspect "atenea/$service:local" --format '{{.Id}}')" >> "$RUNTIME/stack.env"
done
printf 'ATENEA_RUNTIME_DIR=%s\nPANEL_HOST=%s\nRELAY_HOST=%s\nCADDY_TLS=internal\n' "$RUNTIME" "$PANEL_HOST" "$RELAY_HOST" >> "$RUNTIME/stack.env"
chmod 600 "$RUNTIME/stack.env"
echo "Bootstrap listo: $RUNTIME/stack.env (CA privada solo laboratorio; raíz en offline/)"
