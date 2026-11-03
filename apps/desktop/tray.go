// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"slices"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/app"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/health"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/trayicon"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/traymenu"
)

// poll es el intervalo de lectura de la bandeja (el mismo que la versión de Tauri).
const poll = 2 * time.Second

// fillMenu traduce los elementos del menú a entradas nativas. Cada clic se reenvía a `do` con el identificador del
// elemento: toda la decisión está en `traymenu`, que se prueba sin bandeja.
func fillMenu(menu *application.Menu, items []traymenu.Item, do func(id string)) {
	for _, item := range items {
		entry := menu.Add(item.Text).SetEnabled(item.Enabled)
		if item.Enabled {
			id := item.ID
			entry.OnClick(func(*application.Context) { do(id) })
		}
	}
}

// startTray crea la bandeja: el icono lleva un punto del color de la salud y el menú, una línea por red que
// responde, sus acciones (Conectar o Desconectar, Copiar mi IP), «Abrir Atenea One» y «Salir». Solo se reconstruye el
// menú si cambian sus elementos (no cierra un menú abierto) y solo se cambia el icono y el texto si cambian.
func startTray(wails *application.App, core *app.App, handler *traymenu.Handler, base []byte) {
	tray := wails.SystemTray.New()
	tray.SetTooltip("Atenea One")
	setIcon := func(h health.Health) {
		if png, err := trayicon.WithDot(base, h); err == nil {
			tray.SetIcon(png)
		}
	}
	menu := func(items []traymenu.Item) *application.Menu {
		m := wails.NewMenu()
		fillMenu(m, items, handler.Do)
		return m
	}
	current := traymenu.Items([]health.Line{{ID: "*", Text: "Comprobando…"}}, nil)
	level, text := health.Stopped, "Atenea One"
	setIcon(level)
	tray.SetMenu(menu(current))
	go func() {
		state := &app.TrayState{}
		for {
			read := core.Poll(state)
			if items := traymenu.Items(read.Lines, read.Actions); !slices.Equal(items, current) {
				tray.SetMenu(menu(items))
				current = items
			}
			if read.Health != level {
				setIcon(read.Health)
				level = read.Health
			}
			if read.Text != text {
				tray.SetTooltip(read.Text)
				text = read.Text
			}
			time.Sleep(poll)
		}
	}()
}
