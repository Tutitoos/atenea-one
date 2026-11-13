// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"encoding/json"
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

// trustAnyCLI sustituye la comprobación de confianza del CLI: los CLI falsos viven en un directorio temporal del usuario
// y la real los rechaza (lo prueba TestElevatedOperationsRefuseACLIThatAnUnprivilegedUserCanReplace).
func trustAnyCLI(t *testing.T) {
	t.Helper()
	old := CheckCLI
	CheckCLI = func(string) error { return nil }
	t.Cleanup(func() { CheckCLI = old })
}

// capture sustituye la elevación por un registro de lo que se habría ejecutado.
func capture(t *testing.T) *[]string {
	t.Helper()
	trustAnyCLI(t)
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

func TestJoinDialogNamesTheNetworkOnlyWhenTheNameIsSafeAndTheCommandIsUntouched(t *testing.T) {
	// El «CLI» imprime un JSON generado con json.Marshal desde un fichero: ningún nombre pasa por el shell.
	cli := func(t *testing.T, name string) string {
		b, err := json.Marshal(map[string]string{"network_id": testID, "name": name})
		if err != nil {
			t.Fatal(err)
		}
		path := filepath.Join(t.TempDir(), "info.json")
		if err := os.WriteFile(path, b, 0o600); err != nil {
			t.Fatal(err)
		}
		return "cat '" + path + "'"
	}
	fakeCLI(t, cli(t, "Casa 1.0_a-b"))
	calls := capture(t)
	if _, err := Apply("join", "", "atenea1-abcd-ef23", ""); err != nil {
		t.Fatal(err)
	}
	if !strings.HasSuffix((*calls)[0], " | Atenea One quiere unir este equipo a la red «Casa 1.0_a-b».") {
		t.Errorf("con nombre seguro: %q", (*calls)[0])
	}
	command := strings.SplitN((*calls)[0], " | ", 2)[0]
	cliPath := CLIPath()
	want := cliPath + " network join ATENEA1-ABCD-EF23 --yes --status-gid " + strconv.Itoa(os.Getgid()) + " && " + cliPath + " network up " + testID
	if command != want || strings.Contains(command, "Casa") {
		t.Errorf("el nombre no debe entrar en la orden elevada: %q", command)
	}
	// Nombres que podrían romper la cita del diálogo o que el alfabeto no admite: texto genérico.
	for _, bad := range []string{"a\"b", `a\b`, "ñandú", "a'b", " lead", "trail ", "a;id", "a\nb", "a\u202eb", strings.Repeat("a", 49)} {
		fakeCLI(t, cli(t, bad))
		*calls = nil
		if _, err := Apply("join", "", "atenea1-abcd-ef23", ""); err != nil {
			t.Fatalf("%q: %v", bad, err)
		}
		if !strings.HasSuffix((*calls)[0], " | Atenea One quiere unir este equipo a una red.") {
			t.Errorf("%q debía dar el texto genérico: %q", bad, (*calls)[0])
		}
	}
}

func TestSafePromptNameBoundaries(t *testing.T) {
	for name, want := range map[string]bool{
		"": false, "a": true, strings.Repeat("a", 48): true, strings.Repeat("a", 49): false,
		"a b": true, "a\"b": false, "a\\b": false, "a\nb": false, "a\u202eb": false, "é": false, " a": false, "a ": false,
	} {
		if got := safePromptName(name); got != want {
			t.Errorf("safePromptName(%q) = %v, esperaba %v", name, got, want)
		}
	}
}

func TestClientInfoSaysWhetherTheCLIIsThereWithoutRunningIt(t *testing.T) {
	dir := t.TempDir()
	exe := filepath.Join(dir, "atenea")
	notExe := filepath.Join(dir, "sin-permiso")
	// Un «CLI» que, si se ejecutase, dejaría rastro: la comprobación no debe lanzarlo.
	marker := filepath.Join(dir, "ejecutado")
	if err := os.WriteFile(exe, []byte("#!/bin/sh\ntouch '"+marker+"'\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(notExe, []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name, goos, cli string
		want            ClientInfo
	}{
		{"instalado en linux", "linux", exe, ClientInfo{OS: "linux", Supported: true, CanJoin: true, Installed: true}},
		{"instalado en macOS", "darwin", exe, ClientInfo{OS: "darwin", Supported: true, CanJoin: true, Installed: true}},
		{"ausente", "linux", filepath.Join(dir, "no-existe"), ClientInfo{OS: "linux", Supported: true, CanJoin: true}},
		{"sin permiso de ejecución", "linux", notExe, ClientInfo{OS: "linux", Supported: true, CanJoin: true}},
		{"un directorio no es el CLI", "darwin", dir, ClientInfo{OS: "darwin", Supported: true, CanJoin: true}},
		{"Windows se puede ver pero no unir; el CLI no cuenta", "windows", exe, ClientInfo{OS: "windows", Supported: true}},
		{"otro sistema no está soportado", "freebsd", exe, ClientInfo{OS: "freebsd"}},
	} {
		if runtime.GOOS == "windows" && tc.want.Supported {
			continue // un anfitrión Windows no tiene bits de ejecución: el permiso 0o111 de Unix no se puede probar allí
		}
		if got := clientFor(tc.goos, tc.cli); got != tc.want {
			t.Errorf("%s: %+v, esperaba %+v", tc.name, got, tc.want)
		}
	}
	if _, err := os.Stat(marker); err == nil {
		t.Error("comprobar el cliente no debe ejecutarlo")
	}
}

func TestClientInfoUsesTheFixedCLIPathAndExposesNoPath(t *testing.T) {
	fakeCLI(t, "true")
	got := Client()
	if runtime.GOOS == "darwin" || runtime.GOOS == "linux" {
		if !got.Installed || !got.Supported || got.OS != runtime.GOOS {
			t.Errorf("%+v", got)
		}
	}
	b, err := json.Marshal(got)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(b), "/") || strings.Contains(string(b), "atenea") {
		t.Errorf("ClientInfo no debe llevar rutas: %s", b)
	}
	if string(b) != `{"os":"`+runtime.GOOS+`","supported":`+strconv.FormatBool(got.Supported)+`,"can_join":`+strconv.FormatBool(got.CanJoin)+`,"installed":`+strconv.FormatBool(got.Installed)+`}` {
		t.Errorf("forma JSON inesperada: %s", b)
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

func TestSetServicesPublishRunsTheExactClosedCommand(t *testing.T) {
	fakeCLI(t, `echo '[]'`)
	calls := capture(t)
	if err := SetServicesPublish(testID, true); err != nil {
		t.Fatal(err)
	}
	if err := SetServicesPublish(testID, false); err != nil {
		t.Fatal(err)
	}
	cli := CLIPath()
	want := []string{
		cli + " services publish on --network " + testID + " | Atenea One quiere publicar los servicios de este equipo en una red.",
		cli + " services publish off --network " + testID + " | Atenea One quiere dejar de publicar los servicios de este equipo en una red.",
	}
	if len(*calls) != 2 || (*calls)[0] != want[0] || (*calls)[1] != want[1] {
		t.Fatalf("órdenes elevadas = %q, esperaba %q", *calls, want)
	}
	for _, bad := range []string{"", "x", testID + "0", strings.ToUpper(testID), testID + "; id", "--all"} {
		if err := SetServicesPublish(bad, true); err == nil {
			t.Errorf("%q debía rechazarse", bad)
		}
	}
	if len(*calls) != 2 {
		t.Errorf("una validación fallida no debe elevar nada: %q", *calls)
	}
	if _, err := ServicesPublishScript("/usr/bin/atenea; id", testID, true); err == nil {
		t.Error("una ruta de CLI con metacaracteres debía rechazarse")
	}
}

func TestElevatedOperationsRefuseACLIThatAnUnprivilegedUserCanReplace(t *testing.T) {
	args := fakeCLI(t, `echo '[]'`)
	// Un directorio que cualquier usuario puede escribir, también si esta prueba corre como root.
	if err := os.Chmod(filepath.Dir(CLIPath()), 0o777); err != nil {
		t.Fatal(err)
	}
	var elevated []string
	old := Elevate
	Elevate = func(script, prompt string) (string, error) { elevated = append(elevated, script); return "", nil }
	t.Cleanup(func() { Elevate = old })

	if _, err := Apply("up", testID, "", ""); err == nil || !strings.Contains(err.Error(), "ruta no confiable") {
		t.Errorf("Apply: %v", err)
	}
	if err := SetServicesPublish(testID, true); err == nil || !strings.Contains(err.Error(), "ruta no confiable") {
		t.Errorf("SetServicesPublish: %v", err)
	}
	if len(elevated) != 0 {
		t.Errorf("no debe elevarse nada: %q", elevated)
	}
	// Las lecturas sin privilegios siguen usando ese mismo CLI.
	if got, err := List(); err != nil || got != "[]" {
		t.Errorf("List = %q, %v", got, err)
	}
	if got := readArgs(t, args); got != "network list --json" {
		t.Errorf("argumentos = %q", got)
	}
}
