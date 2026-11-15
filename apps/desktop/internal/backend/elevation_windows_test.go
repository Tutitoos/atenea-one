// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

package backend

import (
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"
)

// TestWindowsElevationAcceptance ejerce la ruta REAL de la aplicación (comprobación del CLI de Archivos de programa,
// `powershell.exe Start-Process -Verb RunAs`, canal result.json) contra un cliente ya instalado y una red ya unida.
// Solo se ejecuta en el runner desechable de CI: `tests/windows/network-checks.ps1` instala, une la red y fija
// ATENEA_ACCEPTANCE_NETWORK. En el runner UAC no pide confirmación (la cuenta ya es administrador), así que el diálogo
// y su cancelación NO se ejercitan aquí: eso exige un Windows real (ADR-017 §8).
func TestWindowsElevationAcceptance(t *testing.T) {
	id := os.Getenv("ATENEA_ACCEPTANCE_NETWORK")
	if os.Getenv("GITHUB_ACTIONS") != "true" || id == "" {
		t.Skip("solo en el runner de CI con el cliente instalado y una red unida")
	}
	if !ValidUUID(id) {
		t.Fatalf("id de red no válido: %q", id)
	}
	if got := Client(); !got.Installed || !got.CanJoin {
		t.Fatalf("cliente: %+v", got)
	}
	if err := CheckCLI(CLIPath()); err != nil {
		t.Fatalf("el CLI instalado debe pasar la comprobación de confianza: %v", err)
	}
	// Un CLI fuera de Archivos de programa nunca se eleva.
	if err := CheckCLI(os.Getenv("SystemRoot") + `\System32\cmd.exe`); err == nil {
		t.Fatal("un binario ajeno a la carpeta del instalador no debe elevarse")
	}

	state := func() string {
		text, err := List()
		if err != nil {
			t.Fatal(err)
		}
		var list []map[string]any
		if err := json.Unmarshal([]byte(text), &list); err != nil {
			t.Fatalf("%v: %s", err, text)
		}
		for _, n := range list {
			if n["network_id"] == id {
				s, _ := n["state"].(string)
				return s
			}
		}
		return "ausente"
	}
	if got := state(); got != "detenida" {
		t.Fatalf("la red debía empezar detenida: %s", got)
	}

	// Una red inexistente falla con el mensaje que deja el CLI elevado, no con una salida capturada.
	_, err := Apply("up", "00000000-0000-4000-8000-00000000dead", "", "")
	if err == nil || !strings.Contains(err.Error(), "ninguna red coincide") {
		t.Fatalf("up de una red inexistente: %v", err)
	}
	if _, err := Apply("up", id, "", ""); err != nil {
		t.Fatalf("up: %v", err)
	}
	// La tubería de estado aparece unos instantes después de que el SCM marque el servicio en marcha: la interfaz
	// sondea, y la prueba también.
	waitFor := func(want string) string {
		deadline := time.Now().Add(30 * time.Second)
		for {
			got := state()
			if got == want || time.Now().After(deadline) {
				return got
			}
			InvalidateList()
			time.Sleep(500 * time.Millisecond)
		}
	}
	if got := waitFor("iniciada_sin_configuracion"); got != "iniciada_sin_configuracion" {
		t.Fatalf("tras up: %s", got)
	}
	if _, err := Apply("down", id, "", ""); err != nil {
		t.Fatalf("down: %v", err)
	}
	if got := waitFor("detenida"); got != "detenida" {
		t.Fatalf("tras down: %s", got)
	}
	if _, err := Apply("leave", id, "", ""); err != nil {
		t.Fatalf("leave: %v", err)
	}
	if got := waitFor("ausente"); got != "ausente" {
		t.Fatalf("tras leave: %s", got)
	}
}
