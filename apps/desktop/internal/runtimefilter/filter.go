// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package runtimefilter implementa la decisión 10 de ADR-019: el runtime de Wails exporta módulos propios
// (Clipboard, Window, System, Browser…) que la página puede usar sin que el servicio los exponga y que Wails no
// permite desactivar. Este filtro, puesto en el middleware del servidor de recursos (que ve la petición antes que
// el transporte), solo deja pasar los objetos Call y CancelCall.
package runtimefilter

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"strconv"
	"sync"
)

// allowed son los objetos del runtime de Wails que la aplicación necesita: Call (0), para los métodos del
// servicio, y CancelCall (10). Los identificadores son los de messageprocessor.go de Wails v3.0.0.
var allowed = map[int]bool{0: true, 10: true}

// Filter rechaza con 403 las llamadas al runtime de objetos no permitidos y cuenta los rechazos.
type Filter struct {
	mu     sync.Mutex
	denied map[int]int
}

// New devuelve un filtro sin rechazos.
func New() *Filter { return &Filter{denied: map[int]int{}} }

// Snapshot devuelve cuántas veces se rechazó cada objeto (clave: el identificador; -1 por trozos, -2 sin objeto).
func (f *Filter) Snapshot() map[string]int {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make(map[string]int, len(f.denied))
	for k, v := range f.denied {
		out[strconv.Itoa(k)] = v
	}
	return out
}

func (f *Filter) deny(w http.ResponseWriter, object int) {
	f.mu.Lock()
	f.denied[object]++
	f.mu.Unlock()
	http.Error(w, "runtime object not allowed", http.StatusForbidden)
}

func queryObject(r *http.Request) *int {
	if s := r.URL.Query().Get("object"); s != "" {
		if n, err := strconv.Atoi(s); err == nil {
			return &n
		}
	}
	return nil
}

// Wrap deja pasar todo salvo las peticiones al runtime de objetos no permitidos. Las subidas por trozos
// (cabecera x-wails-chunk-id) no se admiten: sus trozos no se pueden clasificar y la aplicación no las necesita.
func (f *Filter) Wrap(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/wails/runtime" {
			next.ServeHTTP(w, r)
			return
		}
		if r.Header.Get("x-wails-chunk-id") != "" {
			f.deny(w, -1)
			return
		}
		object := queryObject(r)
		if r.Body != nil && r.Method != http.MethodGet {
			body, _ := io.ReadAll(io.LimitReader(r.Body, 1<<20))
			r.Body = io.NopCloser(bytes.NewReader(body))
			var req struct {
				Object *int `json:"object"`
			}
			if json.Unmarshal(body, &req) == nil && req.Object != nil {
				object = req.Object
			}
		}
		if object == nil {
			f.deny(w, -2)
			return
		}
		if !allowed[*object] {
			f.deny(w, *object)
			return
		}
		next.ServeHTTP(w, r)
	})
}
