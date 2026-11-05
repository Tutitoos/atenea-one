// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package health

import (
	"encoding/json"
	"fmt"
	"reflect"
	"testing"
)

func status(lease bool, path string) map[string]any {
	var m map[string]any
	raw := fmt.Sprintf(`{"config": {"lease_active": %t, "last_error": null}, "sync": {"last_error": null},
		"coordinator": {"last_error": null}, "peers": [{"path": %q}]}`, lease, path)
	if err := json.Unmarshal([]byte(raw), &m); err != nil {
		panic(err)
	}
	return m
}

func sub(s map[string]any, k string) map[string]any { return s[k].(map[string]any) }

func TestHealthFollowsLeaseErrorsAndPaths(t *testing.T) {
	for _, c := range []struct {
		path string
		want Health
	}{{"direct", Ok}, {"relay-udp", Degraded}, {"none", Ok}, {"probing", Ok}, {"relay-tls", Degraded}} {
		if got := Assess(status(true, c.path)).Health; got != c.want {
			t.Errorf("%s: %v, quería %v", c.path, got, c.want)
		}
	}
	// Un lease perdido hace tiempo es una caída real.
	longAgo := int64(1_800_000_000 - 1000)
	a := AssessAt(status(false, "direct"), 1_800_000_000, &longAgo)
	if a.Health != Down || a.Text != "Sin conexión: sin lease activo" || a.Level() != "down" {
		t.Errorf("inesperado: %+v", a)
	}
	if !reflect.DeepEqual(a.Reasons, []string{"sin lease activo"}) {
		t.Errorf("reasons: %v", a.Reasons)
	}
	s := status(true, "direct")
	sub(s, "sync")["last_error"] = "acceso denegado"
	a = Assess(s)
	if a.Health != Degraded || a.Text != "Degradado: control plane: acceso denegado" {
		t.Errorf("inesperado: %+v", a)
	}
}

func TestCertificateWarningBoundaries(t *testing.T) {
	now := int64(1_800_000_000)
	for _, c := range []struct {
		seconds int64
		want    Health
	}{{14*86400 + 1, Ok}, {14 * 86400, Degraded}, {3 * 86400, Degraded}, {1, Degraded}, {0, Down}, {-1, Down}} {
		s := status(true, "direct")
		s["server"] = map[string]any{"issuer_cert_not_after_unix": float64(now + c.seconds)}
		a := AssessAt(s, now, nil)
		if a.Health != c.want {
			t.Errorf("%d: %v, quería %v", c.seconds, a.Health, c.want)
		}
		var want []string
		switch {
		case c.seconds <= 0:
			want = []string{"El certificado de la red ha caducado"}
		case c.seconds <= 14*86400:
			want = []string{fmt.Sprintf("El certificado de la red caduca en %d días", (c.seconds+86399)/86400)}
		}
		if len(a.Reasons) != len(want) || (len(want) > 0 && a.Reasons[0] != want[0]) {
			t.Errorf("%d: reasons %v, quería %v", c.seconds, a.Reasons, want)
		}
	}
	for _, server := range []any{nil, map[string]any{}, map[string]any{"issuer_cert_not_after_unix": nil}} {
		s := status(true, "direct")
		s["server"] = server
		if AssessAt(s, now, nil).Health != Ok {
			t.Errorf("server %v debería ser Ok", server)
		}
	}
	if AssessAt(status(true, "direct"), now, nil).Health != Ok {
		t.Error("sin server debería ser Ok")
	}
}

func TestLostLeaseIsReconnectingOnlyDuringGrace(t *testing.T) {
	now := int64(1_800_000_000)
	// Tras despertar, el agente caduca el lease y la configuración antes de renovarlos.
	s := status(false, "none")
	sub(s, "config")["last_error"] = "expired"
	sub(s, "sync")["last_error"] = "error de red"
	for _, since := range []int64{now, now - (leaseGraceSecs - 1)} {
		a := AssessAt(s, now, &since)
		if a.Health != Degraded || a.Text != "Reconectando…" {
			t.Errorf("inesperado: %+v", a)
		}
		for _, r := range a.Reasons {
			if r == "sin lease activo" || r == "configuración: expired" {
				t.Errorf("razón no esperada: %q", r)
			}
		}
	}
	since := now - leaseGraceSecs
	a := AssessAt(s, now, &since)
	if a.Health != Down || a.Text != "Sin conexión: sin lease activo" {
		t.Errorf("inesperado: %+v", a)
	}
	// Un error de configuración distinto de la caducidad sigue siendo una caída inmediata.
	s = status(false, "none")
	sub(s, "config")["last_error"] = "firma inválida"
	if AssessAt(s, now, &now).Health != Down {
		t.Error("debería ser Down")
	}
	// Con el lease activo no hay gracia que valga.
	old := now - 1000
	if AssessAt(status(true, "direct"), now, &old).Health != Ok {
		t.Error("debería ser Ok")
	}
}

func TestAssessTracksLostLeasePerKey(t *testing.T) {
	t0 := int64(1_800_000_000)
	defer func(f func() int64) { clock = f }(clock)
	s := status(false, "none")
	s["public_key"] = "k1"
	clock = func() int64 { return t0 }
	if got := Assess(s).Text; got != "Reconectando…" {
		t.Errorf("t0: %q", got)
	}
	clock = func() int64 { return t0 + leaseGraceSecs }
	if got := Assess(s).Health; got != Down {
		t.Errorf("tras gracia: %v", got)
	}
	other := status(false, "none")
	other["public_key"] = "k2"
	if got := Assess(other).Text; got != "Reconectando…" {
		t.Errorf("otra red: %q", got)
	}
	ok := status(true, "direct")
	ok["public_key"] = "k1"
	Assess(ok) // restablece
	if got := Assess(s).Text; got != "Reconectando…" {
		t.Errorf("tras restablecer: %q", got)
	}
}

func src(h Health, text string) Source { return Source{h, text} }

func TestTraySummaryCountsOnlySourcesThatAnswer(t *testing.T) {
	// Sin redes unidas: gris y «Sin redes».
	h, text := Summarize(0, nil)
	if h != Stopped || text != "Sin redes" {
		t.Errorf("inesperado: %v %q", h, text)
	}
	// Redes unidas pero ninguna responde (paradas a propósito): también gris, sin rojo.
	h, text = Summarize(2, nil)
	if h != Stopped || text != "Redes paradas o sin acceso al estado" {
		t.Errorf("inesperado: %v %q", h, text)
	}
	// Una red detenida no pone rojo el icono de la que sí responde: una sola fuente, el mismo texto de siempre.
	h, text = Summarize(2, []Source{src(Ok, "Conectado")})
	if h != Ok || text != "Conectado" {
		t.Errorf("inesperado: %v %q", h, text)
	}
	// Varias: gana la peor.
	h, text = Summarize(2, []Source{src(Ok, "Conectado"), src(Degraded, "Degradado: por relay")})
	if h != Degraded || text != "2 redes · Degradado: por relay" {
		t.Errorf("inesperado: %v %q", h, text)
	}
	// En empate gana la última, como max_by_key de Rust.
	_, text = Summarize(2, []Source{src(Down, "uno"), src(Down, "dos")})
	if text != "2 redes · dos" {
		t.Errorf("empate: %q", text)
	}
}
