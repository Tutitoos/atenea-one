// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"reflect"
	"testing"

	"github.com/wailsapp/wails/v3/pkg/application"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/surface"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/traymenu"
)

// Guardia de ADR-019 §2 y decisión 10, ampliada por ADR-021: la superficie que la página puede invocar es exactamente
// esta. Añadir un método exportado al servicio obliga a tocar esta lista, es decir, a revisarlo.
func TestServiceSurfaceIsExactlyTheFourteenOperations(t *testing.T) {
	want := []string{"AutostartGet", "AutostartSet", "ClientStatus", "DevicePing", "NetworkApply", "NetworkInspect",
		"NetworkLegacy", "NetworkList", "NotificationsGet", "NotificationsSet", "OpenPanel", "OpenService", "ServicesPublishSet", "Status"}
	if got := surface.Exported(&AteneaService{}); !reflect.DeepEqual(got, want) {
		t.Fatalf("superficie del servicio = %v\nesperada             = %v", got, want)
	}
}

func TestNativeMenuMirrorsTheItemsWithTheirEnabledState(t *testing.T) {
	items := traymenu.Items()
	menu := application.NewMenu()
	fillMenu(menu, items, func(string) {})
	for _, item := range items {
		entry := menu.FindByLabel(item.Text)
		if entry == nil {
			t.Fatalf("falta la entrada %q", item.Text)
		}
		if entry.Enabled() != item.Enabled {
			t.Errorf("%q: habilitada = %v, esperada %v", item.Text, entry.Enabled(), item.Enabled)
		}
	}
}
