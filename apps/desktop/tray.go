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
)

// poll es el intervalo de lectura de la bandeja (el mismo que la versión de Tauri).
const poll = 2 * time.Second

// startTray crea la bandeja: el icono lleva un punto del color de la salud y el menú, una línea por red que
// responde, «Abrir Atenea One» y «Salir». Solo se reconstruye el menú si cambian sus líneas (no cierra un menú
// abierto) y solo se cambia el icono y el texto si cambian. Las acciones por red (Conectar, Desconectar, Copiar
// mi IP) y los avisos del sistema son la fase 3 de ADR-019.
func startTray(wails *application.App, core *app.App, base []byte, show, quit func()) {
	tray := wails.SystemTray.New()
	tray.SetTooltip("Atenea One")
	setIcon := func(h health.Health) {
		if png, err := trayicon.WithDot(base, h); err == nil {
			tray.SetIcon(png)
		}
	}
	menu := func(lines []health.Line) *application.Menu {
		m := wails.NewMenu()
		for _, line := range lines {
			m.Add(line.Text).SetEnabled(false)
		}
		m.Add("Abrir Atenea One").OnClick(func(*application.Context) { show() })
		m.Add("Salir").OnClick(func(*application.Context) { quit() })
		return m
	}
	current := []health.Line{{ID: "*", Text: "Comprobando…"}}
	level, text := health.Stopped, "Atenea One"
	setIcon(level)
	tray.SetMenu(menu(current))
	go func() {
		state := &app.TrayState{}
		for {
			h, t, lines := core.Poll(state)
			if !slices.Equal(lines, current) {
				tray.SetMenu(menu(lines))
				current = lines
			}
			if h != level {
				setIcon(h)
				level = h
			}
			if t != text {
				tray.SetTooltip(t)
				text = t
			}
			time.Sleep(poll)
		}
	}()
}
