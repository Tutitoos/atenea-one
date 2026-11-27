// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build !windows

// Package quiet evita que los procesos auxiliares abran una ventana de consola (solo hace falta en Windows).
package quiet

import "os/exec"

// Hide no hace nada fuera de Windows: no existe el problema.
func Hide(*exec.Cmd) {}
