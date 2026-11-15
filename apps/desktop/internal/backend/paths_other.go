// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build !windows

package backend

// windowsCLIPath solo tiene sentido en Windows; en el resto de sistemas no hay ruta que devolver.
func windowsCLIPath() string { return "" }
