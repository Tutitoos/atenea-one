// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package secure

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/runtimefilter"
)

func serve(t *testing.T, req *http.Request) (*httptest.ResponseRecorder, bool) {
	t.Helper()
	reached := false
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { reached = true })
	rec := httptest.NewRecorder()
	Middleware(runtimefilter.New())(next).ServeHTTP(rec, req)
	return rec, reached
}

func TestEveryResponseCarriesTheStrictCSP(t *testing.T) {
	for name, req := range map[string]*http.Request{
		"página":                           httptest.NewRequest(http.MethodGet, "/", nil),
		"llamada permitida":                httptest.NewRequest(http.MethodPost, "/wails/runtime", strings.NewReader(`{"object":0,"method":0}`)),
		"llamada rechazada (portapapeles)": httptest.NewRequest(http.MethodPost, "/wails/runtime", strings.NewReader(`{"object":1,"method":0}`)),
	} {
		rec, _ := serve(t, req)
		if got := rec.Header().Get("Content-Security-Policy"); got != CSP {
			t.Errorf("%s: CSP = %q", name, got)
		}
	}
	for _, forbidden := range []string{"unsafe-inline", "unsafe-eval", "http:", "https:", "*"} {
		if strings.Contains(CSP, forbidden) {
			t.Errorf("la CSP no puede contener %q", forbidden)
		}
	}
}

func TestOnlyCallReachesTheWailsTransport(t *testing.T) {
	for object, want := range map[string]bool{"0": true, "10": true, "1": false, "6": false, "8": false, "9": false} {
		rec, reached := serve(t, httptest.NewRequest(http.MethodPost, "/wails/runtime", strings.NewReader(`{"object":`+object+`,"method":0}`)))
		if reached != want {
			t.Errorf("objeto %s: llegó=%v, esperaba %v (código %d)", object, reached, want, rec.Code)
		}
	}
}

func TestPagesAndAssetsAlwaysPass(t *testing.T) {
	for _, p := range []string{"/", "/index.html", "/assets/app.js", "/wails/runtime.js"} {
		if _, reached := serve(t, httptest.NewRequest(http.MethodGet, p, nil)); !reached {
			t.Errorf("%s debe llegar al servidor de recursos", p)
		}
	}
}
