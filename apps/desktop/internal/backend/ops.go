// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package backend porta a Go la lógica sin interfaz de apps/desktop (networks.rs y main.rs): validación de
// entradas, construcción de la orden elevada y lectura del socket de estado. Es el spike S4 de ADR-019.
package backend

import (
	"errors"
	"fmt"
	"os"
	"strings"
	"unicode"
)

// Op son las operaciones que cambian el sistema. Cada una exige la autorización del sistema operativo.
type Op int

const (
	OpJoin Op = iota
	OpUp
	OpDown
	OpLeave
	OpMigrate
)

// ParseOp acepta exactamente las cinco operaciones permitidas.
func ParseOp(s string) (Op, error) {
	switch s {
	case "join":
		return OpJoin, nil
	case "up":
		return OpUp, nil
	case "down":
		return OpDown, nil
	case "leave":
		return OpLeave, nil
	case "migrate":
		return OpMigrate, nil
	}
	return 0, errors.New("operación no permitida")
}

// ValidUUID: uuid en minúsculas con la forma canónica 8-4-4-4-12.
func ValidUUID(s string) bool {
	if len(s) != 36 {
		return false
	}
	for i := range len(s) {
		b := s[i]
		switch i {
		case 8, 13, 18, 23:
			if b != '-' {
				return false
			}
		default:
			if !(b >= '0' && b <= '9') && !(b >= 'a' && b <= 'f') {
				return false
			}
		}
	}
	return true
}

// ShortID son los últimos 8 hex del uuid: nombra el directorio de ejecución de la red, igual que el CLI.
func ShortID(uuid string) string { return uuid[28:] }

// NormalizeCode normaliza un código pegado (espacios, saltos de línea, minúsculas) y exige la forma del
// formato: prefijo ATENEA1-, alfabeto base32 A–Z2–7 y guiones, máximo 512 bytes.
func NormalizeCode(raw string) (string, error) {
	var b strings.Builder
	for _, r := range raw {
		if unicode.IsSpace(r) {
			continue
		}
		if r >= 'a' && r <= 'z' {
			r -= 'a' - 'A'
		}
		b.WriteRune(r)
	}
	code := b.String()
	body, ok := strings.CutPrefix(code, "ATENEA1-")
	if !ok {
		return "", errors.New("el código debe empezar por ATENEA1-")
	}
	if len(code) > 512 {
		return "", errors.New("el código es demasiado largo")
	}
	if body == "" {
		return "", errors.New("el código tiene caracteres que no son de un código de red")
	}
	for i := range len(body) {
		c := body[i]
		if c != '-' && !(c >= 'A' && c <= 'Z') && !(c >= '2' && c <= '7') {
			return "", errors.New("el código tiene caracteres que no son de un código de red")
		}
	}
	return code, nil
}

// validName: nombre opcional de la red importada, 1–64 caracteres de [A-Za-z0-9._ -], sin espacios en los
// extremos ni guion inicial. Va entre comillas simples en la orden elevada y este alfabeto no puede cerrarlas.
func validName(name string) (string, bool, error) {
	if name == "" {
		return "", false, nil
	}
	ok := len(name) <= 64 && name == strings.TrimSpace(name) && !strings.HasPrefix(name, "-")
	for i := 0; ok && i < len(name); i++ {
		c := name[i]
		ok = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || strings.IndexByte("._ -", c) >= 0
	}
	if !ok {
		return "", false, errors.New("nombre no válido: hasta 64 caracteres de letras, números, espacio, punto, guion y guion bajo")
	}
	return name, true, nil
}

