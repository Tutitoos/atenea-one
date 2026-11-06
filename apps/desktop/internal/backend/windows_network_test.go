// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"encoding/json"
	"errors"
	"testing"
	"time"
)

func TestClientForSeparatesSeeingFromJoining(t *testing.T) {
	old := windowsAgentResponds
	defer func() { windowsAgentResponds = old }()
	for _, tc := range []struct {
		goos      string
		responds  bool
		canJoin   bool
		supported bool
		installed bool
	}{
		{"windows", true, false, true, true},
		{"windows", false, false, true, false},
		{"freebsd", true, false, false, false},
	} {
		windowsAgentResponds = func() bool { return tc.responds }
		got := clientFor(tc.goos, "/no/existe")
		if got.CanJoin != tc.canJoin || got.Supported != tc.supported || got.Installed != tc.installed {
			t.Errorf("%s responde=%v: %+v", tc.goos, tc.responds, got)
		}
	}
	for _, goos := range []string{"darwin", "linux"} {
		if got := clientFor(goos, "/no/existe"); !got.CanJoin || !got.Supported {
			t.Errorf("%s: %+v", goos, got)
		}
	}
}

func TestWindowsListIsOneSyntheticNetworkOrEmpty(t *testing.T) {
	ask := func(text string, err error) func(string, int64, time.Duration) (string, error) {
		return func(socket string, limit int64, _ time.Duration) (string, error) {
			if socket != WindowsStatusPipe || limit != MaxStatusBytes {
				t.Errorf("pregunta a %q con límite %d", socket, limit)
			}
			return text, err
		}
	}
	if got := windowsList(ask("", errors.New("sin tubería"))); got != "[]" {
		t.Errorf("sin agente: %s", got)
	}
	if got := windowsList(ask("no es json", nil)); got != "[]" {
		t.Errorf("respuesta inválida: %s", got)
	}

	var nets []map[string]any
	live := `{"public_key":"k","config":{"lease_active":true,"last_error":null},"sync":{"url":"https://panel.example.org"}}`
	if err := json.Unmarshal([]byte(windowsList(ask(live, nil))), &nets); err != nil || len(nets) != 1 {
		t.Fatalf("conectada: %v %v", nets, err)
	}
	n := nets[0]
	if n["network_id"] != WindowsNetworkID || !ValidUUID(WindowsNetworkID) || n["name"] != "Red de este equipo" ||
		n["state"] != "conectada" || n["join_status"] != "aprobada" || n["status_socket"] != WindowsStatusPipe ||
		n["server"] != "panel.example.org" || n["interface"] != "atenea0" {
		t.Errorf("red sintética: %v", n)
	}
	if _, has := n["detail"]; has {
		t.Errorf("conectada sin detalle: %v", n)
	}

	down := `{"config":{"lease_active":false,"last_error":"sin respuesta"}}`
	if err := json.Unmarshal([]byte(windowsList(ask(down, nil))), &nets); err != nil || len(nets) != 1 {
		t.Fatalf("sin lease: %v %v", nets, err)
	}
	if nets[0]["state"] != "iniciada_sin_configuracion" || nets[0]["detail"] != "sin respuesta" {
		t.Errorf("sin lease: %v", nets[0])
	}
}
