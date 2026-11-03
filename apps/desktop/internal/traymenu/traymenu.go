// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package traymenu decide qué muestra el menú de la bandeja y qué hace cada clic (ADR-019, fase 3). No depende de
// Wails: `tray.go` solo traduce estos elementos a entradas de menú nativas, así que todo lo que decide algo se prueba
// sin bandeja. Lo que no se puede probar aquí es que el sistema entregue el clic.
package traymenu

import (
	"fmt"
	"sync"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/health"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/trayaction"
)

// Identificadores fijos de las entradas que no son por red.
const (
	IDOpen = "open"
	IDQuit = "quit"
)

// Item es una entrada del menú. Comparable: la bandeja solo reconstruye el menú si cambia la lista (no cierra un
// menú abierto).
type Item struct {
	ID      string
	Text    string
	Enabled bool
}

// Items ordena el menú: una línea de estado por red que responde (informativa, no clicable), las acciones por red,
// «Abrir Atenea One» y «Salir».
func Items(lines []health.Line, actions []trayaction.Entry) []Item {
	items := make([]Item, 0, len(lines)+len(actions)+2)
	for _, l := range lines {
		items = append(items, Item{ID: "state:" + l.ID, Text: l.Text})
	}
	for _, a := range actions {
		items = append(items, Item{ID: a.ID, Text: a.Text, Enabled: true})
	}
	return append(items,
		Item{ID: IDOpen, Text: "Abrir Atenea One", Enabled: true},
		Item{ID: IDQuit, Text: "Salir", Enabled: true},
	)
}

// Handler ejecuta los clics. Todo lo que toca el sistema entra por campos, para sustituirlo en las pruebas.
type Handler struct {
	// Apply conecta (`up`) o desconecta (`down`) una red; el sistema pide la autorización en cada operación.
	Apply func(op, network string) error
	// Copy copia una IPv4 al portapapeles.
	Copy func(address string) error
	Show func()
	Quit func()
	// Log recibe los errores de las acciones (no hay a quién devolverlos: el clic no tiene respuesta).
	Log func(message string)

	busy sync.Map
}

// Do ejecuta el clic de una entrada. Bloquea hasta terminar (puede esperar a un diálogo de autorización), así que
// quien lo llama debe hacerlo fuera del hilo de la interfaz; Wails ya lanza cada clic en su propia goroutine.
// Una acción por red que ya está en marcha ignora un segundo clic (no apila diálogos de autorización).
// Un identificador desconocido no hace nada.
func (h *Handler) Do(id string) {
	switch id {
	case IDOpen:
		h.Show()
		return
	case IDQuit:
		h.Quit()
		return
	}
	op, arg, ok := trayaction.Split(id)
	if !ok {
		return
	}
	var run func() error
	switch op {
	case "up", "down":
		run = func() error { return h.Apply(op, arg) }
	case "copy":
		run = func() error { return h.Copy(arg) }
	default:
		return
	}
	if _, running := h.busy.LoadOrStore(id, struct{}{}); running {
		return
	}
	defer h.busy.Delete(id)
	if err := run(); err != nil {
		h.Log(fmt.Sprintf("acción de bandeja %s: %v", op, err))
	}
}
