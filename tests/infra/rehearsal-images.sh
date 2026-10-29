#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Construye, sin red, imágenes control-plane/coordinator/relay de UNA revisión (worktree) sobre las bases
# atenea/<servicio>:local ya existentes (solo sustituye binarios e identidad, como scripts/release/build-local.sh,
# sin su `cargo metadata`, que exige fuentes de otras plataformas ausentes de la caché).
# Uso: rehearsal-images.sh <worktree> <etiqueta>   -> atenea-rehearsal/<servicio>:<etiqueta>
set -euo pipefail
src=$(cd "${1:?Uso: rehearsal-images.sh <worktree> <etiqueta>}" && pwd -P)
tag=${2:?etiqueta}
[[ $tag =~ ^[a-z0-9][a-z0-9.-]*$ ]]
commit=$(git -C "$src" rev-parse HEAD)
version=$(python3 -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1] + "/apps/cli/Cargo.toml", "rb"))["package"]["version"])' "$src")
epoch=$(git -C "$src" show -s --format=%ct HEAD)
for image in atenea-lab atenea/control-plane:local atenea/coordinator:local atenea/relay:local; do
    docker image inspect "$image" >/dev/null
done
work=$(mktemp -d /private/tmp/atenea-rehearsal-images.XXXXXX)
trap 'rm -rf "$work"' EXIT
docker run --rm --pull never --network none \
    -v "$src:/src:ro" -v "$work:/out" \
    -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry \
    -e ATENEA_BUILD_COMMIT="$commit" -e ATENEA_VERSION="$version" -e SOURCE_DATE_EPOCH="$epoch" \
    -e RUSTUP_TOOLCHAIN=1.99.1 -e CARGO_BUILD_JOBS=2 -e CARGO_TARGET_DIR=/cache/target \
    --entrypoint bash atenea-lab -c '
        set -euo pipefail
        mkdir /tmp/build && cd /tmp/build
        cp -R /src/Cargo.toml /src/Cargo.lock /src/crates /src/apps .
        # Cargo decide por mtimes: dos revisiones no deben reutilizar artefactos de la otra.
        find crates apps -name "*.rs" -exec touch {} +
        cargo build --release --locked --offline -p atenea-cli -p atenea-control-plane -p atenea-coordinator -p atenea-relay
        for name in atenea atenea-control-plane atenea-coordinator atenea-relay; do
            [ "$(/cache/target/release/$name --version)" = "$name $ATENEA_VERSION ($ATENEA_BUILD_COMMIT)" ]
            cp /cache/target/release/$name /out/$name
        done
    '
for service in control-plane coordinator relay; do
    mkdir "$work/$service"
    cp "$work/atenea-$service" "$work/$service/"
    [ "$service" = relay ] || cp "$work/atenea" "$work/$service/"
    {
        echo "FROM atenea/$service:local"
        echo "LABEL org.opencontainers.image.revision=$commit"
        echo "COPY atenea-$service /usr/local/bin/atenea-$service"
        [ "$service" = relay ] || echo "COPY atenea /usr/local/bin/atenea"
    } > "$work/$service/Dockerfile"
    ref="atenea-rehearsal/$service:$tag"
    docker build --pull=false --network none -q -t "$ref" "$work/$service" >/dev/null
    reported=$(docker run --rm --pull never --network none "$ref" --version)
    [ "$reported" = "atenea-$service $version ($commit)" ] || { echo "versión inesperada en $ref: $reported" >&2; exit 1; }
    echo "$service $ref $reported"
done
