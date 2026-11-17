// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

const (
	adminDev  = "0199ab12-3456-7def-8abc-0123456789ad"
	adminDev2 = "0199ab12-3456-7def-8abc-0123456789ae"
	adminFP   = "1234-5678-9abc-def0-1234-5678-9abc-def0"
)

func TestAdminScriptsAreTheExactClosedCommands(t *testing.T) {
	cli := "/usr/local/bin/atenea"
	for _, tc := range []struct {
		op   AdminOp
		p    AdminParams
		want string
	}{
		{AdminJoinCode, AdminParams{TTL: 600}, cli + " network join-code --network " + testID + " --ttl 600 --json"},
		{AdminApprove, AdminParams{Fingerprint: adminFP}, cli + " network approve " + adminFP + " --network " + testID},
		{AdminRevoke, AdminParams{Device: adminDev}, cli + " network revoke " + adminDev + " --network " + testID + " --yes"},
		{AdminPolicyAdd, AdminParams{From: adminDev, To: adminDev2, Proto: "tcp", Ports: "22"}, cli + " network policy add --network " + testID + " --from " + adminDev + " --to " + adminDev2 + " --proto tcp --ports 22"},
		{AdminPolicyAdd, AdminParams{From: adminDev, To: adminDev2, Proto: "any"}, cli + " network policy add --network " + testID + " --from " + adminDev + " --to " + adminDev2 + " --proto any"},
		{AdminPolicyRemove, AdminParams{Policy: adminDev}, cli + " network policy remove " + adminDev + " --network " + testID + " --yes"},
	} {
		got, err := AdminScript(cli, tc.op, testID, tc.p)
		if err != nil || got != tc.want {
			t.Errorf("op %d: %q, %v; esperaba %q", tc.op, got, err, tc.want)
		}
	}
}

func TestAdminScriptRejectsAnythingOutsideTheClosedAlphabets(t *testing.T) {
	cli := "/usr/local/bin/atenea"
	bad := []struct {
		name string
		op   AdminOp
		net  string
		p    AdminParams
	}{
		{"red", AdminApprove, testID + "; id", AdminParams{Fingerprint: adminFP}},
		{"ttl corto", AdminJoinCode, testID, AdminParams{TTL: 59}},
		{"ttl largo", AdminJoinCode, testID, AdminParams{TTL: 3601}},
		{"huella corta", AdminApprove, testID, AdminParams{Fingerprint: "1234-5678"}},
		{"huella en mayúsculas", AdminApprove, testID, AdminParams{Fingerprint: strings.ToUpper(adminFP)}},
		{"huella con orden", AdminApprove, testID, AdminParams{Fingerprint: adminFP + "; rm"}},
		{"dispositivo", AdminRevoke, testID, AdminParams{Device: "--all"}},
		{"política", AdminPolicyRemove, testID, AdminParams{Policy: "x"}},
		{"origen", AdminPolicyAdd, testID, AdminParams{From: "web", To: adminDev2, Proto: "any"}},
		{"protocolo", AdminPolicyAdd, testID, AdminParams{From: adminDev, To: adminDev2, Proto: "sctp"}},
		{"puertos con icmp", AdminPolicyAdd, testID, AdminParams{From: adminDev, To: adminDev2, Proto: "icmp", Ports: "22"}},
		{"puertos en orden inverso", AdminPolicyAdd, testID, AdminParams{From: adminDev, To: adminDev2, Proto: "tcp", Ports: "90-80"}},
		{"puerto 0", AdminPolicyAdd, testID, AdminParams{From: adminDev, To: adminDev2, Proto: "tcp", Ports: "0"}},
		{"puerto enorme", AdminPolicyAdd, testID, AdminParams{From: adminDev, To: adminDev2, Proto: "udp", Ports: "70000"}},
		{"puertos con orden", AdminPolicyAdd, testID, AdminParams{From: adminDev, To: adminDev2, Proto: "tcp", Ports: "22; id"}},
	}
	for _, tc := range bad {
		if got, err := AdminScript(cli, tc.op, tc.net+"", tc.p); err == nil {
			t.Errorf("%s aceptado: %q", tc.name, got)
		}
	}
	if _, err := AdminScript("/usr/bin/atenea; id", AdminApprove, testID, AdminParams{Fingerprint: adminFP}); err == nil {
		t.Error("una ruta de CLI con metacaracteres debía rechazarse")
	}
	for _, op := range []string{"", "join", "policy", "approve --all", "JOIN-CODE"} {
		if _, err := ParseAdminOp(op); err == nil {
			t.Errorf("operación %q aceptada", op)
		}
	}
}

