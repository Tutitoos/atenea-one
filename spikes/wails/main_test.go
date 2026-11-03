// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"reflect"
	"testing"

	"atenea-wails-spike/internal/surface"
)

// Guardia de ADR-019 §2: la superficie del servicio vinculado es exactamente esta. Añadir un método exportado
// obliga a tocar esta lista, es decir, a revisarlo.
func TestServiceSurface(t *testing.T) {
	want := []string{"NotIntended", "Ping", "Report"}
	if got := surface.Exported(&SpikeService{}); !reflect.DeepEqual(got, want) {
		t.Fatalf("superficie del servicio = %v, esperada %v", got, want)
	}
}
