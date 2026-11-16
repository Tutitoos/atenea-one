// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package buildinfo

import (
	"bytes"
	"regexp"
	"testing"
)

// El agente lee esta línea con una expresión estricta (ADR-025): la forma no puede cambiar sin cambiar también al lector.
var strictLine = regexp.MustCompile(`^atenea-desktop [0-9A-Za-z.+-]{1,32} \((?:[0-9a-f]{7,40}|unknown)\)\n$`)

func TestVersionFlagPrintsOneStrictLineAndAsksToExit(t *testing.T) {
	for _, flag := range []string{"--version", "-V"} {
		var out bytes.Buffer
		if !Handle([]string{flag}, &out) {
			t.Fatalf("%s debía atenderse", flag)
		}
		if !strictLine.Match(out.Bytes()) {
			t.Fatalf("%s imprimió %q", flag, out.String())
		}
	}
}

func TestOtherArgumentsNeverPrintOrExit(t *testing.T) {
	// `--open` es la opción real del acceso directo; el resto prueba que ni los argumentos extra ni los parecidos
	// se confunden con la petición de versión (la aplicación seguiría arrancando con normalidad).
	for _, args := range [][]string{nil, {}, {"--open"}, {"--version", "--open"}, {"--open", "--version"}, {"-v"}, {"--versions"}, {"version"}} {
		var out bytes.Buffer
		if Handle(args, &out) || out.Len() != 0 {
			t.Errorf("%q no debía atenderse y escribió %q", args, out.String())
		}
	}
}

func TestInjectedValuesAppearInTheLine(t *testing.T) {
	oldVersion, oldCommit := Version, Commit
	t.Cleanup(func() { Version, Commit = oldVersion, oldCommit })
	Version, Commit = "0.1.31", "dfe72aefc123edc288d1daead1c812825e0a2a00"
	if got, want := Line(), "atenea-desktop 0.1.31 (dfe72aefc123edc288d1daead1c812825e0a2a00)"; got != want {
		t.Fatalf("%q != %q", got, want)
	}
}

// El agente (Rust) busca esta cadena en el binario instalado antes de ejecutarlo con --version: una aplicación anterior a
// la opción arrancaría entera. Cambiarla exige cambiar `DESKTOP_MARKER` en apps/agent/src/versions.rs en el mismo cambio.
func TestMarkerIsTheOneTheAgentSearchesFor(t *testing.T) {
	if Marker != "ATENEA-DESKTOP-VERSION-FLAG-1" {
		t.Fatalf("la marca cambió a %q sin actualizar al agente", Marker)
	}
}
