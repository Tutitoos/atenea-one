#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Prueba `scripts/ci-changes.sh` con un repositorio temporal: si las reglas de rutas fallan, la CI se
# salta en silencio las comprobaciones de plataforma que corresponden.
set -eu

here=$(cd "$(dirname "$0")/../.." && pwd)
script="$here/scripts/ci-changes.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
cd "$work"
git init -q .
git config user.email t@example.test
git config user.name t
git config commit.gpgsign false

for f in docs/a.md apps/web/app/x.tsx apps/control-plane/src/x.rs apps/agent/src/x.rs apps/cli/src/x.rs \
    apps/desktop/src/x.rs crates/tunnel/src/x.rs crates/windows/src/x.rs packaging/macos/x tests/macos/x.py \
    tests/windows/x.ps1 tests/network-lab/x.sh Cargo.lock rust-toolchain.toml .github/workflows/ci.yml \
    .github/workflows/release.yml; do
    mkdir -p "$(dirname "$f")"
    echo base > "$f"
done
git add -A && git commit -q -m base
fails=0

# run EVENT BEFORE BASE_SHA -> "native desktop"
run() {
    : > "$work/.out"
    EVENT=$1 BEFORE=$2 BASE_SHA=$3 SHA=HEAD GITHUB_OUTPUT="$work/.out" sh "$script"
    printf '%s %s' "$(sed -n 's/^native=//p' "$work/.out")" "$(sed -n 's/^desktop=//p' "$work/.out")"
}
# touch FICHERO...: modifica y confirma; deja la confirmación anterior en $prev
touch_commit() {
    prev=$(git rev-parse HEAD)
    for f in "$@"; do echo "$RANDOM$f" >> "$f"; done
    git add -A && git commit -q -m c
}
expect() { # descripción esperado obtenido
    if [ "$2" = "$3" ]; then echo "  OK   $1 ($3)"; else echo "  FAIL $1: esperado '$2', obtenido '$3'"; fails=$((fails + 1)); fi
}

for case in \
    "docs/a.md|false false" "apps/web/app/x.tsx|false false" "apps/control-plane/src/x.rs|false false" \
    "tests/network-lab/x.sh|false false" ".github/workflows/release.yml|false false" \
    "apps/agent/src/x.rs|true false" "apps/cli/src/x.rs|true false" "crates/tunnel/src/x.rs|true false" \
    "packaging/macos/x|true false" "tests/macos/x.py|true false" "tests/windows/x.ps1|true false" \
    "Cargo.lock|true false" "apps/desktop/src/x.rs|false true" "crates/windows/src/x.rs|true true" \
    "rust-toolchain.toml|true true" ".github/workflows/ci.yml|true true"; do
    file=${case%%|*}
    touch_commit "$file"
    expect "push $file" "${case##*|}" "$(run push "$prev" '')"
done

# Varias confirmaciones en un solo push: basta con que una toque código nativo.
touch_commit docs/a.md
first=$prev
touch_commit apps/agent/src/x.rs
touch_commit docs/a.md
expect "push de varias confirmaciones (documentación, agente, documentación)" "true false" "$(run push "$first" '')"

# Borrar y renombrar fuera de una ruta vigilada cuentan igual.
prev=$(git rev-parse HEAD)
git rm -q apps/cli/src/x.rs && git commit -q -m rm
expect "borrar un fichero del CLI" "true false" "$(run push "$prev" '')"
prev=$(git rev-parse HEAD)
git mv apps/agent/src/x.rs docs/moved.md && git commit -q -m mv
expect "mover un fichero del agente a docs/" "true false" "$(run push "$prev" '')"

# Pull request: se compara con la base de la PR.
touch_commit docs/a.md
expect "pull_request solo documentación" "false false" "$(run pull_request '' "$prev")"
touch_commit apps/desktop/src/x.rs
expect "pull_request con escritorio" "false true" "$(run pull_request '' "$prev")"

# Ante la duda, todo.
expect "primer push (base a ceros)" "true true" "$(run push 0000000000000000000000000000000000000000 '')"
expect "push sin base" "true true" "$(run push '' '')"
expect "base que no existe" "true true" "$(run push deadbeefdeadbeefdeadbeefdeadbeefdeadbeef '')"
expect "pull_request sin base" "true true" "$(run pull_request '' '')"
expect "ejecución semanal (schedule)" "true true" "$(run schedule '' '')"
expect "ejecución manual (workflow_dispatch)" "true true" "$(run workflow_dispatch '' '')"
expect "evento desconocido" "true true" "$(run release '' '')"

# Sin cambios entre las dos confirmaciones: nada que ejecutar.
expect "sin diferencias" "false false" "$(run push "$(git rev-parse HEAD)" '')"

[ "$fails" -eq 0 ] && echo "RUTAS DE CI: TODO OK" || { echo "RUTAS DE CI: $fails fallo(s)"; exit 1; }
