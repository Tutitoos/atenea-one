// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package autostart

import (
	"encoding/xml"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func want(t *testing.T, goos, home string, on bool) {
	t.Helper()
	got, err := getFor(goos, home, "")
	if err != nil || got == nil || *got != on {
		t.Fatalf("getFor = %v, %v; esperado %v", got, err, on)
	}
}

func TestEnableDisableRoundTripsAndIsIdempotent(t *testing.T) {
	exe := "/Applications/Atenea One.app/Contents/MacOS/atenea-desktop"
	for _, goos := range []string{"darwin", "linux"} {
		h := t.TempDir()
		want(t, goos, h, false)
		if err := setFor(goos, h, "", exe, false); err != nil { // desactivar sin fichero no falla
			t.Fatal(err)
		}
		for range 2 { // reactivar reescribe sin error
			if err := setFor(goos, h, "", exe, true); err != nil {
				t.Fatal(err)
			}
		}
		want(t, goos, h, true)
		b, err := os.ReadFile(entryPath(goos, h, ""))
		if err != nil {
			t.Fatal(err)
		}
		written := string(b)
		if !strings.Contains(written, "Atenea One.app/Contents/MacOS/atenea-desktop") ||
			!(strings.Contains(written, "RunAtLoad") || strings.Contains(written, `Exec="`)) {
			t.Fatalf("%s: contenido inesperado:\n%s", goos, written)
		}
		if goos == "darwin" { // XML bien formado (el Rust lo valida con plutil -lint)
			d := xml.NewDecoder(strings.NewReader(written))
			for {
				if _, err := d.Token(); err == io.EOF {
					break
				} else if err != nil {
					t.Fatalf("plist inválido: %v", err)
				}
			}
		}
		if _, err := os.Stat(strings.TrimSuffix(entryPath(goos, h, ""), filepath.Ext(entryPath(goos, h, ""))) + ".tmp"); err == nil {
			t.Fatal("el temporal debe renombrarse")
		}
		if err := setFor(goos, h, "", exe, false); err != nil {
			t.Fatal(err)
		}
		want(t, goos, h, false)
	}
}

func TestEnablingReplacesAStaleEntryWithTheCurrentExecutable(t *testing.T) {
	for _, goos := range []string{"darwin", "linux"} {
		h := t.TempDir()
		path := entryPath(goos, h, "")
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		os.WriteFile(path, []byte("antiguo /otra/ruta"), 0o644)
		if err := setFor(goos, h, "", "/opt/atenea/atenea-desktop", true); err != nil {
			t.Fatal(err)
		}
		b, _ := os.ReadFile(path)
		if !strings.Contains(string(b), "/opt/atenea/atenea-desktop") || strings.Contains(string(b), "antiguo") {
			t.Fatalf("%s: %s", goos, b)
		}
	}
}

func TestExecutablePathMustBeAbsoluteAndFreeOfSyntaxCharacters(t *testing.T) {
	for _, bad := range []string{"relativa/atenea", "", "/a\"b", "/a\\b", "/a%u", "/a$HOME", "/a`id`", "/a\nb", "/a<b", "/a&b", "/a'b", "/a\xffb"} {
		if _, err := checkedExe(bad); err == nil {
			t.Errorf("%q debía rechazarse", bad)
		}
	}
	if _, err := checkedExe("relativa"); err == nil || err.Error() != "ruta del ejecutable no válida" {
		t.Errorf("mensaje: %v", err)
	}
	if _, err := checkedExe("/a%u"); err == nil || err.Error() != "la ruta del ejecutable contiene caracteres no admitidos" {
		t.Errorf("mensaje: %v", err)
	}
	if _, err := checkedExe("/Applications/Atenea One.app/Contents/MacOS/x"); err != nil {
		t.Error(err)
	}
	h := t.TempDir()
	if err := setFor("linux", h, "", "/a$b", true); err == nil {
		t.Error("ruta mala debía fallar")
	}
	if _, err := os.Stat(entryPath("linux", h, "")); err == nil {
		t.Error("no debe crearse fichero con una ruta mala")
	}
}

func TestPathsPerOSAndXDGConfigHomeOnlyWhenAbsolute(t *testing.T) {
	h := t.TempDir()
	if got := entryPath("darwin", h, ""); got != filepath.Join(h, "Library/LaunchAgents/one.atenea.desktop.plist") {
		t.Errorf("darwin: %s", got)
	}
	if got := entryPath("linux", h, "relativo"); !strings.HasPrefix(got, filepath.Join(h, ".config")) {
		t.Errorf("relativa: %s", got)
	}
	if got := entryPath("linux", h, "/tmp/xdg"); got != "/tmp/xdg/autostart/one.atenea.desktop.desktop" {
		t.Errorf("absoluta: %s", got)
	}
}

func TestUnsupportedSystemsReportNilAndRefuseToSet(t *testing.T) {
	h := t.TempDir()
	if got, err := getFor("windows", h, ""); got != nil || err != nil {
		t.Fatalf("getFor = %v, %v", got, err)
	}
	if err := setFor("windows", h, "", "/x", true); err == nil {
		t.Fatal("debía fallar")
	}
}
