#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# No descarga, instala, publica ni construye instaladores de otros sistemas.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
export ATENEA_BUILD_COMMIT=${ATENEA_BUILD_COMMIT:-$(git rev-parse HEAD)}
ATENEA_VERSION=${ATENEA_VERSION:-$(python3 -c 'import tomllib; print(tomllib.load(open("apps/cli/Cargo.toml","rb"))["package"]["version"])')}
export ATENEA_VERSION
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git show -s --format=%ct HEAD)}
dist=${1:-"$root/dist"}
mkdir -p "$dist"
dist=$(cd "$dist" && pwd)
[ -z "$(find "$dist" -mindepth 1 -maxdepth 1 -print -quit)" ] || { echo 'El directorio de salida debe estar vacío' >&2; exit 1; }
for image in atenea-lab atenea-infra:local atenea/control-plane:local atenea/coordinator:local atenea/relay:local; do
    docker image inspect "$image" >/dev/null
done
work=$(mktemp -d)
refs=()
complete=false
cleanup() {
    rm -rf "$work"
    if [ "$complete" != true ]; then
        for ref in "${refs[@]}"; do docker image rm "$ref" >/dev/null 2>&1 || true; done
    fi
}
trap cleanup EXIT
# Se usa exclusivamente el toolchain instalado; rustup no consulta la red.
docker run --rm --pull never --network none \
    -v "$root:/src:ro" -v "$dist:/out" \
    -v atenea-lab-target:/cache -v atenea-cargo-registry:/usr/local/cargo/registry \
    -e ATENEA_BUILD_COMMIT -e ATENEA_VERSION -e SOURCE_DATE_EPOCH \
    -e RUSTUP_TOOLCHAIN=1.99.1 -e CARGO_BUILD_JOBS=2 -e CARGO_TARGET_DIR=/cache/target \
    --entrypoint bash atenea-lab -c '
        set -euo pipefail
        mkdir /tmp/release-src
        cp -R /src/Cargo.toml /src/Cargo.lock /src/crates /src/apps /tmp/release-src/
        cd /tmp/release-src
        cargo build --offline --locked --release -p atenea-cli -p atenea-agent -p atenea-control-plane -p atenea-coordinator -p atenea-relay
        cargo metadata --offline --locked --format-version 1 > /out/cargo-metadata.json
        for name in atenea atenea-agent atenea-control-plane atenea-coordinator atenea-relay; do
            output=$(/cache/target/release/$name --version)
            [ "$output" = "$name $ATENEA_VERSION ($ATENEA_BUILD_COMMIT)" ]
            printf "%s\n" "$output"
            cp /cache/target/release/$name /out/$name-linux
        done
        rustc --version > /out/toolchain.txt
    '
python3 - "$dist" <<'PY'
import json, os, pathlib
out = pathlib.Path(__import__('sys').argv[1])
(out / 'BUILD-INFO.json').write_text(json.dumps({'version': os.environ['ATENEA_VERSION'], 'commit': os.environ['ATENEA_BUILD_COMMIT'], 'toolchain': (out / 'toolchain.txt').read_text().strip()}, sort_keys=True) + '\n')
PY
mkdir "$work/bin"
cp "$dist/atenea-linux" "$work/bin/atenea"
cp "$dist/atenea-agent-linux" "$work/bin/atenea-agent"
# dpkg viene de la imagen de infraestructura ya instalada.
docker run --rm --pull never --network none --entrypoint bash \
    -v "$root:/src:ro" -v "$work/bin:/bins:ro" -v "$dist:/out" \
    -e ATENEA_BUILD_COMMIT atenea-infra:local -c \
    "bash /src/packaging/deb/build.sh /bins '$ATENEA_VERSION' \"\$(dpkg --print-architecture)\" /out/atenea.deb"
python3 scripts/release/sbom.py rust "$dist/cargo-metadata.json" "$dist/rust.cdx.json" --version "$ATENEA_VERSION" --commit "$ATENEA_BUILD_COMMIT"
python3 scripts/release/sbom.py panel pnpm-lock.yaml "$dist/panel.cdx.json" --version "$ATENEA_VERSION" --commit "$ATENEA_BUILD_COMMIT"
image_args=()
for service in control-plane coordinator relay; do
    ref="atenea-release-$service:local-$$"
    refs+=("$ref")
    mkdir "$work/$service"
    cp "$dist/atenea-$service-linux" "$work/$service/atenea-$service"
    if [ "$service" != relay ]; then cp "$dist/atenea-linux" "$work/$service/atenea"; fi
    # Conserva el runtime y el panel existentes; solo sustituye binarios e identidad.
    cat > "$work/$service/Dockerfile" <<EOF
FROM atenea/$service:local
LABEL org.opencontainers.image.revision=$ATENEA_BUILD_COMMIT
COPY atenea-$service /usr/local/bin/atenea-$service
EOF
    if [ "$service" != relay ]; then echo 'COPY atenea /usr/local/bin/atenea' >> "$work/$service/Dockerfile"; fi
    docker build --pull=false --network none -t "$ref" "$work/$service"
    [ "$(docker run --rm --pull never --network none "$ref" --version)" = "atenea-$service $ATENEA_VERSION ($ATENEA_BUILD_COMMIT)" ]
    image_args+=("$service=$ref")
done
python3 scripts/release/images.py "$dist" "$ATENEA_BUILD_COMMIT" "$ATENEA_VERSION" "${image_args[@]}"
complete=true
echo 'PASS ensamblado local sin red; macOS .pkg y Windows ZIP NO construidos (solo runners)'
echo 'Las imágenes nuevas figuran en images.json; el ensayo las elimina al terminar.'
