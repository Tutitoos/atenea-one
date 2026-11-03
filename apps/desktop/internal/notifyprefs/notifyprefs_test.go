// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package notifyprefs

import (
	"os"
	"path/filepath"
	"testing"
)

func TestPreferenceDefaultsAndRoundTrips(t *testing.T) {
	path := filepath.Join(t.TempDir(), "atenea-notify-test")
	if on, err := Get(path); err != nil || !on {
		t.Fatalf("defecto = %v, %v", on, err)
	}
	if err := Set(path, false); err != nil {
		t.Fatal(err)
	}
	if on, err := Get(path); err != nil || on {
		t.Fatalf("tras false = %v, %v", on, err)
	}
	if err := Set(path, true); err != nil {
		t.Fatal(err)
	}
	if on, err := Get(path); err != nil || !on {
		t.Fatalf("tras true = %v, %v", on, err)
	}
	if _, err := os.Stat(path + ".json.tmp"); err == nil {
		t.Fatal("el temporal debe renombrarse")
	}
}

func TestSetCreatesParentsAndGetRejectsInvalidContent(t *testing.T) {
	path := filepath.Join(t.TempDir(), "a", "b", "notify.json")
	if err := Set(path, false); err != nil {
		t.Fatal(err)
	}
	for _, bad := range []string{`{"notifications":"si"}`, `{}`, `5`, `[]`} {
		os.WriteFile(path, []byte(bad), 0o644)
		if _, err := Get(path); err == nil || err.Error() != "preferencia de notificaciones no válida" {
			t.Errorf("%s: %v", bad, err)
		}
	}
	os.WriteFile(path, []byte("{"), 0o644)
	if _, err := Get(path); err == nil {
		t.Error("JSON roto debía fallar")
	}
}
