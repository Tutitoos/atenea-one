// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

package quiet

import (
	"os/exec"
	"syscall"
	"testing"
)

func TestHideMarcaSinVentanaYConservaOtrasBanderas(t *testing.T) {
	cmd := exec.Command("cmd.exe", "/c", "exit", "0")
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP}
	Hide(cmd)
	if !cmd.SysProcAttr.HideWindow {
		t.Fatal("HideWindow debe estar activo")
	}
	if cmd.SysProcAttr.CreationFlags&createNoWindow == 0 {
		t.Fatal("falta CREATE_NO_WINDOW")
	}
	if cmd.SysProcAttr.CreationFlags&syscall.CREATE_NEW_PROCESS_GROUP == 0 {
		t.Fatal("se perdió una bandera previa")
	}
}

func TestHideSinAtributosPreviosYElProcesoSeEjecuta(t *testing.T) {
	cmd := exec.Command("cmd.exe", "/c", "exit", "0")
	Hide(cmd)
	if cmd.SysProcAttr == nil || cmd.SysProcAttr.CreationFlags&createNoWindow == 0 {
		t.Fatal("falta CREATE_NO_WINDOW")
	}
	if err := cmd.Run(); err != nil {
		t.Fatalf("el proceso oculto debe ejecutarse: %v", err)
	}
}
