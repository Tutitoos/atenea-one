# Laboratorio del E2E del panel: imagen `atenea-lab` (netns, nftables, HAProxy, Rust) más Node y Chromium
# de Playwright. Solo para pruebas; no es una imagen de producción.
FROM atenea-lab
ARG NODE_VERSION=24.11.0
ARG PLAYWRIGHT_VERSION=1.64.1
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl xz-utils \
 && arch="$(dpkg --print-architecture)"; case "$arch" in amd64) n=x64;; arm64) n=arm64;; esac \
 && curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${n}.tar.xz" | tar -xJ -C /usr/local --strip-components=1 \
 && corepack enable \
 && rm -rf /var/lib/apt/lists/*
# Navegador y dependencias del sistema, fijados a la versión de Playwright del panel.
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
RUN npx --yes playwright@${PLAYWRIGHT_VERSION} install --with-deps chromium \
 && rm -rf /var/lib/apt/lists/*
