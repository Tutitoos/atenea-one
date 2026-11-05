// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package winstate

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestIdaYVuelta(t *testing.T) {
	path := filepath.Join(t.TempDir(), "sub", File)
	want := Size{Width: 1100, Height: 700}
	if err := Save(path, want); err != nil {
		t.Fatal(err)
	}
	got, ok := Load(path)
	if !ok || got != want {
		t.Fatalf("Load = %v, %v; quería %v", got, ok, want)
	}
}

func TestLoadInvalido(t *testing.T) {
	dir := t.TempDir()
	cases := map[string]string{
		"corrupto":   "{no es json",
		"vacio":      "",
		"bajo-min":   `{"width":100,"height":100}`,
		"ancho-bajo": `{"width":759,"height":600}`,
		"alto-bajo":  `{"width":900,"height":459}`,
		"enorme":     `{"width":4001,"height":600}`,
		"decimal":    `{"width":900.5,"height":600}`,
		"cadena":     `{"width":"900","height":600}`,
		"faltan":     `{}`,
	}
	for name, body := range cases {
		path := filepath.Join(dir, name+".json")
		if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
		if s, ok := Load(path); ok {
			t.Errorf("%s: Load = %v, true; quería false", name, s)
		}
	}
	if _, ok := Load(filepath.Join(dir, "no-existe.json")); ok {
		t.Error("fichero ausente: quería false")
	}
}

func TestLimitesInclusivos(t *testing.T) {
	path := filepath.Join(t.TempDir(), File)
	for _, s := range []Size{{MinWidth, MinHeight}, {MaxSize, MaxSize}} {
		if err := Save(path, s); err != nil {
			t.Fatal(err)
		}
		if got, ok := Load(path); !ok || got != s {
			t.Errorf("Load = %v, %v; quería %v", got, ok, s)
		}
	}
}

func TestSaveRechazaFueraDeRangoSinPisar(t *testing.T) {
	path := filepath.Join(t.TempDir(), File)
	good := Size{Width: 900, Height: 600}
	if err := Save(path, good); err != nil {
		t.Fatal(err)
	}
	for _, s := range []Size{{}, {MinWidth - 1, 600}, {900, MinHeight - 1}, {MaxSize + 1, 600}} {
		if err := Save(path, s); err == nil {
			t.Errorf("Save(%v) debería fallar", s)
		}
	}
	if got, ok := Load(path); !ok || got != good {
		t.Fatalf("el valor bueno se perdió: %v, %v", got, ok)
	}
}

func TestSaveAtomicoYPermisos(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, File)
	// Un fichero previo con permisos amplios se sustituye por uno 0600 (rename de un temporal nuevo).
	if err := os.WriteFile(path, []byte(`{"width":800,"height":500}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := Save(path, Size{Width: 1000, Height: 650}); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	// Windows no tiene permisos Unix: el modo que informa no es 0600 aunque el fichero sea privado de su usuario.
	if perm := info.Mode().Perm(); runtime.GOOS != "windows" && perm != 0o600 {
		t.Errorf("permisos = %o; quería 600", perm)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 {
		t.Errorf("quedan temporales: %v", entries)
	}
}

func TestSaveSinCarpetaEscribibleNoDejaTemporales(t *testing.T) {
	dir := t.TempDir()
	// El destino es un directorio: el rename falla y el temporal debe limpiarse.
	path := filepath.Join(dir, File)
	if err := os.Mkdir(path, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(path, "x"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := Save(path, Size{Width: 900, Height: 600}); err == nil {
		t.Fatal("debería fallar")
	}
	entries, _ := os.ReadDir(dir)
	if len(entries) != 1 {
		t.Errorf("quedan temporales: %v", entries)
	}
}
