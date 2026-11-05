// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package traymenu

import (
	"slices"
	"testing"
)

// recorder sustituye todo lo que el manejador toca del sistema y apunta lo que se le pidió.
type recorder struct{ calls []string }

func (r *recorder) handler() *Handler {
	return &Handler{
		Show: func() { r.calls = append(r.calls, "show") },
		Quit: func() { r.calls = append(r.calls, "quit") },
	}
}

func TestMenuIsExactlyOpenAndQuit(t *testing.T) {
	want := []Item{
		{ID: IDOpen, Text: "Abrir Atenea One", Enabled: true},
		{ID: IDQuit, Text: "Salir", Enabled: true},
	}
	if got := Items(); !slices.Equal(got, want) {
		t.Errorf("menú = %+v\nesperado = %+v", got, want)
	}
}

func TestEveryClickMapsToExactlyOneSystemCall(t *testing.T) {
	for id, want := range map[string]string{IDOpen: "show", IDQuit: "quit"} {
		r := &recorder{}
		r.handler().Do(id)
		if !slices.Equal(r.calls, []string{want}) {
			t.Errorf("%s: llamadas = %q, esperada %q", id, r.calls, want)
		}
	}
}

func TestUnknownIdentifiersDoNothing(t *testing.T) {
	// Los identificadores de las acciones por red que tuvo el menú (conectar, desconectar, copiar) ya no existen.
	for _, id := range []string{"", "bogus", "state:x", "up:0199ab12-3456-7def-8abc-0123456789ab", "down:x", "copy:10.0.0.1"} {
		r := &recorder{}
		r.handler().Do(id)
		if len(r.calls) != 0 {
			t.Errorf("%q hizo %q", id, r.calls)
		}
	}
}
