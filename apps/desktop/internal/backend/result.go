// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode"
)

// En Windows la salida de una operación elevada con `Start-Process -Verb RunAs` no se puede capturar, y el proceso
// elevado nunca escribe en una ruta que controle el usuario (ADR-017 §8). El CLI elevado deja el resultado en
// `%ProgramData%\Atenea\ui-results\<nonce>\result.json`, una carpeta que solo SYSTEM y Administradores pueden escribir y
// que `Users` pueden leer. El nonce lo elige la aplicación. El resultado solo aporta el mensaje de error: el estado
// de las redes se vuelve a leer con `network list`, que es la fuente de verdad.

// maxResultBytes acota result.json: es texto de error, no datos.
const maxResultBytes = 4096

// newNonce son 32 dígitos hex aleatorios (128 bits).
func newNonce() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(b[:]), nil
}

// ValidNonce: exactamente 32 hex en minúsculas, lo único que acepta el CLI y lo único que entra en una ruta.
func ValidNonce(s string) bool {
	if len(s) != 32 {
		return false
	}
	for i := range len(s) {
		if c := s[i]; !(c >= '0' && c <= '9') && !(c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}

// resultsDir es `%ProgramData%\Atenea\ui-results`.
func resultsDir() string {
	base := os.Getenv("ProgramData")
	if base == "" {
		base = `C:\ProgramData`
	}
	return filepath.Join(base, "Atenea", "ui-results")
}

type opResult struct {
	OK      bool   `json:"ok"`
	Message string `json:"message"`
}

// readResult lee el resultado de una operación; error si no existe o no es válido. El mensaje se limpia de
// caracteres de control: se muestra al usuario.
func readResult(dir, nonce string) (opResult, error) {
	if !ValidNonce(nonce) {
		return opResult{}, errors.New("identificador de resultado no válido")
	}
	f, err := os.Open(filepath.Join(dir, nonce, "result.json"))
	if err != nil {
		return opResult{}, err
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, maxResultBytes+1))
	if err != nil {
		return opResult{}, err
	}
	if len(data) > maxResultBytes {
		return opResult{}, errors.New("el resultado es demasiado grande")
	}
	var r opResult
	if err := json.Unmarshal(data, &r); err != nil {
		return opResult{}, errors.New("el resultado no es válido")
	}
	r.Message = strings.Map(func(c rune) rune {
		if unicode.IsControl(c) && c != '\n' {
			return -1
		}
		return c
	}, r.Message)
	return r, nil
}
