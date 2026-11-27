// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

// Package quiet evita que los procesos auxiliares abran una ventana de consola. La aplicación se enlaza como
// subsistema gráfico (`-H=windowsgui`) y no tiene consola: sin esta marca, cada ejecutable de consola que lanza (el CLI
// `atenea.exe`, `ping.exe`, `powershell.exe`) crea una consola nueva que se ve parpadear. La bandeja y las pantallas
// sondean el CLI cada pocos segundos, así que parpadearía sin parar.
package quiet

import (
	"os/exec"
	"syscall"
)

// createNoWindow es CREATE_NO_WINDOW: el proceso hijo de consola no recibe ventana. Los hijos que él lance heredan la
// consola oculta, así que tampoco abren la suya. El paquete syscall no exporta la constante.
const createNoWindow = 0x08000000

// Hide marca `cmd` para que no abra ventana de consola. Se llama antes de Start o Run y conserva otras banderas.
func Hide(cmd *exec.Cmd) {
	if cmd.SysProcAttr == nil {
		cmd.SysProcAttr = &syscall.SysProcAttr{}
	}
	cmd.SysProcAttr.HideWindow = true
	cmd.SysProcAttr.CreationFlags |= createNoWindow
}
