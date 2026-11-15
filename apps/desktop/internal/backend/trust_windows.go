// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

package backend

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
)

// trustedPath: la aplicación solo eleva el CLI de la carpeta que protege el instalador, `%ProgramFiles%\Atenea\atenea.exe`
// (ADR-017 §7-§8). La ruta sale de la carpeta conocida del sistema y se compara tal cual: ATENEA_CLI no sirve para una
// operación con UAC. Tampoco puede ser un enlace.
// shortcut: no lee la DACL del fichero (exigiría FFI de seguridad en la aplicación): confía en la que fija el instalador
// y comprueba `tests/windows/installer-checks.ps1`; si alguien relaja esa carpeta, la elevación lo hereda.
func trustedPath(cli string) error {
	want := windowsCLIPath()
	if want == "" || !strings.EqualFold(filepath.Clean(cli), filepath.Clean(want)) {
		return errors.New("el CLI no está en la carpeta de Archivos de programa de Atenea: no se eleva")
	}
	st, err := os.Lstat(cli)
	if err != nil {
		return errors.New("el CLI de Atenea no está instalado")
	}
	if !st.Mode().IsRegular() {
		return errors.New("el CLI de Atenea no es un fichero normal: no se eleva")
	}
	return nil
}
