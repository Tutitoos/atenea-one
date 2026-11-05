// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package traymenu decide qué muestra el menú de la bandeja y qué hace cada clic (ADR-019, fase 3). No depende de
// Wails: `tray.go` solo traduce estos elementos a entradas de menú nativas, así que todo lo que decide algo se prueba
// sin bandeja. Lo que no se puede probar aquí es que el sistema entregue el clic.
package traymenu

// Identificadores de las entradas del menú.
const (
	IDOpen = "open"
	IDQuit = "quit"
)

// Item es una entrada del menú.
type Item struct {
	ID      string
	Text    string
	Enabled bool
}

// Items es el menú de la bandeja: «Abrir Atenea One» y «Salir». Las acciones por red (conectar, desconectar, copiar
// la IP) están en la ventana, no aquí; el estado de las redes lo da el icono y su descripción.
func Items() []Item {
	return []Item{
		{ID: IDOpen, Text: "Abrir Atenea One", Enabled: true},
		{ID: IDQuit, Text: "Salir", Enabled: true},
	}
}

// Handler ejecuta los clics. Todo lo que toca el sistema entra por campos, para sustituirlo en las pruebas.
type Handler struct {
	Show func()
	Quit func()
}

// Do ejecuta el clic de una entrada. Un identificador desconocido no hace nada.
func (h *Handler) Do(id string) {
	switch id {
	case IDOpen:
		h.Show()
	case IDQuit:
		h.Quit()
	}
}
