// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package notify

import (
	"encoding/json"
	"errors"
	"reflect"
	"runtime"
	"strings"
	"testing"
)

func status(path string, lease bool, expiry any) map[string]any {
	return map[string]any{
		"config": map[string]any{"lease_active": lease},
		"peers":  []any{map[string]any{"public_key": "synthetic", "name": "Par", "path": path}},
		"server": map[string]any{"issuer_cert_not_after_unix": expiry},
	}
}

func TestFirstSampleNoneAndRelayDebounce(t *testing.T) {
	var s State
	steps := []struct {
		path string
		now  int64
		want int
	}{{"direct", 0, 0}, {"relay-udp", 10, 0}, {"relay-tls", 69, 0}, {"relay-tls", 70, 1}, {"relay-tls", 80, 0}, {"none", 90, 0}, {"relay-udp", 200, 0}}
	for _, st := range steps {
		got := s.Sample("conectada", status(st.path, true, int64(9000000)), st.now)
		if len(got) != st.want {
			t.Fatalf("%s@%d: %v", st.path, st.now, got)
		}
	}
	var s2 State
	s2.Sample("conectada", status("direct", true, int64(9000000)), 0)
	s2.Sample("conectada", status("relay-udp", true, int64(9000000)), 10)
	got := s2.Sample("conectada", status("relay-udp", true, int64(9000000)), 70)
	if !reflect.DeepEqual(got, []string{"Par lleva un minuto por relay"}) {
		t.Fatalf("%v", got)
	}
}

func TestConnectionLossLeaseConflictAndRecovery(t *testing.T) {
	cases := []struct {
		state  string
		status map[string]any
	}{
		{"conflicto", status("direct", true, int64(9000000))},
		{"conectada", nil},
		{"conectada", status("direct", false, int64(9000000))},
	}
	for _, c := range cases {
		var s State
		s.Sample("conectada", status("direct", true, int64(9000000)), 0)
		if got := s.Sample(c.state, c.status, 1); !reflect.DeepEqual(got, []string{"La red ha perdido la conexión"}) {
			t.Fatalf("%s: %v", c.state, got)
		}
		if got := s.Sample(c.state, c.status, 2); len(got) != 0 {
			t.Fatalf("repetido: %v", got)
		}
		if got := s.Sample("conectada", status("direct", true, int64(9000000)), 3); !reflect.DeepEqual(got, []string{"La red vuelve a estar conectada"}) {
			t.Fatalf("recuperación: %v", got)
		}
	}
}

func TestCertificateRateAndMissingFields(t *testing.T) {
	var s State
	v := status("none", true, int64(100))
	for _, st := range []struct {
		now  int64
		want int
	}{{0, 0}, {1, 1}, {86400, 0}, {86401, 1}} {
		if got := s.Sample("conectada", v, st.now); len(got) != st.want {
			t.Fatalf("@%d: %v", st.now, got)
		}
	}
	var d State
	d.Sample("detenida", nil, 0)
	if got := d.Sample("detenida", nil, 1); len(got) != 0 {
		t.Fatalf("%v", got)
	}
}

func TestCertificateNumberKinds(t *testing.T) {
	var s State
	s.Sample("conectada", status("none", true, json.Number("100")), 0)
	if got := s.Sample("conectada", status("none", true, json.Number("100")), 1); len(got) != 1 {
		t.Fatalf("json.Number: %v", got)
	}
	for _, n := range []any{json.Number("100.5"), json.Number("1e2"), 100.5, "100", nil, float64(1e30)} {
		var f State
		f.Sample("conectada", status("none", true, n), 0)
		if got := f.Sample("conectada", status("none", true, n), 1); len(got) != 0 {
			t.Fatalf("%v no debe contar: %v", n, got)
		}
	}
	var f State
	f.Sample("conectada", status("none", true, float64(100)), 0)
	if got := f.Sample("conectada", status("none", true, float64(100)), 1); len(got) != 1 {
		t.Fatalf("float64 entero: %v", got)
	}
}

func TestSanitizeInterpreterTextAndBoundsLength(t *testing.T) {
	if got := Sanitize("a\"b'c\\d\ne\u202ef\u2067g"); got != "abcdefg" {
		t.Fatalf("%q", got)
	}
	if n := len([]rune(Sanitize(strings.Repeat("ñ", 300)))); n != 180 {
		t.Fatalf("%d", n)
	}
}

func TestCommandPerOS(t *testing.T) {
	if _, _, ok := Command("darwin", "x"); ok {
		t.Fatal("macOS entrega con Native, no con un programa")
	}
	p, a, ok := Command("linux", "Red 'de'\u202e prueba")
	if !ok || p != "notify-send" || !reflect.DeepEqual(a, []string{"--", "Atenea One", "Red de prueba"}) {
		t.Fatalf("%v %v %v", p, a, ok)
	}
	if _, _, ok := Command("windows", "x"); ok {
		t.Fatal("windows no soportado")
	}
}

func TestDeliverPrefersNativeThenRun(t *testing.T) {
	oldRun, oldNative := Run, Native
	defer func() { Run, Native = oldRun, oldNative }()
	var program string
	var args []string
	Run = func(p string, a []string) error { program, args = p, a; return errors.New("fallo") }

	var got string
	Native = func(text string) error { got = text; return nil }
	if err := Deliver("Red \"de\" prueba"); err != nil || got != "Red de prueba" || program != "" {
		t.Fatalf("nativo: %v %q %q", err, got, program)
	}

	Native = nil
	err := Deliver("Red de prueba")
	if _, _, ok := Command(goosForTest(), "x"); !ok {
		if err != nil || program != "" {
			t.Fatalf("SO sin soporte: %v %q", err, program)
		}
		return
	}
	if err == nil || program == "" || len(args) == 0 {
		t.Fatalf("%v %q %v", err, program, args)
	}
}

func goosForTest() string { return runtime.GOOS }
