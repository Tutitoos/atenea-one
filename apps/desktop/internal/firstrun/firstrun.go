// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package firstrun decide si esta es la primera vez que la aplicación se abre para este usuario (ADR-021). La señal es
// un fichero de usuario, `first-run.json`, que escribe este paquete: la interfaz no interviene ni hace falta ningún
// método vinculado. No guarda credenciales ni datos de ninguna red.
package firstrun

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
)

// File es el nombre del fichero dentro de la carpeta de configuración de la aplicación.
const File = "first-run.json"

// Claim devuelve true solo si ESTA llamada es la primera: crea el fichero de forma exclusiva (O_EXCL), así que si dos
// instancias arrancan a la vez solo una lo consigue. El fichero se escribe ANTES de abrir la ventana: si no se puede
// escribir (permisos, disco lleno) devuelve false y el error, y la ventana no se abre; así un fallo no repite la
// ventana en cada arranque. Cualquier fichero existente cuenta como «ya ocurrió», sea cual sea su contenido.
func Claim(dir string) (bool, error) {
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return false, err
	}
	f, err := os.OpenFile(filepath.Join(dir, File), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if errors.Is(err, fs.ErrExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	_, werr := f.WriteString("{\"shown\":true}\n")
	if cerr := f.Close(); werr == nil {
		werr = cerr
	}
	if werr != nil {
		return false, werr
	}
	return true, nil
}
