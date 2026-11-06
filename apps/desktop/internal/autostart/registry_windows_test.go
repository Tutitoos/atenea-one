// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

package autostart

import (
	"testing"

	"golang.org/x/sys/windows/registry"
)

// useTempRunKey redirige el registro a una clave propia de la prueba (nunca toca la `Run` real) y la borra al acabar.
func useTempRunKey(t *testing.T) {
	t.Helper()
	old := runKeyPath
	runKeyPath = `Software\AteneaOneTest\` + t.Name()
	t.Cleanup(func() {
		runKeyPath = old
		_ = registry.DeleteKey(registry.CURRENT_USER, `Software\AteneaOneTest\`+t.Name())
		_ = registry.DeleteKey(registry.CURRENT_USER, `Software\AteneaOneTest`)
	})
}

func TestWindowsRegistryRoundTripWritesTheQuotedCommandAndIsIdempotent(t *testing.T) {
	useTempRunKey(t)
	exe := `C:\Users\equipo-a\AppData\Local\Programs\Atenea One\atenea-desktop.exe`

	if got, err := getFor("windows", "", ""); err != nil || got == nil || *got {
		t.Fatalf("al principio debe estar desactivado (la clave ni existe): %v, %v", got, err)
	}
	if err := setFor("windows", "", "", exe, false); err != nil { // desactivar lo que no existe no falla
		t.Fatal(err)
	}
	for range 2 { // reactivar reescribe sin error
		if err := setFor("windows", "", "", exe, true); err != nil {
			t.Fatal(err)
		}
	}
	if got, err := getFor("windows", "", ""); err != nil || got == nil || !*got {
		t.Fatalf("tras activar: %v, %v", got, err)
	}
	k, err := registry.OpenKey(registry.CURRENT_USER, runKeyPath, registry.QUERY_VALUE)
	if err != nil {
		t.Fatal(err)
	}
	defer k.Close()
	if v, _, err := k.GetStringValue(valueName); err != nil || v != `"`+exe+`"` {
		t.Fatalf("valor = %q, %v", v, err)
	}
	for range 2 {
		if err := setFor("windows", "", "", exe, false); err != nil {
			t.Fatal(err)
		}
	}
	if got, err := getFor("windows", "", ""); err != nil || got == nil || *got {
		t.Fatalf("tras desactivar: %v, %v", got, err)
	}
}

func TestWindowsRegistryRefusesABadPathWithoutWriting(t *testing.T) {
	useTempRunKey(t)
	if err := setFor("windows", "", "", `C:\a"b\x.exe`, true); err == nil {
		t.Fatal("una ruta con comilla no debe escribirse")
	}
	if got, err := getFor("windows", "", ""); err != nil || got == nil || *got {
		t.Fatalf("no debe haber quedado nada: %v, %v", got, err)
	}
}
