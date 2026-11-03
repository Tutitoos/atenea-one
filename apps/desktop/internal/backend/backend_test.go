// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// Los casos son los de las pruebas de apps/desktop/src/networks.rs: paridad demostrada, no supuesta.
const (
	testID  = "0199ab12-3456-7def-8abc-0123456789ab"
	testCLI = "/Library/Atenea/bin/atenea"
)

func TestUUIDMustBeCanonicalLowercase(t *testing.T) {
	if !ValidUUID(testID) {
		t.Fatal("el uuid canónico debe ser válido")
	}
	for _, bad := range []string{"", "0199AB12-3456-7def-8abc-0123456789ab", "0199ab123456-7def-8abc-0123456789ab",
		"0199ab12-3456-7def-8abc-0123456789a;", "0199ab12-3456-7def-8abc-0123456789abc", "$(id)"} {
		if ValidUUID(bad) {
			t.Errorf("uuid inválido aceptado: %q", bad)
		}
	}
	if ShortID(testID) != "456789ab" {
		t.Errorf("ShortID = %q", ShortID(testID))
	}
}

func TestPastedCodeIsNormalizedAndOnlyTheFormatAlphabetPasses(t *testing.T) {
	ok, err := NormalizeCode("  atenea1-abcd ef-gh23\n-7x ")
	if err != nil || ok != "ATENEA1-ABCDEF-GH23-7X" {
		t.Fatalf("NormalizeCode = %q, %v", ok, err)
	}
	for _, bad := range []string{"ATENEA1-", "ABCDEF-GHIJ", "ATENEA1-ABC1", "ATENEA1-AB;rm -rf", "ATENEA1-AB\"CD",
		"ATENEA1-AB$(id)", "ATENEA1-AB`id`", "ATENEA1-AB&&id", "ATENEA1-AB\\CD", "ATENEA1-" + strings.Repeat("A", 512)} {
		if _, err := NormalizeCode(bad); err == nil {
			t.Errorf("código inválido aceptado: %q", bad)
		}
	}
	if _, err := NormalizeCode("ATENEA1-" + strings.Repeat("A", 504)); err != nil {
		t.Errorf("512 bytes exactos deben pasar: %v", err)
	}
}

func TestPrivilegedScriptsAreExactlyTheAllowedCLICalls(t *testing.T) {
	code := "ATENEA1-ABCDEF-GH23"
	cases := []struct {
		op   Op
		code string
		want string
	}{
		{OpJoin, code, testCLI + " network join " + code + " --yes --status-gid 20 && " + testCLI + " network up " + testID},
		{OpUp, "", testCLI + " network up " + testID},
		{OpDown, "", testCLI + " network down " + testID},
		{OpLeave, "", testCLI + " network leave " + testID + " --yes"},
	}
	for _, c := range cases {
		got, err := Script(testCLI, c.op, testID, c.code, "", 20)
		if err != nil || got != c.want {
			t.Errorf("op %d: %q, %v", c.op, got, err)
		}
	}
}

func TestNoUnvalidatedTextReachesThePrivilegedScript(t *testing.T) {
	for _, badID := range []string{"", "0199ab12; id", "sudo", "--yes"} {
		for _, op := range []Op{OpUp, OpDown, OpLeave, OpJoin} {
			if _, err := Script(testCLI, op, badID, "ATENEA1-ABCD", "", 20); err == nil {
				t.Errorf("op %d con id %q aceptado", op, badID)
			}
		}
	}
	if _, err := Script(testCLI, OpJoin, testID, "", "", 20); err == nil {
		t.Error("join sin código aceptado")
	}
	if _, err := Script(testCLI, OpJoin, testID, "ATENEA1-AB; id", "", 20); err == nil {
		t.Error("código con metacaracteres aceptado")
	}
	for _, cli := range []string{"/x; id", "/x\"y"} {
		if _, err := Script(cli, OpUp, testID, "", "", 0); err == nil {
			t.Errorf("ruta del CLI %q aceptada", cli)
		}
	}
}