// Script es la orden de shell que se ejecuta con privilegios. Todos los fragmentos variables ya están
// validados (ValidUUID, NormalizeCode, un número), así que no contienen comillas ni metacaracteres.
func Script(cli string, op Op, network, code, name string, gid uint32) (string, error) {
	if err := validCLI(cli); err != nil {
		return "", err
	}
	net := func() (string, error) {
		if ValidUUID(network) {
			return network, nil
		}
		return "", errors.New("identificador de red no válido")
	}
	switch op {
	case OpJoin:
		if code == "" {
			return "", errors.New("falta el código")
		}
		c, err := NormalizeCode(code)
		if err != nil {
			return "", err
		}
		id, err := net()
		if err != nil {
			return "", err
		}
		return fmt.Sprintf("%s network join %s --yes --status-gid %d && %s network up %s", cli, c, gid, cli, id), nil
	case OpUp:
		id, err := net()
		if err != nil {
			return "", err
		}
		return fmt.Sprintf("%s network up %s", cli, id), nil
	case OpDown:
		id, err := net()
		if err != nil {
			return "", err
		}
		return fmt.Sprintf("%s network down %s", cli, id), nil
	case OpLeave:
		id, err := net()
		if err != nil {
			return "", err
		}
		return fmt.Sprintf("%s network leave %s --yes", cli, id), nil
	case OpMigrate:
		n, has, err := validName(name)
		if err != nil {
			return "", err
		}
		if has {
			return fmt.Sprintf("%s network migrate-legacy --yes --name '%s'", cli, n), nil
		}
		return fmt.Sprintf("%s network migrate-legacy --yes", cli), nil
	}
	return "", errors.New("operación no permitida")
}

func validCLI(cli string) error {
	for i := range len(cli) {
		c := cli[i]
		if !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || strings.IndexByte("/._-", c) >= 0) {
			return errors.New("ruta del CLI no válida")
		}
	}
	return nil
}

// ServicesPublishScript es EXACTAMENTE `<cli> services publish on|off --network <uuid>` (ADR-024, fase 5): lista
// cerrada, sin argumentos libres.
func ServicesPublishScript(cli, network string, on bool) (string, error) {
	if err := validCLI(cli); err != nil {
		return "", err
	}
	if !ValidUUID(network) {
		return "", errors.New("identificador de red no válido")
	}
	state := "off"
	if on {
		state = "on"
	}
	return fmt.Sprintf("%s services publish %s --network %s", cli, state, network), nil
}

// ElevationCommand devuelve el programa y los argumentos que piden la autorización del sistema. No los
// ejecuta. En Windows el aviso no se usa: la ventana de UAC la pone el sistema y nombra el programa, no un texto.
func ElevationCommand(goos, script, prompt string) (string, []string, error) {
	switch goos {
	case "darwin":
		if strings.ContainsAny(script, "\"\\") || strings.ContainsAny(prompt, "\"\\") {
			return "", nil, errors.New("la orden o el aviso contienen comillas o barras invertidas")
		}
		apple := fmt.Sprintf("do shell script \"%s\" with administrator privileges with prompt \"%s\"", script, prompt)
		return "/usr/bin/osascript", []string{"-e", apple}, nil
	case "linux":
		return "/usr/bin/pkexec", []string{"/bin/sh", "-c", script}, nil
	case "windows":
		return windowsElevation(script)
	}
	return "", nil, errors.New("elevación no soportada en este sistema")
}

// windowsElevation convierte una orden de WindowsScript (`"<exe>" <argumentos>`) en `powershell.exe` que lanza el CLI con
// `[Diagnostics.Process]::Start` y `Verb = runas` (sin FFI, ADR-017 §8). El proceso elevado corre oculto y se espera;
// PowerShell sale con su código de salida, y con 126 si el usuario cierra el diálogo de UAC (error 1223), que es lo que
// runCtx llama «Cancelado».
//
// No se usa `Start-Process -Verb RunAs`: en Windows PowerShell 5.1 su excepción es una `InvalidOperationException` sin
// excepción interna, así que el error 1223 no se puede reconocer (medido en un Windows real el 2026-11-28; la cancelación
// salía como un error genérico con el texto de PowerShell). `Process.Start` lanza `Win32Exception` con su
// `NativeErrorCode`, y además conserva el código de salida (con `Start-Process -PassThru` puede llegar como `$null`).
func windowsElevation(script string) (string, []string, error) {
	exe, args, ok := splitWindowsScript(script)
	if !ok {
		return "", nil, errors.New("orden elevada no válida")
	}
	root := os.Getenv("SystemRoot")
	if root == "" {
		root = `C:\Windows`
	}
	ps := fmt.Sprintf(`$ErrorActionPreference = 'Stop'; try { $i = New-Object Diagnostics.ProcessStartInfo; $i.FileName = '%s'; $i.Arguments = '%s'; $i.UseShellExecute = $true; $i.Verb = 'runas'; $i.WindowStyle = 'Hidden'; $p = [Diagnostics.Process]::Start($i); if (-not $p) { [Console]::Error.WriteLine('no se pudo iniciar el proceso'); exit 1 }; $p.WaitForExit(); exit $p.ExitCode } catch { $e = $_.Exception; while ($e.InnerException) { $e = $e.InnerException }; if ($e -is [ComponentModel.Win32Exception] -and $e.NativeErrorCode -eq 1223) { exit 126 }; [Console]::Error.WriteLine($e.Message); exit 1 }`, exe, args)
	return root + `\System32\WindowsPowerShell\v1.0\powershell.exe`,
		[]string{"-NoProfile", "-NonInteractive", "-Command", ps}, nil
}

