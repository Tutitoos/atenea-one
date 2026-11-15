// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

package backend

import (
	"path/filepath"

	"golang.org/x/sys/windows"
)

// windowsCLIPath es `%ProgramFiles%\Atenea\atenea.exe`, la carpeta que el instalador protege (ADR-023). Sale del
// sistema (carpeta conocida), no de la variable de entorno ProgramFiles: la aplicación no confía en lo que herede.
func windowsCLIPath() string {
	dir, err := windows.KnownFolderPath(windows.FOLDERID_ProgramFiles, 0)
	if err != nil {
		return ""
	}
	return filepath.Join(dir, "Atenea", "atenea.exe")
}
