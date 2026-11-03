// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Spike de ADR-019: comprueba con un webview real la CSP (S1), la superficie del puente (S2) y la bandeja (S3)
// de Wails v3. No es la aplicación: no lee ningún agente ni ejecuta el CLI.
package main

import (
	"embed"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"os"
	"strconv"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
)

//go:embed all:frontend
var frontend embed.FS

// csp es la de ADR-011 §2 adaptada a Wails: el puente de Wails habla con el origen de la propia aplicación
// (`connect-src 'self'`), no con `ipc:` como Tauri. El spike S1 comprueba que con ella la página funciona.
const csp = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'"

func main() {
	assets, err := fs.Sub(frontend, "frontend")
	if err != nil {
		panic(err)
	}
	svc := &SpikeService{reportPath: os.Getenv("SPIKE_REPORT")}
	if os.Getenv("SPIKE_FILTER") == "1" {
		svc.filter = newRuntimeFilter()
	}
	if n, err := strconv.Atoi(os.Getenv("SPIKE_HOLD_SECONDS")); err == nil && n > 0 {
		svc.hold = time.Duration(n) * time.Second
	}
	app := application.New(application.Options{
		Name:     "AteneaSpike",
		Services: []application.Service{application.NewService(svc)},
		Assets: application.AssetOptions{
			Handler: application.AssetFileServerFS(assets),
			Middleware: func(next http.Handler) http.Handler {
				if svc.filter != nil {
					next = svc.filter.wrap(next)
				}
				return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
					w.Header().Set("Content-Security-Policy", csp)
					// Camino de respaldo del informe si el puente fallase: no forma parte de la aplicación real.
					if r.Method == http.MethodPost && r.URL.Path == "/spike-report" {
						body, _ := io.ReadAll(io.LimitReader(r.Body, 1<<20))
						svc.finish(string(body), "page-http")
						w.WriteHeader(http.StatusNoContent)
						return
					}
					next.ServeHTTP(w, r)
				})
			},
		},
	})
	svc.app = app
	svc.tray = setupTray(app)
	app.Window.NewWithOptions(application.WebviewWindowOptions{Title: "Atenea spike", Width: 800, Height: 600, URL: "/"})
	time.AfterFunc(2*time.Second, svc.tray.update)
	time.AfterFunc(90*time.Second, func() { svc.finish("", "timeout") })
	if err := app.Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
