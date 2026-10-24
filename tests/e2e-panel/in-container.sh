#!/usr/bin/env bash
# Dentro del contenedor del E2E del panel: compila el backend, prepara la copia de trabajo de Node
# (el repositorio está montado de solo lectura) y ejecuta Playwright.
set -euo pipefail

cargo build --release --locked -q 2>&1 | tail -3
export PATH=$CARGO_TARGET_DIR/release:$PATH

# Copia de trabajo con node_modules propios de Linux (el volumen /work se reutiliza entre ejecuciones).
rsync -a --delete --exclude node_modules --exclude target --exclude .git --exclude fuzz \
  --exclude 'apps/web/test-results' /src/ /work/repo/ 2>/dev/null || {
  mkdir -p /work/repo && cp -a /src/. /work/repo/
}
cd /work/repo
corepack enable
pnpm config set store-dir /pnpm-store >/dev/null
pnpm install --frozen-lockfile >/dev/null
# El control plane sirve el build hecho en el anfitrión.
export LAB_ROOT=/src
cd apps/web
pnpm exec playwright test --config e2e/playwright.config.ts
rc=$?
cp -r test-results/. /src/apps/web/test-results/ 2>/dev/null || true
exit $rc
