FROM rust:1.99@sha256:931c2c0531a8b143e044f0f3914dfe802f15bb3d1955f0b657e9359327ac2ffc AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps/agent ./apps/agent
COPY apps/control-plane ./apps/control-plane
COPY apps/coordinator ./apps/coordinator
COPY apps/relay ./apps/relay
COPY apps/cli ./apps/cli
RUN --mount=type=cache,id=atenea-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=atenea-cargo-target,target=/app/target,sharing=locked \
    cargo build --release --locked -p atenea-control-plane -p atenea-coordinator -p atenea-relay -p atenea-cli \
    && mkdir /out \
    && cp target/release/atenea-control-plane target/release/atenea-coordinator target/release/atenea-relay target/release/atenea /out/
COPY deploy/images/healthcheck.rs /healthcheck.rs
RUN rustc --edition=2024 -O /healthcheck.rs -o /out/healthcheck \
    && rustc --edition=2024 --test /healthcheck.rs -o /healthcheck-test \
    && /healthcheck-test

FROM node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205 AS web
WORKDIR /app
RUN npm install --global pnpm@11.23.0
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json ./apps/web/package.json
COPY packages/api-client ./packages/api-client
COPY docs/api/openapi.json ./docs/api/openapi.json
RUN --mount=type=cache,id=atenea-pnpm-store,target=/pnpm/store,sharing=locked \
    pnpm install --frozen-lockfile --store-dir /pnpm/store
COPY apps/web/app ./apps/web/app
COPY apps/web/scripts ./apps/web/scripts
COPY apps/web/vite.config.ts apps/web/react-router.config.ts apps/web/tsconfig.json ./apps/web/
RUN pnpm --filter @atenea/web build

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:3ee00291cf386ec52a552e6a858d555debd74776eb4bfa53fa3df2cd6677515e AS tools
COPY --from=build /out/atenea /usr/local/bin/atenea
COPY --from=build /out/atenea-coordinator /usr/local/bin/atenea-coordinator
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/atenea"]

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:3ee00291cf386ec52a552e6a858d555debd74776eb4bfa53fa3df2cd6677515e AS runtime
COPY --from=build /out/atenea-control-plane /usr/local/bin/atenea-control-plane
COPY --from=build /out/atenea /usr/local/bin/atenea
COPY --from=build /out/healthcheck /usr/local/bin/healthcheck
COPY --from=web /app/apps/web/build/client /usr/share/atenea/web
ENV ATENEA_CP_WEB_DIR=/usr/share/atenea/web ATENEA_CP_LISTEN=0.0.0.0:8080
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/atenea-control-plane"]
