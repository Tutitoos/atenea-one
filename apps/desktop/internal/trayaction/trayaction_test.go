// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package trayaction

import (
	"errors"
	"reflect"
	"runtime"
	"testing"
)

func TestLabelsFollowNetworkStateAndIPv4(t *testing.T) {
	net := func(state string) map[string]any {
		return map[string]any{"network_id": "id", "name": "Red de prueba", "state": state}
	}
	status := map[string]any{"addrs": []any{"fd00::1", "basura", "10.50.0.1"}}
	want := []Entry{{"up:id", "Conectar Red de prueba"}, {"copy:10.50.0.1", "Copiar mi IP · Red de prueba"}}
	if got := Entries(net("detenida"), status, true); !reflect.DeepEqual(got, want) {
		t.Fatalf("got %+v", got)
	}
	for _, state := range []string{"conectada", "conflicto", "iniciada_sin_configuracion"} {
		want := []Entry{{"down:id", "Desconectar Red de prueba"}}
		if got := Entries(net(state), nil, false); !reflect.DeepEqual(got, want) {
			t.Fatalf("%s: got %+v", state, got)
		}
	}
	if got := Entries(net("conectada"), status, false); len(got) != 1 {
		t.Fatalf("sin portapapeles: %+v", got)
	}
	got := Entries(map[string]any{"network_id": "x"}, nil, true)
	if !reflect.DeepEqual(got, []Entry{{"up:x", "Conectar Red"}}) {
		t.Fatalf("por defecto: %+v", got)
	}
}

func TestSplit(t *testing.T) {
	if op, arg, ok := Split("copy:10.50.0.1"); op != "copy" || arg != "10.50.0.1" || !ok {
		t.Fatal(op, arg, ok)
	}
	if op, arg, ok := Split("a:b:c"); op != "a" || arg != "b:c" || !ok {
		t.Fatal(op, arg, ok)
	}
	if _, _, ok := Split("sin-dos-puntos"); ok {
		t.Fatal("debía ser ok=false")
	}
}

func TestClipboardProgramPerOS(t *testing.T) {
	only := func(found ...string) func(string) (string, error) {
		return func(file string) (string, error) {
			for _, f := range found {
				if f == file {
					return "/bin/" + file, nil
				}
			}
			return "", errors.New("no encontrado")
		}
	}
	cases := []struct {
		goos  string
		found []string
		want  string
	}{
		{"darwin", []string{"pbcopy", "xclip"}, "/bin/pbcopy"},
		{"darwin", nil, ""},
		{"linux", []string{"wl-copy", "xclip"}, "/bin/wl-copy"},
		{"linux", []string{"xclip"}, "/bin/xclip"},
		{"linux", nil, ""},
		{"windows", []string{"pbcopy", "wl-copy", "xclip"}, ""},
	}
	for _, c := range cases {
		if got := ClipboardProgram(c.goos, only(c.found...)); got != c.want {
			t.Errorf("%s %v: got %q", c.goos, c.found, got)
		}
	}
}

func TestCopyUsesInjectedExec(t *testing.T) {
	oldLook, oldExec := LookPath, Exec
	t.Cleanup(func() { LookPath, Exec = oldLook, oldExec })
	var program, stdin string
	var args []string
	var calls int
	Exec = func(p string, a []string, s string) error {
		calls++
		program, args, stdin = p, a, s
		return nil
	}
	LookPath = func(file string) (string, error) { return "/usr/bin/" + file, nil }
	if Available() != (runtime.GOOS == "darwin" || runtime.GOOS == "linux") {
		t.Fatalf("Available inesperado en %s", runtime.GOOS)
	}
	if runtime.GOOS != "darwin" && runtime.GOOS != "linux" {
		return
	}
	if err := Copy("10.50.0.1"); err != nil || stdin != "10.50.0.1" {
		t.Fatalf("copy: %v stdin=%q", err, stdin)
	}
	if runtime.GOOS == "darwin" && (program != "/usr/bin/pbcopy" || args != nil) {
		t.Fatalf("darwin: %s %v", program, args)
	}
	if runtime.GOOS == "linux" && program != "/usr/bin/wl-copy" {
		t.Fatalf("linux: %s", program)
	}
	if runtime.GOOS == "linux" {
		LookPath = func(file string) (string, error) {
			if file == "xclip" {
				return "/usr/bin/xclip", nil
			}
			return "", errors.New("no")
		}
		if err := Copy("10.0.0.2"); err != nil || !reflect.DeepEqual(args, []string{"-selection", "clipboard"}) {
			t.Fatalf("xclip: %v %v", err, args)
		}
	}
	calls = 0
	for _, bad := range []string{"10.0.0.1; id", "fd00::1", "10.0.0.1%x", "", "::ffff:10.0.0.1"} {
		if err := Copy(bad); err == nil || err.Error() != "IP no válida" {
			t.Errorf("Copy(%q) = %v", bad, err)
		}
	}
	if calls != 0 {
		t.Fatal("no debía ejecutar nada con IP inválida")
	}
	Exec = func(string, []string, string) error { return errors.New("fallo") }
	if err := Copy("10.0.0.1"); err == nil || err.Error() != "no se pudo copiar la IP" {
		t.Fatalf("exec: %v", err)
	}
	LookPath = func(string) (string, error) { return "", errors.New("no") }
	if err := Copy("10.0.0.1"); err == nil || err.Error() != "portapapeles no disponible" {
		t.Fatalf("sin portapapeles: %v", err)
	}
}
