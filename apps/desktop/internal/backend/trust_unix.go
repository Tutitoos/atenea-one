// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build !windows

package backend

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"syscall"
)

// trustedPath exige que ni el fichero ni ningún directorio de su ruta pueda sustituirlo un usuario sin privilegios: de
// root, sin enlaces simbólicos y sin escritura de grupo ni de otros. Es la regla de `network_service::trusted` del CLI.
func trustedPath(path string) error {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return errors.New("la ruta del CLI debe ser absoluta y sin . ni ..")
	}
	for p := path; ; p = filepath.Dir(p) {
		fi, err := os.Lstat(p)
		if err != nil {
			return err
		}
		if !trustedEntry(fi, p == path, 0) {
			return fmt.Errorf("%s: ruta no confiable para una operación con privilegios (root, sin enlaces ni escritura de grupo/otros)", p)
		}
		if filepath.Dir(p) == p {
			return nil
		}
	}
}

// trustedEntry dice si solo `owner` puede cambiar la entrada: es suya, ni el grupo ni otros pueden escribirla y es un
// fichero regular (la última de la ruta) o un directorio (las demás); un enlace simbólico no es ninguno de los dos.
// `owner` es siempre root; es un parámetro para que las pruebas usen un propietario que pueden crear.
func trustedEntry(fi os.FileInfo, leaf bool, owner uint32) bool {
	st, ok := fi.Sys().(*syscall.Stat_t)
	kind := fi.Mode().IsDir()
	if leaf {
		kind = fi.Mode().IsRegular()
	}
	return ok && kind && st.Uid == owner && fi.Mode().Perm()&0o022 == 0
}
