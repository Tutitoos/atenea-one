// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build !windows

package backend

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestTrustedEntryAcceptsOnlyWhatItsOwnerAloneCanReplace(t *testing.T) {
	dir := t.TempDir()
	me := uint32(os.Getuid()) // el propietario esperado es quien ejecuta la prueba: no se puede crear un fichero de root
	file := filepath.Join(dir, "atenea")
	link := filepath.Join(dir, "enlace")
	if err := os.WriteFile(file, []byte("#!/bin/sh\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(file, link); err != nil {
		t.Fatal(err)
	}
	entry := func(path string, mode os.FileMode) os.FileInfo {
		t.Helper()
		if mode != 0 {
			if err := os.Chmod(path, mode); err != nil {
				t.Fatal(err)
			}
		}
		fi, err := os.Lstat(path)
		if err != nil {
			t.Fatal(err)
		}
		return fi
	}
	for _, tc := range []struct {
		name  string
		fi    os.FileInfo
		leaf  bool
		owner uint32
		want  bool
	}{
		{"fichero del propietario sin escritura de otros", entry(file, 0o755), true, me, true},
		{"escritura de grupo", entry(file, 0o775), true, me, false},
		{"escritura de otros", entry(file, 0o757), true, me, false},
		{"de otro propietario", entry(file, 0o755), true, me + 1, false},
		{"enlace simbólico como fichero", entry(link, 0), true, me, false},
		{"enlace simbólico como directorio", entry(link, 0), false, me, false},
		{"directorio como fichero", entry(dir, 0o755), true, me, false},
		{"directorio del propietario", entry(dir, 0o755), false, me, true},
		{"directorio con escritura de grupo", entry(dir, 0o775), false, me, false},
		{"fichero como directorio", entry(file, 0o755), false, me, false},
	} {
		if got := trustedEntry(tc.fi, tc.leaf, tc.owner); got != tc.want {
			t.Errorf("%s: %v, esperaba %v", tc.name, got, tc.want)
		}
	}
}

func TestTrustedPathAcceptsARootChainAndRefusesWhatItCannotTrust(t *testing.T) {
	// /usr/bin/env es de root bajo directorios de root en un sistema normal; donde no lo sea (contenedor sin root real,
	// distribución con enlaces) no hay cadena de root que recorrer y esa parte no aplica.
	if fi, err := os.Lstat("/usr/bin/env"); err == nil && trustedEntry(fi, true, 0) {
		if err := trustedPath("/usr/bin/env"); err != nil {
			t.Errorf("una cadena de root debe aceptarse: %v", err)
		}
	}

	dir := t.TempDir()
	file := filepath.Join(dir, "atenea")
	if err := os.WriteFile(file, nil, 0o755); err != nil {
		t.Fatal(err)
	}
	// Escribible por todos: se rechaza sea quien sea el propietario (también si la prueba corre como root).
	if err := os.Chmod(file, 0o777); err != nil {
		t.Fatal(err)
	}
	err := trustedPath(file)
	if err == nil || !strings.Contains(err.Error(), file+": ruta no confiable") {
		t.Errorf("un fichero escribible por todos debe rechazarse, nombrándolo: %v", err)
	}
	for _, bad := range []string{"", "atenea", "./atenea", "/usr/bin/../bin/env", "/usr/bin/env/", filepath.Join(dir, "no-existe")} {
		if err := trustedPath(bad); err == nil {
			t.Errorf("%q aceptada", bad)
		}
	}
}
