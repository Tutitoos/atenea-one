// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func run(t *testing.T, f *runtimeFilter, req *http.Request) (code int, reached bool, body string) {
	t.Helper()
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		reached = true
		b, _ := io.ReadAll(r.Body)
		body = string(b)
	})
	rec := httptest.NewRecorder()
	f.wrap(next).ServeHTTP(rec, req)
	return rec.Code, reached, body
}

func post(path, body string) *http.Request {
	return httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
}

func TestFilterAllowsOnlyCallAndCancelCall(t *testing.T) {
	for object, want := range map[string]bool{"0": true, "10": true, "1": false, "2": false, "3": false, "5": false,
		"6": false, "7": false, "8": false, "9": false, "11": false, "99": false} {
		f := newRuntimeFilter()
		code, reached, _ := run(t, f, post("/wails/runtime", `{"object":`+object+`,"method":0,"args":{}}`))
		if reached != want || (want && code != 200) || (!want && code != http.StatusForbidden) {
			t.Errorf("objeto %s: llegó=%v código=%d, esperaba permitido=%v", object, reached, code, want)
		}
	}
}

func TestFilterPassesTheBodyUnchangedToTheNextHandler(t *testing.T) {
	const body = `{"object":0,"method":42,"args":{"x":"y"}}`
	_, reached, got := run(t, newRuntimeFilter(), post("/wails/runtime", body))
	if !reached || got != body {
		t.Fatalf("el cuerpo debe llegar intacto: llegó=%v cuerpo=%q", reached, got)
	}
}

func TestFilterReadsTheObjectFromTheQueryToo(t *testing.T) {
	f := newRuntimeFilter()
	if code, reached, _ := run(t, f, httptest.NewRequest(http.MethodGet, "/wails/runtime?object=0&method=1", nil)); !reached || code != 200 {
		t.Errorf("GET con object=0 debe pasar: %d %v", code, reached)
	}
	if code, reached, _ := run(t, f, httptest.NewRequest(http.MethodGet, "/wails/runtime?object=1&method=1", nil)); reached || code != http.StatusForbidden {
		t.Errorf("GET con object=1 debe rechazarse: %d %v", code, reached)
	}
}

func TestFilterRejectsChunkedMissingAndMalformedRequests(t *testing.T) {
	chunked := post("/wails/runtime", `{"object":0,"method":0}`)
	chunked.Header.Set("x-wails-chunk-id", "abc")
	for name, req := range map[string]*http.Request{
		"por trozos":      chunked,
		"sin object":      post("/wails/runtime", `{"method":0}`),
		"no es JSON":      post("/wails/runtime", `not json`),
		"cuerpo vacío":    post("/wails/runtime", ``),
		"object negativo": post("/wails/runtime", `{"object":-1,"method":0}`),
		"object de texto": post("/wails/runtime", `{"object":"0","method":0}`),
	} {
		if code, reached, _ := run(t, newRuntimeFilter(), req); reached || code != http.StatusForbidden {
			t.Errorf("%s: llegó=%v código=%d", name, reached, code)
		}
	}
}

func TestFilterLeavesOtherPathsAloneAndCountsDenials(t *testing.T) {
	f := newRuntimeFilter()
	for _, p := range []string{"/", "/spike.js", "/wails/runtime.js", "/spike-report"} {
		if code, reached, _ := run(t, f, httptest.NewRequest(http.MethodGet, p, nil)); !reached || code != 200 {
			t.Errorf("%s debe pasar: %d %v", p, code, reached)
		}
	}
	run(t, f, post("/wails/runtime", `{"object":1,"method":0}`))
	run(t, f, post("/wails/runtime", `{"object":1,"method":1}`))
	run(t, f, post("/wails/runtime", `{"object":9,"method":0}`))
	if got := f.snapshot(); got["1"] != 2 || got["9"] != 1 || len(got) != 2 {
		t.Errorf("contadores = %v", got)
	}
}
