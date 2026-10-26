#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
export PYTHONDONTWRITEBYTECODE=1
export ATENEA_BUILD_COMMIT=${ATENEA_BUILD_COMMIT:-$(git rev-parse HEAD)}
ATENEA_VERSION=${ATENEA_VERSION:-$(python3 -c 'import tomllib; print(tomllib.load(open("apps/cli/Cargo.toml","rb"))["package"]["version"])')}
export ATENEA_VERSION
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git show -s --format=%ct HEAD)}
keys=$(mktemp -d)
dist="$root/dist"
cleanup() {
    rm -rf "$keys"
    if [ -f "$dist/images.json" ]; then
        python3 - "$dist/images.json" <<'PY'
import json, subprocess, sys
for image in json.load(open(sys.argv[1])):
    subprocess.run(['docker', 'image', 'rm', image['local_reference']], check=True, stdout=subprocess.DEVNULL)
PY
    fi
}
trap cleanup EXIT
# Nunca borra un dist ajeno; el operador debe archivarlo antes de repetir.
bash scripts/release/build-local.sh "$dist"
umask 077
for name in signing other; do
    openssl genpkey -algorithm ED25519 -out "$keys/$name.key"
    openssl pkey -in "$keys/$name.key" -pubout -out "$keys/$name.pub"
done
python3 scripts/release/manifest.py create "$dist" "$keys/signing.key" "$ATENEA_BUILD_COMMIT" "$ATENEA_VERSION"
bash scripts/release/verify.sh "$dist" "$keys/signing.pub" "$ATENEA_BUILD_COMMIT" "$ATENEA_VERSION"
cp "$dist/release-manifest.json" "$keys/manifest.json"
cp "$dist/release-manifest.json.sig" "$keys/manifest.sig"
python3 scripts/release/manifest.py create "$dist" "$keys/signing.key" "$ATENEA_BUILD_COMMIT" "$ATENEA_VERSION"
cmp "$dist/release-manifest.json" "$keys/manifest.json"
cmp "$dist/release-manifest.json.sig" "$keys/manifest.sig"
echo 'PASS manifiesto y firma reproducibles con SOURCE_DATE_EPOCH'
python3 - "$dist" <<'PY'
import json, pathlib, sys
dist = pathlib.Path(sys.argv[1])
for name in ('control-plane', 'coordinator', 'relay'):
    bom = json.loads((dist / (name + '.cdx.json')).read_text())
    packages = [c for c in bom['components'] if c['bom-ref'].startswith('deb:')]
    assert packages, 'Falta inventario Debian/distroless'
    print(f'PASS SBOM {name}: {len(packages)} paquetes del filesystem y ejecutables con hash')
PY
python3 tests/release/tamper.py "$root" "$dist" "$keys/signing.pub" "$keys/other.pub" "$ATENEA_BUILD_COMMIT" "$ATENEA_VERSION"
bash tests/packaging/run.sh --release "$dist" "$ATENEA_BUILD_COMMIT"
echo 'RELEASE: TODO OK; claves efímeras eliminadas al salir (no son confianza de producción)'
echo 'Descargas de red: ninguna; docker --network none / --pull never, cargo --offline --locked'
echo 'Recursos Docker propios: contenedores eliminados; etiquetas de release eliminadas al salir'
echo 'SIN COMPROBAR: GHCR/Releases reales, digest de registro, .pkg/ZIP de runners, clave real'
