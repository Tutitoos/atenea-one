// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package app

import (
	"runtime"
	"slices"
	"testing"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/backend"
)

func TestAdminApplyRunsTheExactCommandOnlyForAJoinedNetwork(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("en Windows no está soportada")
	}
	list := fakeCLI(t)
	writeList(t, list, `[{"network_id":"`+id1+`","name":"casa"}]`)
	var scripts []string
	old := backend.Elevate
	backend.Elevate = func(script, prompt string) (string, error) { scripts = append(scripts, script); return "ok", nil }
	t.Cleanup(func() { backend.Elevate = old })
	oldCheck := backend.CheckCLI
	backend.CheckCLI = func(string) error { return nil }
	t.Cleanup(func() { backend.CheckCLI = oldCheck })
	a := &App{}

	fp := "1234-5678-9abc-def0-1234-5678-9abc-def0"
	if _, err := a.AdminApply("approve", id1, backend.AdminParams{Fingerprint: fp}); err != nil {
		t.Fatal(err)
	}
	if want := []string{backend.CLIPath() + " network approve " + fp + " --network " + id1}; !slices.Equal(scripts, want) {
		t.Fatalf("órdenes = %v, esperaba %v", scripts, want)
	}
	scripts = nil
	if _, err := a.AdminApply("approve", id2, backend.AdminParams{Fingerprint: fp}); err == nil || len(scripts) != 0 {
		t.Errorf("red no unida: err=%v órdenes=%v", err, scripts)
	}
	if _, err := a.AdminApply("approve", "no-es-uuid", backend.AdminParams{Fingerprint: fp}); err == nil || len(scripts) != 0 {
		t.Errorf("red no válida: err=%v órdenes=%v", err, scripts)
	}
}
