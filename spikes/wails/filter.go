// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"strconv"
	"sync"
)

// allowedObjects son los objetos del runtime de Wails que la aplicación real necesitaría: Call (0), para los
// métodos del servicio, y CancelCall (10). Todo lo demás (Clipboard, Application, Events, Dialog, Window,
// Screens, System, Browser…) se rechaza en el servidor de recursos, que ve la petición antes que el transporte
// de Wails. Spike S2c de ADR-019: se activa con SPIKE_FILTER=1 para comparar con y sin filtro.
var allowedObjects = map[int]bool{0: true, 10: true}

type runtimeFilter struct {
	mu     sync.Mutex
	denied map[int]int
}

func newRuntimeFilter() *runtimeFilter { return &runtimeFilter{denied: map[int]int{}} }

func (f *runtimeFilter) snapshot() map[string]int {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make(map[string]int, len(f.denied))
	for k, v := range f.denied {
		out[strconv.Itoa(k)] = v
	}
	return out
}

func (f *runtimeFilter) deny(w http.ResponseWriter, object int) {
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

// wrap deja pasar todo salvo las peticiones al runtime de objetos no permitidos. Las subidas por trozos
// (cabecera x-wails-chunk-id) no se admiten: sus trozos no se pueden clasificar y la aplicación no las necesita.
func (f *runtimeFilter) wrap(next http.Handler) http.Handler {
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
		if !allowedObjects[*object] {
			f.deny(w, *object)
			return
		}
		next.ServeHTTP(w, r)
	})
}
