// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package traymenu

import (
	"errors"
	"slices"
	"strings"
	"sync"
	"testing"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/health"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/trayaction"
)

const net1 = "0199ab12-3456-7def-8abc-0123456789ab"

// recorder sustituye todo lo que el manejador toca del sistema y apunta lo que se le pidió.
type recorder struct {
	mu    sync.Mutex
	calls []string
	logs  []string
	fail  error
}

func (r *recorder) add(call string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.calls = append(r.calls, call)
}

func (r *recorder) handler() *Handler {
	return &Handler{
		Apply: func(op, network string) error { r.add("apply " + op + " " + network); return r.fail },
		Copy:  func(address string) error { r.add("copy " + address); return r.fail },
		Show:  func() { r.add("show") },
		Quit:  func() { r.add("quit") },
		Log: func(message string) {
			r.mu.Lock()
			defer r.mu.Unlock()
			r.logs = append(r.logs, message)
		},
	}
}

func TestItemsOrderStatusLinesThenActionsThenOpenAndQuit(t *testing.T) {
	lines := []health.Line{{ID: net1, Text: "casa: Conectado"}}
	actions := []trayaction.Entry{{ID: "down:" + net1, Text: "Desconectar casa"}, {ID: "copy:10.50.0.1", Text: "Copiar mi IP · casa"}}
	want := []Item{
		{ID: "state:" + net1, Text: "casa: Conectado", Enabled: false},
		{ID: "down:" + net1, Text: "Desconectar casa", Enabled: true},
		{ID: "copy:10.50.0.1", Text: "Copiar mi IP · casa", Enabled: true},
		{ID: IDOpen, Text: "Abrir Atenea One", Enabled: true},
		{ID: IDQuit, Text: "Salir", Enabled: true},
	}
	if got := Items(lines, actions); !slices.Equal(got, want) {
		t.Errorf("elementos = %+v\nesperados  = %+v", got, want)
	}
	if got := Items(nil, nil); len(got) != 2 || got[0].ID != IDOpen || got[1].ID != IDQuit {
		t.Errorf("sin redes quedan Abrir y Salir: %+v", got)
	}
}

func TestEveryClickMapsToExactlyOneSystemCall(t *testing.T) {
	for _, tc := range []struct{ id, want string }{
		{IDOpen, "show"},
		{IDQuit, "quit"},
		{"up:" + net1, "apply up " + net1},
		{"down:" + net1, "apply down " + net1},
		{"copy:10.50.0.1", "copy 10.50.0.1"},
	} {
		var r recorder
		r.handler().Do(tc.id)
		if !slices.Equal(r.calls, []string{tc.want}) {
			t.Errorf("clic en %q: llamadas %q, esperaba %q", tc.id, r.calls, tc.want)
		}
	}
}

func TestUnknownOrMalformedIdentifiersDoNothing(t *testing.T) {
	// Las líneas de estado no son clicables, pero un id suyo que llegase igualmente no debe hacer nada.
	for _, id := range []string{"", "state:" + net1, "leave:" + net1, "join:" + net1, "migrate:x", "up", "UP:" + net1, " up:" + net1, "*"} {
		var r recorder
		r.handler().Do(id)
		if len(r.calls) != 0 || len(r.logs) != 0 {
			t.Errorf("%q no debía hacer nada: llamadas %q, avisos %q", id, r.calls, r.logs)
		}
	}
}

func TestOnlyUpAndDownCanReachTheElevatedOperationFromTheTray(t *testing.T) {
	// Salir de una red borra claves y unirse acepta un código: ninguna de las dos se alcanza desde un menú.
	var r recorder
	h := r.handler()
	for _, op := range []string{"leave", "join", "migrate"} {
		h.Do(op + ":" + net1)
	}
	if len(r.calls) != 0 {
		t.Errorf("la bandeja no debe poder lanzar %q", r.calls)
	}
}

func TestFailedActionsAreLoggedWithTheirOperationAndNeverPanic(t *testing.T) {
	r := recorder{fail: errors.New("Cancelado")}
	h := r.handler()
	h.Do("up:" + net1)
	h.Do("copy:10.50.0.1")
	if len(r.logs) != 2 || !strings.Contains(r.logs[0], "up") || !strings.Contains(r.logs[0], "Cancelado") ||
		!strings.Contains(r.logs[1], "copy") {
		t.Errorf("registros = %q", r.logs)
	}
}

func TestSecondClickWhileTheSameActionIsRunningIsIgnoredButOthersProceed(t *testing.T) {
	release := make(chan struct{})
	started := make(chan struct{})
	var r recorder
	h := r.handler()
	h.Apply = func(op, network string) error {
		r.add("apply " + op + " " + network)
		if op == "up" {
			close(started)
			<-release // el diálogo de autorización sigue abierto
		}
		return nil
	}
	done := make(chan struct{})
	go func() { h.Do("up:" + net1); close(done) }()
	<-started

	h.Do("up:" + net1)   // mismo clic con el diálogo abierto: se ignora
	h.Do("down:" + net1) // otra acción sí se ejecuta
	close(release)
	<-done
	if want := []string{"apply up " + net1, "apply down " + net1}; !slices.Equal(r.calls, want) {
		t.Errorf("llamadas = %q, esperadas %q", r.calls, want)
	}
	// Terminada la acción, un nuevo clic vuelve a funcionar.
	h.Apply = func(op, network string) error { r.add("again " + op); return nil }
	h.Do("up:" + net1)
	if got := r.calls[len(r.calls)-1]; got != "again up" {
		t.Errorf("tras terminar, el clic debe volver a funcionar: %q", r.calls)
	}
}
