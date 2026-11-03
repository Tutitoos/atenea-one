// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package app

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/health"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/panel"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/ping"
)

const (
	id1 = "0199ab12-3456-7def-8abc-0123456789ab"
	id2 = "0199ab12-3456-7def-8abc-0123456789ac"
)

func statusJSON(path string) string {
	return `{"public_key":"k-` + path + `","config":{"lease_active":true,"last_error":null},"sync":{"last_error":null},` +
		`"coordinator":{"last_error":null},"peers":[{"public_key":"p","addrs":["10.0.0.2"],"path":"` + path + `","rx_bytes":9007199254740993}]}`
}

func skipWindows(t *testing.T) {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("las redes múltiples no están soportadas en Windows")
	}
}

// fakeCLI instala un «CLI» de shell que imprime el contenido de list.json (que la prueba puede cambiar).
func fakeCLI(t *testing.T) (listFile string) {
	t.Helper()
	skipWindows(t)
	dir := t.TempDir()
	listFile = filepath.Join(dir, "list.json")
	cli := filepath.Join(dir, "atenea")
	if err := os.WriteFile(cli, []byte("#!/bin/sh\ncat '"+listFile+"'\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("ATENEA_CLI", cli)
	t.Setenv("ATENEA_RUN_DIR", "/run/atenea-prueba")
	return listFile
}

func writeList(t *testing.T, path, content string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestStatusComposesHealthKeepsNumbersExactAndNeverEmitsNullReasons(t *testing.T) {
	skipWindows(t)
	a := &App{Ask: func(string) (string, error) { return statusJSON("direct"), nil }}
	got, err := a.Status(id1)
	if err != nil {
		t.Fatal(err)
	}
	// 2^53+1 no cabe en un float64: si se reemitiera por float64 cambiaría.
	if !strings.Contains(got, "9007199254740993") {
		t.Errorf("el estado debe reenviarse sin alterar los números: %s", got)
	}
	if !strings.Contains(got, `"reasons":[]`) {
		t.Errorf("sin motivos, `reasons` debe ser [] y no null: %s", got)
	}
	var parsed struct {
		Health struct{ Level, Text string }
	}
	if err := json.Unmarshal([]byte(got), &parsed); err != nil || parsed.Health.Level != "ok" || parsed.Health.Text != "Conectado" {
		t.Errorf("salud = %+v, %v", parsed.Health, err)
	}
	relay := &App{Ask: func(string) (string, error) { return statusJSON("relay-udp"), nil }}
	if got, _ := relay.Status(id1); !strings.Contains(got, `"level":"degraded"`) {
		t.Errorf("un par por relay degrada la salud: %s", got)
	}
}

func TestStatusFailuresAreReturnedUnchangedAndNeverAssessed(t *testing.T) {
	skipWindows(t)
	a := &App{Ask: func(string) (string, error) { return "", errors.New("agente: orden rechazada") }}
	if _, err := a.Status(id1); err == nil || err.Error() != "agente: orden rechazada" {
		t.Errorf("error del agente: %v", err)
	}
	asked := false
	a = &App{Ask: func(string) (string, error) { asked = true; return "{}", nil }}
	if _, err := a.Status("no-es-un-uuid"); err == nil || err.Error() != "identificador de red no válido" || asked {
		t.Errorf("un id no válido no debe llegar al agente: %v (preguntó=%v)", err, asked)
	}
}

func TestDevicePingChecksTheNetworkAndTheAddressBeforeLaunchingAnything(t *testing.T) {
	list := fakeCLI(t)
	writeList(t, list, `[{"network_id":"`+id1+`","name":"casa"}]`)
	asked, launched := false, false
	oldExec := ping.Exec
	ping.Exec = func(string, []string, time.Duration) (string, error) { launched = true; return "", nil }
	t.Cleanup(func() { ping.Exec = oldExec })
	a := &App{Ask: func(string) (string, error) { asked = true; return statusJSON("direct"), nil }}

	if _, err := a.DevicePing(id2, "10.0.0.2"); err == nil || err.Error() != "la red no está unida" || asked || launched {
		t.Errorf("red no unida: err=%v preguntó=%v lanzó=%v", err, asked, launched)
	}
	for _, foreign := range []string{"10.99.99.99", "ejemplo.test", "10.0.0.2; id", ""} {
		if _, err := a.DevicePing(id1, foreign); err == nil || launched {
			t.Errorf("%q: err=%v lanzó=%v", foreign, err, launched)
		}
	}
}

func TestOpenPanelBuildsTheURLFromTheJoinedNetworkOnly(t *testing.T) {
	list := fakeCLI(t)
	var opened []string
	old := panel.Launch
	panel.Launch = func(program, arg string) error { opened = append(opened, arg); return nil }
	t.Cleanup(func() { panel.Launch = old })
	a := &App{}

	writeList(t, list, `[{"network_id":"`+id1+`","name":"casa","url":"https://Control.Example.test:443/ruta?x=1#f"}]`)
	if err := a.OpenPanel(id1, id2); err != nil {
		t.Fatal(err)
	}
	if want := "https://control.example.test/?net=" + id1 + "&view=Dispositivos"; len(opened) != 1 || opened[0] != want {
		t.Errorf("URL abierta = %v, esperaba %q", opened, want)
	}
	opened = nil
	writeList(t, list, `[{"network_id":"`+id1+`","name":"casa"}]`)
	if err := a.OpenPanel(id1, ""); err == nil || err.Error() != "falta la URL del servidor" || len(opened) != 0 {
		t.Errorf("sin URL: err=%v abierto=%v", err, opened)
	}
	if err := a.OpenPanel(id2, ""); err == nil || err.Error() != "la red no está unida" || len(opened) != 0 {
		t.Errorf("red no unida: err=%v abierto=%v", err, opened)
	}
}

func TestNotificationPreferenceLivesInTheTauriConfigFolder(t *testing.T) {
	dir := t.TempDir()
	a := &App{ConfigDir: func() (string, error) { return dir, nil }}
	if on, err := a.NotificationsGet(); err != nil || !on {
		t.Fatalf("por defecto activada: %v %v", on, err)
	}
	if err := a.NotificationsSet(false); err != nil {
		t.Fatal(err)
	}
	if on, err := a.NotificationsGet(); err != nil || on {
		t.Errorf("tras desactivar: %v %v", on, err)
	}
	if _, err := os.Stat(filepath.Join(dir, "notifications.json")); err != nil {
		t.Errorf("el fichero debe llamarse notifications.json en la carpeta de configuración: %v", err)
	}
}

func TestAutostartRoundTripWritesOnlyUnderTheUsersHome(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Windows no admite el inicio con la sesión")
	}
	home := t.TempDir()
	a := &App{
		HomeDir: func() (string, error) { return home, nil },
		Exe:     func() (string, error) { return "/usr/local/bin/atenea-desktop", nil },
		Getenv:  func(string) string { return "" },
	}
	if on, err := a.AutostartGet(); err != nil || on == nil || *on {
		t.Fatalf("al principio desactivado: %v %v", on, err)
	}
	if err := a.AutostartSet(true); err != nil {
		t.Fatal(err)
	}
	if on, err := a.AutostartGet(); err != nil || on == nil || !*on {
		t.Errorf("tras activar: %v %v", on, err)
	}
	if err := a.AutostartSet(false); err != nil {
		t.Fatal(err)
	}
	if on, err := a.AutostartGet(); err != nil || on == nil || *on {
		t.Errorf("tras desactivar: %v %v", on, err)
	}
}

func TestPollRefreshesTheNetworkListEveryFiveReadsAndCountsOnlyAnsweringNetworks(t *testing.T) {
	list := fakeCLI(t)
	answers := map[string]string{ // por id corto del socket
		"456789ab": statusJSON("direct"),
		"456789ac": statusJSON("relay-udp"),
	}
	a := &App{Ask: func(socket string) (string, error) {
		if s, ok := answers[filepath.Base(filepath.Dir(socket))]; ok {
			return s, nil
		}
		return "", errors.New("sin acceso al estado")
	}}
	state := &TrayState{}

	writeList(t, list, `[{"network_id":"`+id1+`","name":"casa"}]`)
	h, text, lines := a.Poll(state)
	if h != health.Ok || text != "Conectado" || len(lines) != 1 {
		t.Fatalf("una red: %v %q %v", h, text, lines)
	}

	// Aparece una segunda red, pero el CLI no se vuelve a consultar hasta la lectura número seis.
	writeList(t, list, `[{"network_id":"`+id1+`","name":"casa"},{"network_id":"`+id2+`","name":"oficina"}]`)
	for read := 2; read <= 5; read++ {
		if _, text, _ := a.Poll(state); text != "Conectado" {
			t.Fatalf("lectura %d: la lista no debe refrescarse todavía: %q", read, text)
		}
	}
	h, text, lines = a.Poll(state)
	if h != health.Degraded || !strings.HasPrefix(text, "2 redes · ") || len(lines) != 2 {
		t.Fatalf("lectura 6, dos redes: %v %q %v", h, text, lines)
	}

	// Una red que no responde (parada a propósito o sin acceso) no cuenta ni pone en rojo a las demás.
	delete(answers, "456789ac")
	for read := 7; read <= 10; read++ {
		a.Poll(state)
	}
	if h, text, _ := a.Poll(state); h != health.Ok || text != "Conectado" {
		t.Errorf("la red que no responde no debe contar: %v %q", h, text)
	}
}
