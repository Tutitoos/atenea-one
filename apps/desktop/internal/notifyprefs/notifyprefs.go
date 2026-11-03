// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package notifyprefs guarda la preferencia de notificaciones: {"notifications": bool}.
package notifyprefs

import (
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

// Get devuelve true si el fichero aún no existe.
func Get(path string) (bool, error) {
	b, err := os.ReadFile(path)
	if errors.Is(err, fs.ErrNotExist) {
		return true, nil
	}
	if err != nil {
		return false, err
	}
	var v any
	if err := json.Unmarshal(b, &v); err != nil {
		return false, err
	}
	m, _ := v.(map[string]any)
	on, ok := m["notifications"].(bool)
	if !ok {
		return false, errors.New("preferencia de notificaciones no válida")
	}
	return on, nil
}

// Set escribe la preferencia de forma atómica.
func Set(path string, on bool) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	// Como Path::with_extension("json.tmp") del Rust: sustituye la última extensión.
	base := filepath.Base(path)
	stem := path
	if ext := filepath.Ext(base); ext != "" && ext != base {
		stem = strings.TrimSuffix(path, ext)
	}
	tmp := stem + ".json.tmp"
	body := `{"notifications":false}`
	if on {
		body = `{"notifications":true}`
	}
	if err := os.WriteFile(tmp, []byte(body), 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}