func TestMigrateScriptIsExactAndNameIsOptional(t *testing.T) {
	base := testCLI + " network migrate-legacy --yes"
	for _, name := range []string{""} {
		if got, err := Script(testCLI, OpMigrate, "", "", name, 0); err != nil || got != base {
			t.Errorf("sin nombre: %q, %v", got, err)
		}
	}
	got, err := Script(testCLI, OpMigrate, "", "", "Red piloto-1.a_b", 0)
	if err != nil || got != base+" --name 'Red piloto-1.a_b'" {
		t.Errorf("con nombre: %q, %v", got, err)
	}
	op, err := ParseOp("migrate")
	if err != nil || op != OpMigrate {
		t.Errorf("ParseOp(migrate) = %d, %v", op, err)
	}
}

func TestMigrateNameRejectsInjectionControlsBidiAndLength(t *testing.T) {
	for _, bad := range []string{"a'b", "a\"b", "a;id", "$(id)", "`id`", "a&&b", "a\\b", "a\nb", "a\tb", "a\u202eb",
		"a\u200fb", "ñandú", " lead", "trail ", "--yes", strings.Repeat("a", 65)} {
		if _, err := Script(testCLI, OpMigrate, "", "", bad, 0); err == nil {
			t.Errorf("nombre inválido aceptado: %q", bad)
		}
	}
	if _, err := Script(testCLI, OpMigrate, "", "", strings.Repeat("a", 64), 0); err != nil {
		t.Errorf("64 caracteres deben pasar: %v", err)
	}
}

func TestOnlyListedOperationsExist(t *testing.T) {
	if op, err := ParseOp("join"); err != nil || op != OpJoin {
		t.Fatalf("join: %d, %v", op, err)
	}
	for _, bad := range []string{"", "JOIN", "shell", "list", "up; id", "leave ", "migrate-legacy"} {
		if _, err := ParseOp(bad); err == nil {
			t.Errorf("operación inválida aceptada: %q", bad)
		}
	}
}

func TestElevationCommandPerOS(t *testing.T) {
	prog, args, err := ElevationCommand("darwin", testCLI+" network up "+testID, "Atenea One quiere conectar este equipo a una red.")
	if err != nil || prog != "/usr/bin/osascript" || len(args) != 2 || args[0] != "-e" ||
		!strings.HasPrefix(args[1], "do shell script \"") || !strings.Contains(args[1], "with administrator privileges") {
		t.Errorf("darwin: %q %q %v", prog, args, err)
	}
	prog, args, err = ElevationCommand("linux", "x", "p")
	if err != nil || prog != "/usr/bin/pkexec" || len(args) != 3 || args[0] != "/bin/sh" {
		t.Errorf("linux: %q %q %v", prog, args, err)
	}
	if _, _, err := ElevationCommand("windows", "x", "p"); err == nil {
		t.Error("windows no debe estar soportado aquí")
	}
	if _, _, err := ElevationCommand("darwin", "a\"b", "p"); err == nil {
		t.Error("comillas en la orden deben rechazarse")
	}
	if _, _, err := ElevationCommand("darwin", "ok", "a\\b"); err == nil {
		t.Error("barras invertidas en el aviso deben rechazarse")
	}
}

// socketServer arranca un servidor Unix que responde con `reply` a cada conexión y devuelve la ruta. El
// directorio es corto: las rutas de socket Unix tienen un límite de unos 100 caracteres.
func socketServer(t *testing.T, reply func(c net.Conn)) string {
	t.Helper()
	dir, err := os.MkdirTemp("", "s")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(dir) })
	path := filepath.Join(dir, "a.sock")
	l, err := net.Listen("unix", path)
	if err != nil {
		t.Skipf("este sistema no admite sockets Unix: %v", err)
	}
	t.Cleanup(func() { l.Close() })
	go func() {
		for {
			c, err := l.Accept()
			if err != nil {
				return
			}
			go func() { defer c.Close(); reply(c) }()
		}
	}()
	return path
}

func readRequest(c net.Conn) string {
	buf := make([]byte, 16)
	n, _ := c.Read(buf)
	return string(buf[:n])
}

