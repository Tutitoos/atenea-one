// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package secure es el middleware del servidor de recursos de la aplicación (ADR-019, decisiones 3 y 10): pone la
// CSP en toda respuesta y aplica el filtro de objetos del runtime de Wails antes de que la petición llegue a su
// transporte. Vive fuera de `main` para poder probarlo sin Wails.
package secure

import (
	"net/http"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/runtimefilter"
)

// CSP es la de ADR-011 §2 adaptada a Wails: el puente habla con el origen de la propia aplicación
// (`connect-src 'self'`) en lugar de `ipc:` de Tauri. Sin `unsafe-inline`, sin `unsafe-eval` y sin orígenes remotos.
const CSP = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'"

// Middleware envuelve el manejador de recursos de Wails: CSP en toda respuesta (también en los rechazos) y filtro
// de objetos del runtime.
func Middleware(filter *runtimefilter.Filter) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		guarded := filter.Wrap(next)
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Security-Policy", CSP)
			guarded.ServeHTTP(w, r)
		})
	}
}
