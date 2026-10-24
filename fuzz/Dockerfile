# Entorno de fuzzing: nightly + cargo-fuzz sobre la base del proyecto (Rust 1.99).
FROM rust:1.99-bookworm
RUN rustup toolchain install nightly --profile minimal \
    && cargo install cargo-fuzz --locked
WORKDIR /work/fuzz
