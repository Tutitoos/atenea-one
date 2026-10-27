#!/usr/bin/env bash
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Construye "Atenea One.app" (barra de menús) sin instalarla: build.sh [DIRECTORIO_SALIDA]
# Solo usa swiftc y codesign (firma ad hoc: no es una firma de distribución ni notarización).
set -euo pipefail
cd "$(dirname "$0")"
OUT="${1:-../../target/macos-menu}"
APP="$OUT/Atenea One.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# Un solo icono para el panel local y la barra de menús.
cp ../cli/src/icon.svg "$APP/Contents/Resources/icon.svg"
cp Info.plist "$APP/Contents/Info.plist"
# shortcut: solo la arquitectura del equipo que compila, no universal; ampliar al empaquetar para distribuir.
swiftc -O -swift-version 5 -target "$(uname -m)-apple-macos13.0" main.swift -o "$APP/Contents/MacOS/AteneaMenu"
codesign --force --sign - "$APP"
echo "$APP"