func TestAskSendsStatusAndReturnsValidatedJSON(t *testing.T) {
	var got string
	path := socketServer(t, func(c net.Conn) {
		got = readRequest(c)
		c.Write([]byte("{\"a\":1}\n"))
	})
	out, err := Ask(path, MaxStatusBytes, 2*time.Second)
	if err != nil || out != "{\"a\":1}" || got != "status\n" {
		t.Fatalf("out=%q err=%v petición=%q", out, err, got)
	}
}

func TestAskRejectsAgentErrorsEmptyAndInvalidReplies(t *testing.T) {
	for name, tc := range map[string]struct{ reply, want string }{
		"error":           {"{\"error\":\"x\"}\n", "agente: x"},
		"error sin texto": {"{\"error\":5}\n", "agente: orden rechazada"},
		"vacía":           {"", "respuesta vacía o JSON inválido"},
		"no json":         {"hola\n", "respuesta vacía o JSON inválido"},
	} {
		path := socketServer(t, func(c net.Conn) { readRequest(c); c.Write([]byte(tc.reply)) })
		if _, err := Ask(path, MaxStatusBytes, 2*time.Second); err == nil || !strings.Contains(err.Error(), tc.want) {
			t.Errorf("%s: err=%v, quería %q", name, err, tc.want)
		}
	}
}

func TestAskCapsTheReplyAtFourMiB(t *testing.T) {
	big := "{\"x\":\"" + strings.Repeat("a", 5<<20) + "\"}\n"
	path := socketServer(t, func(c net.Conn) { readRequest(c); c.Write([]byte(big)) })
	start := time.Now()
	_, err := Ask(path, MaxStatusBytes, 5*time.Second)
	if err == nil || !strings.Contains(err.Error(), "JSON inválido") {
		t.Fatalf("una respuesta de 5 MiB debe truncarse y fallar: %v", err)
	}
	if time.Since(start) > 4*time.Second {
		t.Errorf("tardó demasiado: %v", time.Since(start))
	}
}

func TestAskTimesOutWithoutReply(t *testing.T) {
	path := socketServer(t, func(c net.Conn) { readRequest(c); time.Sleep(3 * time.Second) })
	start := time.Now()
	if _, err := Ask(path, MaxStatusBytes, 300*time.Millisecond); err == nil {
		t.Fatal("sin respuesta debe fallar")
	}
	if time.Since(start) > 2*time.Second {
		t.Errorf("no respetó el plazo: %v", time.Since(start))
	}
}

// El proceso de prueba se reejecuta a sí mismo como «CLI» falso (patrón de os/exec): portátil, sin scripts.
func TestHelperProcess(t *testing.T) {
	mode := os.Getenv("ATENEA_SPIKE_HELPER")
	if mode == "" {
		return
	}
	switch mode {
	case "ok":
		os.Stdout.WriteString("hola\n")
	case "fail":
		os.Stderr.WriteString("falló\n")
		os.Exit(3)
	case "cancel":
		os.Exit(126)
	case "usercancel":
		os.Stderr.WriteString("execution error: User canceled. (-128)\n")
		os.Exit(1)
	}
	os.Exit(0)
}

func TestRunMapsExitsAndCancellations(t *testing.T) {
	run := func(mode string) (string, error) {
		t.Setenv("ATENEA_SPIKE_HELPER", mode)
		return Run(os.Args[0], "-test.run=TestHelperProcess")
	}
	if out, err := run("ok"); err != nil || out != "hola" {
		t.Errorf("ok: %q %v", out, err)
	}
	if _, err := run("fail"); err == nil || err.Error() != "falló" {
		t.Errorf("fail: %v", err)
	}
	for _, m := range []string{"cancel", "usercancel"} {
		if _, err := run(m); err == nil || !strings.HasPrefix(err.Error(), "Cancelado") {
			t.Errorf("%s: %v", m, err)
		}
	}
	if _, err := Run(filepath.Join(os.TempDir(), "no-existe-xyz")); err == nil {
		t.Error("un programa inexistente debe fallar")
	}
}
