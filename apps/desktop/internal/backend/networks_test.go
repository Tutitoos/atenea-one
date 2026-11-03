// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
)

// fakeCLI escribe un «CLI» de shell en un directorio temporal y lo fija como ATENEA_CLI. Devuelve el fichero donde
// el CLI vuelca sus argumentos (`$*`), para comprobar con qué lo llamó el backend.
func fakeCLI(t *testing.T, body string) string {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("el CLI falso es un script de shell")
	}
	dir := t.TempDir()
	args := filepath.Join(dir, "args")
	path := filepath.Join(dir, "atenea")
	script := "#!/bin/sh\nprintf '%s' \"$*\" > '" + args + "'\n" + body + "\n"
	if err := os.WriteFile(path, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("ATENEA_CLI", path)
	return args
}

func readArgs(t *testing.T, path string) string {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func TestListWithoutCLIIsAnEmptyList(t *testing.T) {
	t.Setenv("ATENEA_CLI", filepath.Join(t.TempDir(), "no-existe"))
	if got, err := List(); err != nil || got != "[]" {
		t.Fatalf("List = %q, %v", got, err)
	}
	if got, err := Legacy(); err != nil || got != `{"present":false}` {
		t.Fatalf("Legacy = %q, %v", got, err)
	}
}

func TestListAcceptsOnlyJSONArrays(t *testing.T) {
	args := fakeCLI(t, `echo '[{"network_id":"x"}]'`)
	if got, err := List(); err != nil || got != `[{"network_id":"x"}]` {
		t.Fatalf("List = %q, %v", got, err)
	}
	if got := readArgs(t, args); got != "network list --json" {
		t.Errorf("argumentos = %q", got)
	}
	for _, bad := range []string{`echo '{}'`, `echo hola`, `echo ''`} {
		fakeCLI(t, bad)
		if _, err := List(); err == nil || !strings.Contains(err.Error(), "respuesta inesperada") {
			t.Errorf("%s: err = %v", bad, err)
		}
	}
}

func TestLegacyRequiresABooleanPresent(t *testing.T) {
	fakeCLI(t, `echo '{"present":true}'`)
	if got, err := Legacy(); err != nil || got != `{"present":true}` {
		t.Fatalf("Legacy = %q, %v", got, err)
	}
	for _, bad := range []string{`echo '{"present":"si"}'`, `echo '[]'`, `echo x`} {
		fakeCLI(t, bad)
		if _, err := Legacy(); err == nil {
			t.Errorf("%s: debía fallar", bad)
		}
	}
}

func TestInspectNormalizesTheCodeBeforeCallingTheCLI(t *testing.T) {
	args := fakeCLI(t, `echo '{"name":"casa"}'`)
	if _, err := Inspect("  atenea1-abcd ef-gh23\n"); err != nil {
		t.Fatal(err)
	}
	if got := readArgs(t, args); got != "network inspect ATENEA1-ABCDEF-GH23 --json" {
		t.Errorf("argumentos = %q", got)
	}
	if _, err := Inspect("ATENEA1-AB; id"); err == nil {
		t.Error("un código con metacaracteres no debe llegar al CLI")
	}
	fakeCLI(t, `echo '[]'`)
	if _, err := Inspect("ATENEA1-ABCD"); err == nil || !strings.Contains(err.Error(), "respuesta inesperada") {
		t.Errorf("solo se admite un objeto: %v", err)
	}
}

// capture sustituye la elevación por un registro de lo que se habría ejecutado.
func capture(t *testing.T) *[]string {
	t.Helper()
	var calls []string
	old := Elevate
	Elevate = func(script, prompt string) (string, error) {
		calls = append(calls, script+" | "+prompt)
		return "hecho", nil
	}
	t.Cleanup(func() { Elevate = old })
	return &calls
}

func TestApplyJoinTakesTheNetworkIDFromTheCodeNotFromTheCaller(t *testing.T) {
	fakeCLI(t, `echo '{"network_id":"`+testID+`"}'`)
	calls := capture(t)
	other := "ffffffff-ffff-7fff-8fff-ffffffffffff"
	got, err := Apply("join", other, "atenea1-abcd-ef23", "")
	if err != nil || got != "hecho" {
		t.Fatalf("Apply = %q, %v", got, err)
	}
	cli := CLIPath()
	want := cli + " network join ATENEA1-ABCD-EF23 --yes --status-gid " + strconv.Itoa(os.Getgid()) + " && " + cli + " network up " + testID +
		" | Atenea One quiere unir este equipo a una red."
	if len(*calls) != 1 || (*calls)[0] != want {
		t.Fatalf("orden elevada = %v, esperaba %q", *calls, want)
	}
	if strings.Contains((*calls)[0], other) {
		t.Error("el id del llamante no debe aparecer en la orden")
	}
}

func TestApplyJoinRejectsMissingCodeAndBadIDInCode(t *testing.T) {
	calls := capture(t)
	fakeCLI(t, `echo '{"network_id":"nope"}'`)
	if _, err := Apply("join", "", "", ""); err == nil || err.Error() != "falta el código" {
		t.Errorf("sin código: %v", err)
	}
	if _, err := Apply("join", "", "ATENEA1-ABCD", ""); err == nil || err.Error() != "el código no trae un id de red válido" {
		t.Errorf("id inválido en el código: %v", err)
	}
	if len(*calls) != 0 {
		t.Errorf("no debe elevarse nada: %v", *calls)
	}
}

func TestApplyOtherOperationsUseTheCallerNetworkAndTheirOwnPrompt(t *testing.T) {
	fakeCLI(t, `echo '[]'`)
	calls := capture(t)
	cli := CLIPath()
	for op, want := range map[string]string{
		"up":    cli + " network up " + testID + " | Atenea One quiere conectar este equipo a una red.",
		"down":  cli + " network down " + testID + " | Atenea One quiere desconectar este equipo de una red.",
		"leave": cli + " network leave " + testID + " --yes | Atenea One quiere sacar este equipo de una red y borrar sus claves.",
	} {
		*calls = (*calls)[:0]
		if _, err := Apply(op, testID, "", ""); err != nil {
			t.Fatalf("%s: %v", op, err)
		}
		if len(*calls) != 1 || (*calls)[0] != want {
			t.Errorf("%s: %v, esperaba %q", op, *calls, want)
		}
	}
	*calls = (*calls)[:0]
	if _, err := Apply("migrate", "", "", "Red piloto"); err != nil {
		t.Fatal(err)
	}
	if want := cli + " network migrate-legacy --yes --name 'Red piloto' | Atenea One quiere importar la instalación heredada como una red."; (*calls)[0] != want {
		t.Errorf("migrate: %q", (*calls)[0])
	}
	for _, bad := range []string{"", "JOIN", "shell", "up; id"} {
		if _, err := Apply(bad, testID, "", ""); err == nil {
			t.Errorf("operación %q aceptada", bad)
		}
	}
	if _, err := Apply("up", "no-es-un-uuid", "", ""); err == nil {
		t.Error("un id inválido no debe elevarse")
	}
}

func TestNetworkSocketIsTheCLISocketPath(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("las redes múltiples no están soportadas en Windows")
	}
	t.Setenv("ATENEA_RUN_DIR", "/tmp/run")
	got, err := NetworkSocket(testID)
	if err != nil || got != "/tmp/run/456789ab/status.sock" {
		t.Fatalf("NetworkSocket = %q, %v", got, err)
	}
	for _, bad := range []string{"", "x", testID + "0", strings.ToUpper(testID)} {
		if _, err := NetworkSocket(bad); err == nil {
			t.Errorf("id %q aceptado", bad)
		}
	}
}

func TestJoinedNetworkLookupAndFiltering(t *testing.T) {
	fakeCLI(t, `echo '[{"network_id":"`+testID+`","name":"casa"},{"network_id":"basura","name":"x"},{"name":"sin id"}]'`)
	n, err := JoinedNetwork(testID)
	if err != nil || n["name"] != "casa" {
		t.Fatalf("JoinedNetwork = %v, %v", n, err)
	}
	if _, err := JoinedNetwork("ffffffff-ffff-7fff-8fff-ffffffffffff"); err == nil || err.Error() != "la red no está unida" {
		t.Errorf("red ausente: %v", err)
	}
	if _, err := JoinedNetwork("nope"); err == nil || err.Error() != "identificador de red no válido" {
		t.Errorf("id inválido: %v", err)
	}
	if got := JoinedNetworks(); len(got) != 1 || got[0]["name"] != "casa" {
		t.Errorf("JoinedNetworks debe filtrar los ids no válidos: %v", got)
	}
	fakeCLI(t, `echo hola`)
	if got := JoinedNetworks(); len(got) != 0 {
		t.Errorf("ante un fallo, vacío: %v", got)
	}
}
