// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"reflect"
	"testing"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/surface"
)

// Guardia de ADR-019 §2 y decisión 10: la superficie que la página puede invocar es exactamente esta. Añadir un
// método exportado al servicio obliga a tocar esta lista, es decir, a revisarlo.
func TestServiceSurfaceIsExactlyTheElevenOperations(t *testing.T) {
	want := []string{"AutostartGet", "AutostartSet", "DevicePing", "NetworkApply", "NetworkInspect", "NetworkLegacy",
		"NetworkList", "NotificationsGet", "NotificationsSet", "OpenPanel", "Status"}
	if got := surface.Exported(&AteneaService{}); !reflect.DeepEqual(got, want) {
		t.Fatalf("superficie del servicio = %v\nesperada             = %v", got, want)
	}
}
