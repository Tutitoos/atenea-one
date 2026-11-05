// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

// countingCLI instala un «CLI» que anota cada ejecución en un fichero y imprime `body`.
func countingCLI(t *testing.T, body string) (runs func() int) {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("CLI de shell")
	}
	dir := t.TempDir()
	log := filepath.Join(dir, "runs")
	cli := filepath.Join(dir, "atenea")
	script := "#!/bin/sh\necho x >> '" + log + "'\n" + body + "\n"
	if err := os.WriteFile(cli, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("ATENEA_CLI", cli)
	InvalidateList()
	return func() int {
		b, _ := os.ReadFile(log)
		return strings.Count(string(b), "x")
	}
}

func TestListIsCachedWithinTTLInvalidatedAndErrorsAreNotCached(t *testing.T) {
	runs := countingCLI(t, "echo '[]'")
	for range 2 {
		if out, err := List(); err != nil || out != "[]" {
			t.Fatalf("List: %q %v", out, err)
		}
	}
	if n := runs(); n != 1 {
		t.Fatalf("dos llamadas dentro del TTL deben ejecutar el CLI una vez, fueron %d", n)
	}
	InvalidateList()
	if _, err := List(); err != nil || runs() != 2 {
		t.Fatalf("tras invalidar se ejecuta de nuevo: %v (%d)", err, runs())
	}

	bad := countingCLI(t, "echo 'no es json'")
	for range 2 {
		if _, err := List(); err == nil {
			t.Fatal("una respuesta inesperada debe fallar")
		}
	}
	if n := bad(); n != 2 {
		t.Errorf("un error no se cachea: %d ejecuciones, esperadas 2", n)
	}
}

func TestApplyInvalidatesTheListEvenWhenElevationFails(t *testing.T) {
	runs := countingCLI(t, "echo '[]'")
	old := Elevate
	Elevate = func(string, string) (string, error) { return "", os.ErrPermission }
	t.Cleanup(func() { Elevate = old })

	if _, err := List(); err != nil {
		t.Fatal(err)
	}
	if _, err := Apply("up", "0199ab12-3456-7def-8abc-0123456789ab", "", ""); err == nil {
		t.Fatal("la elevación falla")
	}
	if _, err := List(); err != nil || runs() != 2 {
		t.Errorf("Apply debe invalidar la caché: %v (%d ejecuciones)", err, runs())
	}
}

func TestRunReadTimesOutOnAHungCLIButElevatedRunDoesNot(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("CLI de shell")
	}
	old := ReadTimeout
	ReadTimeout = 200 * time.Millisecond
	t.Cleanup(func() { ReadTimeout = old })

	start := time.Now()
	_, err := Run("/bin/sh", "-c", "sleep 30; echo tarde") // el nieto `sleep` retiene las tuberías
	if err == nil || !strings.Contains(err.Error(), "tiempo de espera") {
		t.Fatalf("un CLI colgado debe devolver error de plazo: %v", err)
	}
	if d := time.Since(start); d > 5*time.Second {
		t.Errorf("no debe bloquear: %v", d)
	}

	out, err := RunElevated("/bin/sh", "-c", "sleep 0.5; echo hecho")
	if err != nil || out != "hecho" {
		t.Errorf("la operación elevada no tiene plazo de lectura: %q %v", out, err)
	}
}
