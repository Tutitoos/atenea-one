// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const (
	winCLI   = `C:\Program Files\Atenea\atenea.exe`
	winNonce = "0123456789abcdef0123456789abcdef"
)

func TestWindowsScriptIsOneCLICallPerOperationWithTheResultChannel(t *testing.T) {
	const code = "ATENEA1-ABCD-2345"
	for _, tc := range []struct {
		op   Op
		code string
		want string
	}{
		// `join` y `up` en una sola llamada: no hay shell que encadene `&&`.
		{OpJoin, code, `"` + winCLI + `" network join ` + code + ` --yes --up --result ` + winNonce},
		{OpUp, "", `"` + winCLI + `" network up ` + testID + ` --result ` + winNonce},
		{OpDown, "", `"` + winCLI + `" network down ` + testID + ` --result ` + winNonce},
		{OpLeave, "", `"` + winCLI + `" network leave ` + testID + ` --yes --result ` + winNonce},
	} {
		got, err := WindowsScript(winCLI, tc.op, testID, tc.code, winNonce)
		if err != nil || got != tc.want {
			t.Errorf("op %d: %q, %v; esperaba %q", tc.op, got, err, tc.want)
		}
	}
}

func TestWindowsScriptRejectsWhatCouldEscapeTheQuoting(t *testing.T) {
	for _, tc := range []struct {
		name, cli, network, code, nonce string
		op                              Op
	}{
		{"CLI relativo", `atenea.exe`, testID, "", winNonce, OpUp},
		{"CLI con comilla simple", `C:\a'b\atenea.exe`, testID, "", winNonce, OpUp},
		{"CLI con comilla doble", `C:\a"b\atenea.exe`, testID, "", winNonce, OpUp},
		{"CLI que no es un exe", `C:\Program Files\Atenea\atenea.cmd`, testID, "", winNonce, OpUp},
		{"CLI con ..", `C:\Program Files\..\atenea.exe`, testID, "", winNonce, OpUp},
		{"CLI con metacaracteres", `C:\a;b\atenea.exe`, testID, "", winNonce, OpUp},
		{"red con inyección", winCLI, testID + "; calc", "", winNonce, OpUp},
		{"red en mayúsculas", winCLI, strings.ToUpper(testID), "", winNonce, OpDown},
		{"nonce corto", winCLI, testID, "", "abc", OpUp},
		{"nonce con mayúsculas", winCLI, testID, "", strings.ToUpper(winNonce), OpUp},
		{"join sin código", winCLI, testID, "", winNonce, OpJoin},
		{"join con código ajeno", winCLI, testID, "ATENEA1-ab'c", winNonce, OpJoin},
		{"migrar no existe en Windows", winCLI, testID, "", winNonce, OpMigrate},
	} {
		if got, err := WindowsScript(tc.cli, tc.op, tc.network, tc.code, tc.nonce); err == nil {
			t.Errorf("%s: aceptado %q", tc.name, got)
		}
	}
}

func TestWindowsElevationRunsPowerShellWithRunAsAndMapsCancellation(t *testing.T) {
	t.Setenv("SystemRoot", `D:\Windows`)
	script, err := WindowsScript(winCLI, OpUp, testID, "", winNonce)
	if err != nil {
		t.Fatal(err)
	}
	prog, args, err := ElevationCommand("windows", script, "texto que Windows no usa")
	if err != nil || prog != `D:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe` {
		t.Fatalf("%q %v", prog, err)
	}
	if len(args) != 4 || args[0] != "-NoProfile" || args[1] != "-NonInteractive" || args[2] != "-Command" {
		t.Fatalf("argumentos inesperados: %q", args)
	}
	for _, want := range []string{
		`-FilePath '` + winCLI + `'`,
		`-ArgumentList 'network up ` + testID + ` --result ` + winNonce + `'`,
		`-Verb RunAs`, `-Wait`, `-PassThru`, `-WindowStyle Hidden`,
		`exit $p.ExitCode`,
		// El diálogo cerrado (error 1223) sale con 126, que runCtx traduce a «Cancelado».
		`NativeErrorCode -eq 1223) { exit 126 }`,
	} {
		if !strings.Contains(args[3], want) {
			t.Errorf("falta %q en %s", want, args[3])
		}
	}
	if strings.Contains(args[3], "texto que Windows no usa") {
		t.Error("el aviso de Unix no debe viajar a PowerShell")
	}
}

func TestWindowsElevationRefusesAnythingThatIsNotAWindowsScript(t *testing.T) {
	for _, script := range []string{
		"", "x", winCLI + " network up " + testID, // sin comillas
		`"` + winCLI + `"`, // sin argumentos
		`"` + winCLI + `" network up ` + testID + `; calc`,           // encadena
		`"` + winCLI + `" network up ` + testID + ` 'x'`,             // cierra la cita de PowerShell
		`"` + winCLI + `" network up ` + testID + ` $(calc)`,         // subexpresión
		`"relativo.exe" network up ` + testID,                        // ruta relativa
		`"` + winCLI + `" network  up ` + testID,                     // doble espacio
		`"C:\a'b\atenea.exe" network up ` + testID + ` --result abc`, // comilla simple en el exe
	} {
		if _, _, err := ElevationCommand("windows", script, ""); err == nil {
			t.Errorf("aceptado %q", script)
		}
	}
}

