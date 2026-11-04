// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package firstrun

import (
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"sync/atomic"
	"testing"
)

func TestOnlyTheFirstClaimWinsAndTheFileSurvives(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "one.atenea.desktop") // la carpeta aún no existe
	if first, err := Claim(dir); err != nil || !first {
		t.Fatalf("primera llamada: %v, %v", first, err)
	}
	for range 3 {
		if first, err := Claim(dir); err != nil || first {
			t.Fatalf("las siguientes no son la primera: %v, %v", first, err)
		}
	}
	if _, err := os.Stat(filepath.Join(dir, File)); err != nil {
		t.Fatalf("el fichero debe quedar: %v", err)
	}
}

func TestConcurrentLaunchesProduceExactlyOneFirstRun(t *testing.T) {
	dir := t.TempDir()
	var wins atomic.Int32
	var wg sync.WaitGroup
	for range 32 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if first, err := Claim(dir); err == nil && first {
				wins.Add(1)
			}
		}()
	}
	wg.Wait()
	if wins.Load() != 1 {
		t.Fatalf("ganadores = %d, esperaba exactamente 1", wins.Load())
	}
}

func TestAnyExistingFileCountsAsAlreadyShownWhateverItContains(t *testing.T) {
	for name, content := range map[string]string{"vacío": "", "basura": "\x00\xffno es json", "otro": `{"shown":false}`} {
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, File), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
		if first, err := Claim(dir); err != nil || first {
			t.Errorf("%s: %v, %v", name, first, err)
		}
	}
}

func TestWhenTheFileCannotBeWrittenTheWindowIsNotOpenedAndTheErrorIsReturned(t *testing.T) {
	if runtime.GOOS == "windows" || os.Geteuid() == 0 {
		t.Skip("los permisos de directorio no se comportan igual (Windows o root)")
	}
	dir := t.TempDir()
	if err := os.Chmod(dir, 0o500); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.Chmod(dir, 0o700) })
	if first, err := Claim(dir); err == nil || first {
		t.Fatalf("sin permiso de escritura: %v, %v (no debe abrir la ventana y debe avisar)", first, err)
	}
	// Y no repite: tras el fallo no queda fichero a medias que cuente como «ya ocurrió».
	if _, err := os.Stat(filepath.Join(dir, File)); err == nil {
		t.Error("no debe quedar fichero si no se pudo crear")
	}
}

func TestADirectoryInsteadOfAConfigFolderFailsWithoutOpening(t *testing.T) {
	file := filepath.Join(t.TempDir(), "es-un-fichero")
	if err := os.WriteFile(file, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	if first, err := Claim(file); err == nil || first {
		t.Fatalf("%v, %v", first, err)
	}
}
