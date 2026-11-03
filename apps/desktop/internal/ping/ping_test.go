// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package ping

import (
	"net/netip"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

func TestSummariesMacOSLinuxIPv6AndLoss(t *testing.T) {
	for _, summary := range []string{
		"5 packets transmitted, 5 packets received, 0.0% packet loss\nround-trip min/avg/max/stddev = 0.1/0.2/0.3/0.1 ms",
		"5 packets transmitted, 5 received, 0% packet loss, time 801ms\nrtt min/avg/max/mdev = 0.1/0.2/0.3/0.1 ms",
		"PING6(56=40+8+8 bytes) fd00::1 --> fd00::2\n8 bytes from fd00::2, icmp_seq=0 hlim=64 time=0.2 ms\n5 packets transmitted, 5 packets received, 0.0% packet loss\nround-trip min/avg/max/std-dev = 0.1/0.2/0.3/0.1 ms",
	} {
		r, err := Parse(summary)
		if err != nil || r.Received != 5 || r.AvgMs == nil || *r.AvgMs != 0.2 {
			t.Fatalf("Parse(%q) = %+v, %v", summary, r, err)
		}
	}
	partial, err := Parse("5 packets transmitted, 3 received, 40% packet loss\nrtt min/avg/max/mdev = 1/2/3/0 ms")
	if err != nil || partial.LossPct != 40.0 {
		t.Fatalf("parcial: %+v, %v", partial, err)
	}
	lost, err := Parse("5 packets transmitted, 0 packets received, 100.0% packet loss")
	if err != nil || lost.AvgMs != nil {
		t.Fatalf("perdido: %+v, %v", lost, err)
	}
	errs, err := Parse("5 packets transmitted, 0 received, +5 errors, 100% packet loss")
	if err != nil || errs.MinMs != nil {
		t.Fatalf("con errores: %+v, %v", errs, err)
	}
	for _, corrupt := range []string{
		"",
		"PING ::1",
		"5 packets transmitted, six received, 0% packet loss",
		"5 packets transmitted, 6 received, 0% packet loss",
		"5 packets transmitted, 1 received, 80% packet loss",
	} {
		if _, err := Parse(corrupt); err == nil || err.Error() != "respuesta de ping no válida" {
			t.Errorf("Parse(%q) debía fallar con el mensaje del Rust, got %v", corrupt, err)
		}
	}
}

func TestResultJSONMatchesSerdeValue(t *testing.T) {
	r, _ := Parse("5 packets transmitted, 5 received, 0% packet loss\nrtt min/avg/max/mdev = 1/2.5/3/0 ms")
	b, _ := r.MarshalJSON()
	want := `{"avg_ms":2.5,"loss_pct":0.0,"max_ms":3.0,"min_ms":1.0,"received":5,"sent":5}`
	if string(b) != want {
		t.Fatalf("got %s", b)
	}
	lost, _ := Parse("5 packets transmitted, 0 received, 100% packet loss")
	b, _ = lost.MarshalJSON()
	want = `{"avg_ms":null,"loss_pct":100.0,"max_ms":null,"min_ms":null,"received":0,"sent":5}`
	if string(b) != want {
		t.Fatalf("got %s", b)
	}
}

func TestValidatesOnlyRealPeerAddresses(t *testing.T) {
	status := map[string]any{
		"peers": []any{map[string]any{"addrs": []any{"10.50.0.2", "fd00::2"}}},
		"addrs": []any{"10.50.0.1"},
	}
	if ip, err := Validate("fd00::2", status); err != nil || ip.String() != "fd00::2" {
		t.Fatalf("fd00::2: %v, %v", ip, err)
	}
	if _, err := Validate("10.50.0.2", status); err != nil {
		t.Fatal(err)
	}
	for _, address := range []string{"10.50.0.1", "10.60.0.2", "example.test", "10.50.0.2;id", "-c5", "fd00::2%en0"} {
		if _, err := Validate(address, status); err == nil {
			t.Errorf("%q debía rechazarse", address)
		}
	}
	if _, err := Validate("example.test", status); err == nil || err.Error() != "dirección IP no válida" {
		t.Errorf("mensaje: %v", err)
	}
	if _, err := Validate("10.60.0.2", status); err == nil || err.Error() != "la dirección no pertenece a un par de esta red" {
		t.Errorf("mensaje: %v", err)
	}
}

func TestCommandPerOS(t *testing.T) {
	v4, v6 := netip.MustParseAddr("10.50.0.2"), netip.MustParseAddr("fd00::2")
	cases := []struct {
		goos string
		ip   netip.Addr
		prog string
		args []string
	}{
		{"darwin", v4, "/sbin/ping", []string{"-n", "-c", "5", "-i", "0.2", "-W", "1000", "10.50.0.2"}},
		{"darwin", v6, "/sbin/ping6", []string{"-n", "-c", "5", "-i", "0.2", "-W", "1000", "fd00::2"}},
		{"linux", v4, "ping", []string{"-n", "-c", "5", "-i", "0.2", "-w", "8", "-W", "1", "10.50.0.2"}},
		{"linux", v6, "ping", []string{"-6", "-n", "-c", "5", "-i", "0.2", "-w", "8", "-W", "1", "fd00::2"}},
	}
	for _, c := range cases {
		prog, args, err := Command(c.goos, c.ip)
		if err != nil || prog != c.prog || !reflect.DeepEqual(args, c.args) {
			t.Errorf("%s %s: %q %q %v", c.goos, c.ip, prog, args, err)
		}
	}
	if _, _, err := Command("windows", v4); err == nil || err.Error() != "no soportado en Windows" {
		t.Errorf("windows: %v", err)
	}
}

func TestRunUsesInjectedLauncher(t *testing.T) {
	old := Exec
	defer func() { Exec = old }()
	var gotProg string
	var gotArgs []string
	var gotTimeout time.Duration
	Exec = func(p string, a []string, d time.Duration) (string, error) {
		gotProg, gotArgs, gotTimeout = p, a, d
		return "5 packets transmitted, 5 received, 0% packet loss\nrtt min/avg/max/mdev = 1/2/3/0 ms", nil
	}
	out, err := run("linux", netip.MustParseAddr("10.50.0.2"), deadline)
	want := `{"avg_ms":2.0,"loss_pct":0.0,"max_ms":3.0,"min_ms":1.0,"received":5,"sent":5}`
	if err != nil || out != want || gotProg != "ping" || gotArgs[len(gotArgs)-1] != "10.50.0.2" || gotTimeout != 9*time.Second {
		t.Fatalf("%q %v %q %v %v", out, err, gotProg, gotArgs, gotTimeout)
	}
	Exec = func(string, []string, time.Duration) (string, error) { return "basura", nil }
	if _, err := run("linux", netip.MustParseAddr("10.50.0.2"), deadline); err == nil {
		t.Fatal("salida corrupta debía fallar")
	}
	if _, err := run("windows", netip.MustParseAddr("10.50.0.2"), deadline); err == nil {
		t.Fatal("windows debía fallar")
	}
}

// El proceso de prueba se reejecuta como «ping» falso (patrón de os/exec).
func TestHelperProcess(t *testing.T) {
	switch os.Getenv("ATENEA_PING_HELPER") {
	case "":
		return
	case "ok":
		os.Stdout.WriteString("lc=" + os.Getenv("LC_ALL") + "\n")
		os.Exit(1) // el código de salida no cuenta, solo la salida
	case "sleep":
		time.Sleep(30 * time.Second)
	}
	os.Exit(0)
}

func TestExecuteCapturesOutputWithCLocaleAndKillsAtDeadline(t *testing.T) {
	t.Setenv("ATENEA_PING_HELPER", "ok")
	out, err := execute(os.Args[0], []string{"-test.run=TestHelperProcess"}, 5*time.Second)
	if err != nil || !strings.Contains(out, "lc=C") {
		t.Fatalf("out=%q err=%v", out, err)
	}
	t.Setenv("ATENEA_PING_HELPER", "sleep")
	start := time.Now()
	_, err = execute(os.Args[0], []string{"-test.run=TestHelperProcess"}, 300*time.Millisecond)
	if err == nil || err.Error() != "ping agotó el plazo de 9 s" || time.Since(start) > 5*time.Second {
		t.Fatalf("err=%v tras %v", err, time.Since(start))
	}
	if _, err := execute("/no/existe/ping", nil, time.Second); err == nil || !strings.HasPrefix(err.Error(), "no se pudo ejecutar ping: ") {
		t.Fatalf("spawn: %v", err)
	}
}
