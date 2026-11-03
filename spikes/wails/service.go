// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"sync"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// SpikeService es el servicio vinculado del spike. Sus métodos exportados son los únicos que la página puede
// invocar: el spike S2 comprueba qué ve el JavaScript y que una prueba de superficie detecta cualquier añadido.
type SpikeService struct {
	app        *application.App
	reportPath string
	tray       *trayState
	filter     *runtimeFilter
	hold       time.Duration
	once       sync.Once
}

// Ping es el método «previsto».
func (s *SpikeService) Ping() string { return "pong" }

// NotIntended es exportado pero NO estaría en la lista permitida de la aplicación real: el spike comprueba
// que la página puede invocarlo igualmente (todo método exportado de un servicio registrado queda vinculado).
func (s *SpikeService) NotIntended() string { return "exported-but-not-intended" }

// private no es exportado: no debe poder invocarse.
func (s *SpikeService) private() string { return "private" }

var _ = (*SpikeService).private

// Report recibe el resultado de la página, lo escribe junto al de Go y cierra la aplicación.
func (s *SpikeService) Report(payload string) error {
	s.finish(payload, "page-binding")
	return nil
}

// Hidden es un tipo con un método exportado que NO se registra como servicio.
type Hidden struct{}

// Do no debe poder invocarse desde la página.
func (h *Hidden) Do() string { return "hidden" }

var _ = (*Hidden).Do

// finish escribe el informe una sola vez y cierra la aplicación. No forma parte de la superficie (no exportado).
func (s *SpikeService) finish(payload, reason string) {
	s.once.Do(func() {
		report := map[string]any{
			"reason": reason,
			"goos":   runtime.GOOS,
			"goarch": runtime.GOARCH,
			"go":     runtime.Version(),
		}
		if payload != "" {
			var page json.RawMessage
			if json.Unmarshal([]byte(payload), &page) == nil {
				report["page"] = page
			} else {
				report["page_invalid"] = payload
			}
		}
		if s.tray != nil {
			report["tray"] = s.tray.snapshot()
		}
		report["filter"] = map[string]any{"enabled": s.filter != nil}
		if s.filter != nil {
			report["filter"] = map[string]any{"enabled": true, "denied": s.filter.snapshot()}
		}
		data, _ := json.MarshalIndent(report, "", "  ")
		if s.reportPath != "" {
			if err := os.WriteFile(s.reportPath, data, 0o600); err != nil {
				fmt.Fprintln(os.Stderr, "no se pudo escribir el informe:", err)
			}
		} else {
			fmt.Println(string(data))
		}
		go func() {
			// SPIKE_HOLD_SECONDS mantiene la aplicación viva tras informar para poder medir sus sockets.
			time.Sleep(300*time.Millisecond + s.hold)
			s.app.Quit()
		}()
	})
}
