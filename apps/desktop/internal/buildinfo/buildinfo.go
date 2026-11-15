// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package buildinfo da a la aplicación su versión y su commit (ADR-025). Se incrustan al compilar con `-X`
// (`packaging/desktop/build-binary.sh`); sin ellos valen los valores de desarrollo. El agente de cada equipo ejecuta
// `atenea-desktop --version` desde una ruta fija para informar qué versión de la aplicación hay instalada.
package buildinfo

import (
	"fmt"
	"io"
)

// Name es el nombre que imprime la línea de versión, igual que `atenea --version` y `atenea-agent --version`.
const Name = "atenea-desktop"

// Version y Commit se sustituyen con `-ldflags "-X …/buildinfo.Version=… -X …/buildinfo.Commit=…"`.
var (
	Version = "0.0.0-dev"
	Commit  = "unknown"
)

// Line es `atenea-desktop <versión> (<commit>)`, el mismo formato que los binarios de Rust.
func Line() string {
	return fmt.Sprintf("%s %s (%s)", Name, Version, Commit)
}

// Handle responde a `--version` o `-V` (como único argumento): escribe la línea y devuelve true para que el programa
// salga antes de crear ventana, bandeja ni servicio. Con cualquier otra cosa devuelve false y no escribe nada.
func Handle(args []string, out io.Writer) bool {
	if len(args) != 1 || (args[0] != "--version" && args[0] != "-V") {
		return false
	}
	_, _ = fmt.Fprintln(out, Line())
	return true
}
