// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package panel

import (
	"errors"
	"os/exec"
	"reflect"
	"testing"
)

const id = "00000000-0000-0000-0000-000000000001"

func TestBuildsHTTPSRootAndValidatesIDs(t *testing.T) {
	got, err := URL("https://panel.example.test/api?old=x#old", id, id)
	want := "https://panel.example.test/?net=" + id + "&view=Dispositivos"
	if err != nil || got != want {
		t.Fatalf("URL = %q, %v", got, err)
	}
	for _, base := range []string{"http://panel.example.test", "javascript:alert(1)", "https://user:pass@panel.example.test"} {
		if _, err := URL(base, id, ""); err == nil {
			t.Errorf("%q debía rechazarse", base)
		}
	}
	if _, err := URL("https://panel.example.test", "bad", ""); err == nil || err.Error() != "identificador no válido" {
		t.Errorf("red mala: %v", err)
	}
	if _, err := URL("https://panel.example.test", id, "bad"); err == nil {
		t.Error("dispositivo malo debía rechazarse")
	}
	if _, err := URL("http://x", id, ""); err == nil || err.Error() != "el panel necesita una URL HTTPS sin credenciales" {
		t.Errorf("mensaje: %v", err)
	}
	if _, err := URL("https://[::1", id, ""); err == nil || err.Error() != "URL del servidor no válida" {
		t.Errorf("mensaje: %v", err)
	}
}

func TestOpenPassesTheURLAsOneArgumentPerOS(t *testing.T) {
	old := Launch
	defer func() { Launch = old }()
	var calls [][]string
	Launch = func(p string, a ...string) error { calls = append(calls, append([]string{p}, a...)); return nil }
	u := "https://panel.example.test/?net=" + id + "&view=Dispositivos; id ^ %PATH%"
	for _, goos := range []string{"darwin", "linux", "windows"} {
		if err := open(goos, u); err != nil {
			t.Fatal(err)
		}
	}
	want := [][]string{{"/usr/bin/open", u}, {"xdg-open", u}, {"rundll32.exe", "url.dll,FileProtocolHandler", u}}
	if !reflect.DeepEqual(calls, want) {
		t.Fatalf("calls = %q", calls)
	}
}

func TestOpenMapsFailures(t *testing.T) {
	old := Launch
	defer func() { Launch = old }()
	Launch = func(string, ...string) error { return &exec.ExitError{} }
	if err := open("linux", "u"); err == nil || err.Error() != "no se pudo abrir el panel" {
		t.Fatalf("salida no nula: %v", err)
	}
	Launch = func(string, ...string) error { return errors.New("no encontrado") }
	if err := open("linux", "u"); err == nil || err.Error() != "no encontrado" {
		t.Fatalf("spawn: %v", err)
	}
}
