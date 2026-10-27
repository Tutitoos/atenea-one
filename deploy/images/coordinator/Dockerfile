FROM rust:1.99@sha256:931c2c0531a8b143e044f0f3914dfe802f15bb3d1955f0b657e9359327ac2ffc AS build
ARG ATENEA_BUILD_COMMIT=unknown
ENV ATENEA_BUILD_COMMIT=$ATENEA_BUILD_COMMIT
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

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:3ee00291cf386ec52a552e6a858d555debd74776eb4bfa53fa3df2cd6677515e AS tools
ARG ATENEA_BUILD_COMMIT=unknown
LABEL org.opencontainers.image.revision=$ATENEA_BUILD_COMMIT
LABEL org.opencontainers.image.source=https://github.com/Tutitoos/atenea-one
COPY --from=build /out/atenea /usr/local/bin/atenea
COPY --from=build /out/atenea-coordinator /usr/local/bin/atenea-coordinator
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/atenea"]

FROM tools AS runtime
COPY --from=build /out/healthcheck /usr/local/bin/healthcheck
ENV ATENEA_COORD_LISTEN=0.0.0.0:8081
EXPOSE 8081 3478/udp 3479/udp
ENTRYPOINT ["/usr/local/bin/atenea-coordinator"]
