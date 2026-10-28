#!/bin/sh
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Decide qué jobs de plataforma (macOS y Windows) necesita un cambio. Salida: `native=true|false` y
# `desktop=true|false` (en $GITHUB_OUTPUT, o por salida estándar si no existe).
#   native  -> agente, CLI, crates, empaquetado y sus pruebas de plataforma, Cargo y toolchain.
#   desktop -> la aplicación de escritorio (workspace propio; en Windows solo depende de crates/windows).
# Ante la duda ejecuta TODO: sin base utilizable (primer push, historia reescrita), ejecución semanal,
# manual o cualquier evento desconocido. Los jobs obligatorios (`check`) no dependen de esto.
# Entradas por entorno: EVENT, BEFORE (push), BASE_SHA (pull_request) y SHA (por defecto HEAD).
set -eu

out=${GITHUB_OUTPUT:-/dev/stdout}
emit() { printf 'native=%s\ndesktop=%s\n' "$1" "$2" >> "$out"; }

NATIVE='^(apps/(agent|cli)/|crates/|packaging/|tests/(macos|windows)/|Cargo\.(toml|lock)$|rust-toolchain\.toml$|\.github/workflows/ci\.yml$|scripts/ci-changes\.sh$)'
DESKTOP='^(apps/desktop/|crates/windows/|rust-toolchain\.toml$|\.github/workflows/ci\.yml$|scripts/ci-changes\.sh$)'

case "${EVENT:-}" in
    push) base=${BEFORE:-} ;;
    pull_request) base=${BASE_SHA:-} ;;
    *) emit true true; exit 0 ;;
esac

zero=0000000000000000000000000000000000000000
if [ -z "$base" ] || [ "$base" = "$zero" ] || ! git cat-file -e "$base^{commit}" 2>/dev/null; then
    emit true true
    exit 0
fi

# Sin detección de renombrados: mover un fichero fuera de una ruta vigilada también cuenta.
files=$(git diff --no-renames --name-only "$base" "${SHA:-HEAD}")
has() { if printf '%s\n' "$files" | grep -Eq "$1"; then echo true; else echo false; fi; }
emit "$(has "$NATIVE")" "$(has "$DESKTOP")"