func TestApplyAdminElevatesOnceWithItsPromptAndReturnsOnlyTheCode(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("en Windows no está soportada")
	}
	fakeCLI(t, `echo '[]'`)
	trustAnyCLI(t)
	var calls []string
	old := Elevate
	Elevate = func(script, prompt string) (string, error) {
		calls = append(calls, script+" | "+prompt)
		return `{"token":"secreto","code":"ATENEA1-ABC","expires_at":"2026-11-17T10:00:00Z","join":{"token":"secreto"}}`, nil
	}
	t.Cleanup(func() { Elevate = old })

	out, err := ApplyAdmin("join-code", testID, AdminParams{TTL: 120})
	if err != nil {
		t.Fatal(err)
	}
	if out != `{"code":"ATENEA1-ABC","expires_at":"2026-11-17T10:00:00Z"}` || strings.Contains(out, "secreto") {
		t.Errorf("solo el código y su caducidad deben llegar a la página: %s", out)
	}
	want := CLIPath() + " network join-code --network " + testID + " --ttl 120 --json | Atenea One quiere generar un código de unión para una red."
	if len(calls) != 1 || calls[0] != want {
		t.Fatalf("órdenes = %q, esperaba %q", calls, want)
	}
	// Una validación fallida no eleva nada.
	if _, err := ApplyAdmin("approve", testID, AdminParams{Fingerprint: "corta"}); err == nil || len(calls) != 1 {
		t.Errorf("err=%v órdenes=%q", err, calls)
	}
	if _, err := ApplyAdmin("leave", testID, AdminParams{}); err == nil || len(calls) != 1 {
		t.Errorf("operación ajena aceptada: %v", err)
	}
	// Cancelar el diálogo no deja nada a medias y se informa tal cual.
	Elevate = func(string, string) (string, error) { return "", errors.New("Cancelado") }
	if _, err := ApplyAdmin("revoke", testID, AdminParams{Device: adminDev}); err == nil || err.Error() != "Cancelado" {
		t.Errorf("cancelación: %v", err)
	}
	// Un CLI que no devuelve código no se da por bueno.
	Elevate = func(string, string) (string, error) { return "hecho", nil }
	if _, err := ApplyAdmin("join-code", testID, AdminParams{TTL: 60}); err == nil {
		t.Error("una salida sin código debía fallar")
	}
}

func TestApplyAdminRefusesACLIThatAnUnprivilegedUserCanReplace(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("en Windows no está soportada")
	}
	fakeCLI(t, `echo '[]'`)
	if err := os.Chmod(filepath.Dir(CLIPath()), 0o777); err != nil {
		t.Fatal(err)
	}
	var elevated []string
	old := Elevate
	Elevate = func(script, prompt string) (string, error) { elevated = append(elevated, script); return "", nil }
	t.Cleanup(func() { Elevate = old })
	if _, err := ApplyAdmin("revoke", testID, AdminParams{Device: adminDev}); err == nil || !strings.Contains(err.Error(), "ruta no confiable") {
		t.Errorf("ApplyAdmin: %v", err)
	}
	if len(elevated) != 0 {
		t.Errorf("no debe elevarse nada: %q", elevated)
	}
}