func TestNonceIs128BitsOfLowercaseHexAndNeverRepeats(t *testing.T) {
	a, err := newNonce()
	if err != nil || !ValidNonce(a) {
		t.Fatalf("%q %v", a, err)
	}
	if b, _ := newNonce(); a == b {
		t.Error("dos nonces iguales")
	}
	for _, bad := range []string{"", "abc", winNonce + "0", strings.ToUpper(winNonce), "../" + winNonce[3:], strings.Repeat("g", 32)} {
		if ValidNonce(bad) {
			t.Errorf("nonce %q aceptado", bad)
		}
	}
}

func writeResult(t *testing.T, dir, nonce, body string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Join(dir, nonce), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, nonce, "result.json"), []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestReadResultAcceptsOnlyAShortWellFormedFileAndCleansTheMessage(t *testing.T) {
	dir := t.TempDir()
	writeResult(t, dir, winNonce, "{\"ok\":false,\"message\":\"línea 1\\nfallo\\u001b[31m rojo\\u0007\"}")
	r, err := readResult(dir, winNonce)
	if err != nil || r.OK || r.Message != "línea 1\nfallo[31m rojo" {
		t.Fatalf("%+v %v", r, err)
	}
	other := strings.Repeat("a", 32)
	writeResult(t, dir, other, `{"ok":true}`)
	if r, err := readResult(dir, other); err != nil || !r.OK {
		t.Fatalf("%+v %v", r, err)
	}
	big := strings.Repeat("b", 32)
	writeResult(t, dir, big, `{"ok":false,"message":"`+strings.Repeat("x", maxResultBytes)+`"}`)
	if _, err := readResult(dir, big); err == nil {
		t.Error("un resultado enorme debía rechazarse")
	}
	bad := strings.Repeat("c", 32)
	writeResult(t, dir, bad, `no es json`)
	if _, err := readResult(dir, bad); err == nil {
		t.Error("JSON roto aceptado")
	}
	if _, err := readResult(dir, strings.Repeat("d", 32)); err == nil {
		t.Error("un resultado ausente debía fallar")
	}
	if _, err := readResult(dir, "../x"); err == nil {
		t.Error("un nonce con ruta debía rechazarse antes de abrir nada")
	}
}

// stubElevate sustituye Elevate: guarda la orden y, como haría el CLI elevado, deja su result.json bajo el nonce que
// la aplicación puso en `--result`.
func stubElevate(t *testing.T, result string, runErr error) *[]string {
	t.Helper()
	programData := t.TempDir()
	t.Setenv("ProgramData", programData)
	var scripts []string
	old := Elevate
	Elevate = func(script, _ string) (string, error) {
		scripts = append(scripts, script)
		if result != "" {
			nonce := script[strings.LastIndex(script, " ")+1:]
			writeResult(t, filepath.Join(programData, "Atenea", "ui-results"), nonce, result)
		}
		return "", runErr
	}
	t.Cleanup(func() { Elevate = old })
	return &scripts
}

func TestApplyWindowsReportsTheMessageTheElevatedCLILeft(t *testing.T) {
	for _, tc := range []struct {
		name, result string
		runErr       error
		wantErr      string // vacío: éxito
	}{
		{"éxito", `{"ok":true}`, nil, ""},
		{"fallo del CLI con proceso en error", `{"ok":false,"message":"la red ya está unida"}`, errors.New("salida 1"), "la red ya está unida"},
		{"fallo del CLI aunque el proceso saliera bien", `{"ok":false,"message":"no hay red"}`, nil, "no hay red"},
		{"sin resultado y proceso en error", "", errors.New("salida 1"), "salida 1"},
		{"sin resultado y proceso correcto", "", nil, "sin dejar resultado"},
		{"cancelado en UAC", `{"ok":true}`, errors.New("Cancelado: no se ha cambiado nada"), "Cancelado"},
	} {
		scripts := stubElevate(t, tc.result, tc.runErr)
		_, err := applyWindows(winCLI, OpUp, testID, "", "")
		switch {
		case tc.wantErr == "" && err != nil:
			t.Errorf("%s: %v", tc.name, err)
		case tc.wantErr != "" && (err == nil || !strings.Contains(err.Error(), tc.wantErr)):
			t.Errorf("%s: %v, esperaba %q", tc.name, err, tc.wantErr)
		}
		if len(*scripts) != 1 || !strings.HasPrefix((*scripts)[0], `"`+winCLI+`" network up `+testID+` --result `) {
			t.Errorf("%s: órdenes %q", tc.name, *scripts)
		}
	}
}

func TestApplyWindowsNeverLeavesTheNetworkToTheCaller(t *testing.T) {
	scripts := stubElevate(t, `{"ok":true}`, nil)
	if _, err := applyWindows(winCLI, OpLeave, "no-es-uuid", "", ""); err == nil {
		t.Error("un id inválido no debe elevarse")
	}
	if len(*scripts) != 0 {
		t.Errorf("se elevó una orden con un id inválido: %q", *scripts)
	}
	if _, err := applyWindows(winCLI, OpMigrate, testID, "", ""); err == nil || len(*scripts) != 0 {
		t.Errorf("migrar no existe en Windows: %v %q", err, *scripts)
	}
}
