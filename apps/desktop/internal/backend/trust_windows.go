// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

package backend

// trustedPath: en Windows la aplicación no ejecuta nada con privilegios (ADR-017 sin implementar), así que no hay ruta
// que confiar. Rechaza todo, con el mismo texto que Elevate, hasta que exista esa elevación y su comprobación.
func trustedPath(string) error { return errNoElevation }
