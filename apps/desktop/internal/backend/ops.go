// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package backend porta a Go la lógica sin interfaz de apps/desktop (networks.rs y main.rs): validación de
// entradas, construcción de la orden elevada y lectura del socket de estado. Es el spike S4 de ADR-019.
package backend

import (
	"errors"
	"fmt"
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
	for i := range len(cli) {
		c := cli[i]
		if !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || strings.IndexByte("/._-", c) >= 0) {
			return "", errors.New("ruta del CLI no válida")
		}
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

// ElevationCommand devuelve el programa y los argumentos que piden la autorización del sistema. No los
// ejecuta. Windows queda fuera (ADR-017): la elevación allí es otra mecánica.
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
	}
	return "", nil, errors.New("elevación no soportada en este sistema")
}
