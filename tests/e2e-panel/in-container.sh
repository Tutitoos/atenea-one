#!/usr/bin/env bash
# Dentro del contenedor del E2E del panel: compila el backend, prepara la copia de trabajo de Node
# (el repositorio está montado de solo lectura) y ejecuta Playwright.
set -euo pipefail

export COREPACK_ENABLE_DOWNLOAD_PROMPT=0 CI=true
cargo build --release --locked -q 2>&1 | tail -3
export PATH=$CARGO_TARGET_DIR/release:$PATH

# Copia de trabajo sin node_modules del anfitrión (macOS): se instalan los de Linux desde el almacén
# de pnpm, reutilizado entre ejecuciones.
rsync -a --delete --delete-excluded --exclude node_modules --exclude target --exclude .git \
  --exclude fuzz --exclude 'apps/web/test-results' --exclude 'apps/web/build' /src/ /work/repo/
cd /work/repo
corepack enable
pnpm config set store-dir /pnpm-store >/dev/null
pnpm install --frozen-lockfile
# El control plane sirve el build hecho en el anfitrión (LAB_ROOT=/src).
export LAB_ROOT=/src
cd apps/web
rc=0
pnpm exec playwright test --config e2e/playwright.config.ts || rc=$?
cp -r test-results/. /src/apps/web/test-results/ 2>/dev/null || true
exit $rc
