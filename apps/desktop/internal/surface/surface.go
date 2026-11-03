// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package surface lista los métodos exportados de un servicio vinculado. En Wails todo método exportado de un
// servicio registrado es invocable desde el JavaScript de la página (spike S2 de ADR-019): una prueba compara
// esta lista con la permitida, de modo que añadir un método sin revisión rompa la compilación de pruebas.
package surface

import (
	"reflect"
	"sort"
)

// Exported devuelve, ordenados, los nombres de los métodos exportados de v.
func Exported(v any) []string {
	t := reflect.TypeOf(v)
	names := make([]string, 0, t.NumMethod())
	for i := range t.NumMethod() {
		names = append(names, t.Method(i).Name)
	}
	sort.Strings(names)
	return names
}
