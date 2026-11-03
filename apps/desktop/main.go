// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Atenea One: aplicación de escritorio (ADR-019, Wails v3). Vive en la bandeja del sistema; la ventana muestra el
// estado de las redes y pide la autorización del sistema para cambiarlas. No tiene privilegios: ejecuta el CLI.
package main

import (
	"embed"
	"fmt"
	"io/fs"
	"os"
	"slices"
	"sync/atomic"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/app"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/backend"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/runtimefilter"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/secure"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/trayaction"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/traymenu"
)

// La interfaz se compila antes (`pnpm run build`) y se embebe en el binario: no hay servidor ni puerto.
//
//go:embed all:build/client
var assets embed.FS

//go:embed icons/tray.png
var trayPNG []byte

func main() {
	ui, err := fs.Sub(assets, "build/client")
	if err != nil {
		panic(err)
	}
	core := app.New()
	service := &AteneaService{core: core}
	wails := application.New(application.Options{
		Name:        "Atenea One",
		Description: "Estado y gestión de las redes de Atenea One",
		Services:    []application.Service{application.NewService(service)},
		Assets: application.AssetOptions{
			Handler:    application.AssetFileServerFS(ui),
			Middleware: secure.Middleware(runtimefilter.New()),
		},
		// Aplicación de bandeja: sin icono en el Dock.
		Mac: application.MacOptions{ActivationPolicy: application.ActivationPolicyAccessory},
	})

	smoke := os.Getenv("ATENEA_SMOKE") == "1"
	window := wails.Window.NewWithOptions(application.WebviewWindowOptions{
		Name:      "main",
		Title:     "Atenea One",
		Width:     920,
		Height:    600,
		MinWidth:  760,
		MinHeight: 460,
		URL:       "/",
		Hidden:    !smoke && !slices.Contains(os.Args[1:], "--open"),
	})
	// Cerrar la ventana la oculta; la aplicación sigue en la bandeja hasta «Salir».
	var quitting atomic.Bool
	window.RegisterHook(events.Common.WindowClosing, func(event *application.WindowEvent) {
		if !quitting.Load() {
			window.Hide()
			event.Cancel()
		}
	})
	show := func() {
		window.Show()
		window.Focus()
	}
	quit := func() {
		quitting.Store(true)
		wails.Quit()
	}
	handler := &traymenu.Handler{
		Apply: func(op, network string) error { _, err := backend.Apply(op, network, "", ""); return err },
		Copy:  trayaction.Copy,
		Show:  show,
		Quit:  quit,
		Log:   core.Log,
	}
	startTray(wails, core, handler, trayPNG)

	if smoke {
		// Prueba de humo: la interfaz embebida carga bajo la CSP y el filtro y llama a un método vinculado.
		service.onCall = func(method string) {
			fmt.Println("SMOKE-CALL", method)
			if method == "NetworkList" {
				fmt.Println("SMOKE-OK")
				time.AfterFunc(500*time.Millisecond, quit)
			}
		}
		time.AfterFunc(60*time.Second, func() {
			fmt.Println("SMOKE-TIMEOUT")
			quit()
		})
	}
	if err := wails.Run(); err != nil {
		fmt.Fprintln(os.Stderr, "error al ejecutar Atenea One:", err)
		os.Exit(1)
	}
}