// splitWindowsScript separa `"<exe>" <resto>` y vuelve a validar los dos trozos: el alfabeto de ambos no contiene
// comillas simples, que son lo único que cerraría el texto entre comillas simples de PowerShell.
func splitWindowsScript(script string) (exe, args string, ok bool) {
	rest, found := strings.CutPrefix(script, `"`)
	if !found {
		return "", "", false
	}
	exe, args, found = strings.Cut(rest, `" `)
	if !found || validWindowsCLI(exe) != nil || !validWindowsArgs(args) {
		return "", "", false
	}
	return exe, args, true
}

// validWindowsCLI: ruta absoluta de disco local a un `.exe`, sin comillas ni metacaracteres.
func validWindowsCLI(cli string) error {
	if len(cli) < 4 || !strings.HasSuffix(strings.ToLower(cli), ".exe") || cli[1] != ':' || cli[2] != '\\' {
		return errors.New("ruta del CLI no válida")
	}
	if c := cli[0]; !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')) {
		return errors.New("ruta del CLI no válida")
	}
	for i := 3; i < len(cli); i++ {
		c := cli[i]
		if !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || strings.IndexByte(`\._- ()`, c) >= 0) {
			return errors.New("ruta del CLI no válida")
		}
	}
	if strings.Contains(cli, `..`) {
		return errors.New("ruta del CLI no válida")
	}
	return nil
}

// validWindowsArgs: palabras de [A-Za-z0-9._-] separadas por un espacio; el código (mayúsculas, 2-7 y guiones) y los
// uuid caben en él. Nada puede cerrar una cita ni encadenar órdenes.
func validWindowsArgs(args string) bool {
	if args == "" || strings.HasPrefix(args, " ") || strings.HasSuffix(args, " ") || strings.Contains(args, "  ") {
		return false
	}
	for i := range len(args) {
		c := args[i]
		if !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == ' ' || c == '.' || c == '_' || c == '-') {
			return false
		}
	}
	return true
}

// WindowsScript es la orden elevada de Windows: `"<cli>" network <op> …  --result <nonce>`. A diferencia de Unix no
// encadena `join && up` (no hay shell): usa `join --up`. `migrate` no existe en Windows (ADR-017 §10). Los fragmentos
// variables ya están validados (ValidUUID, NormalizeCode, ValidNonce).
func WindowsScript(cli string, op Op, network, code, nonce string) (string, error) {
	if err := validWindowsCLI(cli); err != nil {
		return "", err
	}
	if !ValidNonce(nonce) {
		return "", errors.New("identificador de resultado no válido")
	}
	net := func() (string, error) {
		if ValidUUID(network) {
			return network, nil
		}
		return "", errors.New("identificador de red no válido")
	}
	var args string
	switch op {
	case OpJoin:
		if code == "" {
			return "", errors.New("falta el código")
		}
		c, err := NormalizeCode(code)
		if err != nil {
			return "", err
		}
		if _, err := net(); err != nil {
			return "", err
		}
		args = "network join " + c + " --yes --up"
	case OpUp, OpDown, OpLeave:
		id, err := net()
		if err != nil {
			return "", err
		}
		args = "network " + map[Op]string{OpUp: "up", OpDown: "down", OpLeave: "leave"}[op] + " " + id
		if op == OpLeave {
			args += " --yes"
		}
	default:
		return "", errors.New("Windows no migra la instalación heredada: desinstálala, reinstala y únete con un código")
	}
	return fmt.Sprintf(`"%s" %s --result %s`, cli, args, nonce), nil
}
