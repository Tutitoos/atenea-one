// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package winstate recuerda el tamaño de la ventana principal entre ejecuciones: {"width": int, "height": int}. No
// guarda la posición: restaurarla fuera de pantalla (monitor desconectado) es un riesgo que no se comprueba aquí.
package winstate

import (
	"encoding/json"
	"os"
	"path/filepath"
)

// File es el nombre del fichero dentro de la carpeta de configuración de la aplicación.
const File = "window.json"

// Límites válidos. El mínimo coincide con el de la ventana (main.go); el máximo evita restaurar tamaños absurdos.
const (
	MinWidth  = 760
	MinHeight = 460
	MaxSize   = 4000
)

// Size es el tamaño de la ventana en píxeles lógicos.
type Size struct {
	Width  int `json:"width"`
	Height int `json:"height"`
}

// Valid dice si el tamaño está dentro de los límites.
func (s Size) Valid() bool {
	return s.Width >= MinWidth && s.Width <= MaxSize && s.Height >= MinHeight && s.Height <= MaxSize
}

// Load lee el tamaño guardado. Fichero ausente, corrupto o fuera de rango: false (se usan los valores por defecto).
func Load(path string) (Size, bool) {
	b, err := os.ReadFile(path)
	if err != nil {
		return Size{}, false
	}
	var s Size
	if err := json.Unmarshal(b, &s); err != nil || !s.Valid() {
		return Size{}, false
	}
	return s, true
}

// Save escribe el tamaño de forma atómica (temporal + rename, permisos 0600). Rechaza tamaños fuera de rango para no
// pisar un buen valor con el 0×0 de una ventana ya destruida.
func Save(path string, s Size) error {
	if !s.Valid() {
		return os.ErrInvalid
	}
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	body, err := json.Marshal(s)
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, File+".*.tmp") // CreateTemp ya usa 0600
	if err != nil {
		return err
	}
	_, werr := tmp.Write(body)
	if cerr := tmp.Close(); werr == nil {
		werr = cerr
	}
	if werr == nil {
		werr = os.Rename(tmp.Name(), path)
	}
	if werr != nil {
		_ = os.Remove(tmp.Name())
	}
	return werr
}
