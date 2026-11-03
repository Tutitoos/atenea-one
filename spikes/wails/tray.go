// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"bytes"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"sync"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// icon genera un PNG de 22x22 de un solo color: el spike no lleva binarios.
func icon(c color.RGBA) []byte {
	img := image.NewRGBA(image.Rect(0, 0, 22, 22))
	for y := 3; y < 19; y++ {
		for x := 3; x < 19; x++ {
			img.Set(x, y, c)
		}
	}
	var b bytes.Buffer
	_ = png.Encode(&b, img)
	return b.Bytes()
}

// trayState recoge lo que el spike S3 observa de la bandeja. Que la creación y la actualización no fallen no
// demuestra que el icono sea visible: eso solo se comprueba a mano en un escritorio real.
type trayState struct {
	mu   sync.Mutex
	app  *application.App
	tray *application.SystemTray
	res  map[string]any
}

func (t *trayState) set(k string, v any) {
	t.mu.Lock()
	t.res[k] = v
	t.mu.Unlock()
}

func (t *trayState) snapshot() map[string]any {
	t.mu.Lock()
	defer t.mu.Unlock()
	out := make(map[string]any, len(t.res))
	for k, v := range t.res {
		out[k] = v
	}
	return out
}

func menuOK(app *application.App) *application.Menu {
	m := app.NewMenu()
	m.Add("Estado: conectada").SetEnabled(false)
	m.AddSeparator()
	m.AddCheckbox("Abrir al iniciar sesión", false).OnClick(func(*application.Context) {})
	m.Add("Salir").OnClick(func(*application.Context) { app.Quit() })
	return m
}

// setupTray crea la bandeja con icono, etiqueta y menú. Un fallo se registra, no aborta el spike.
func setupTray(app *application.App) *trayState {
	t := &trayState{app: app, res: map[string]any{}}
	defer func() {
		if r := recover(); r != nil {
			t.set("panic_create", fmt.Sprint(r))
		}
	}()
	t.tray = app.SystemTray.New()
	t.tray.SetIcon(icon(color.RGBA{R: 0, G: 160, B: 0, A: 255}))
	t.tray.SetLabel("Atenea")
	t.tray.SetTooltip("Atenea spike")
	t.tray.SetMenu(menuOK(app))
	t.set("created", true)
	return t
}

// update reconstruye el menú y cambia el icono con la aplicación ya en marcha (como hace la bandeja real
// cuando cambia la salud de una red).
func (t *trayState) update() {
	defer func() {
		if r := recover(); r != nil {
			t.set("panic_update", fmt.Sprint(r))
		}
	}()
	if t.tray == nil {
		t.set("updated", "sin bandeja")
		return
	}
	m := t.app.NewMenu()
	m.Add("Estado: degradado").SetEnabled(false)
	m.Add("Copiar mi IP")
	t.tray.SetIcon(icon(color.RGBA{R: 200, G: 40, B: 40, A: 255}))
	t.tray.SetLabel("Atenea (degradado)")
	t.tray.SetMenu(m)
	t.set("updated", true)
}
