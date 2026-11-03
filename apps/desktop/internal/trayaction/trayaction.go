// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package trayaction contiene las acciones de bandeja: etiquetas puras y
// portapapeles sin plugins.
package trayaction

import (
	"errors"
	"net/netip"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
)

// Entry es una entrada del menú de la bandeja.
type Entry struct{ ID, Text string }

// LookPath y Exec son inyectables en las pruebas.
var (
	LookPath = exec.LookPath
	Exec     = func(program string, args []string, stdin string) error {
		cmd := exec.Command(program, args...)
		cmd.Stdin = strings.NewReader(stdin)
		return cmd.Run()
	}
)

func str(m map[string]any, key string) (string, bool) {
	s, ok := m[key].(string)
	return s, ok
}

// Entries devuelve las entradas de bandeja de una red.
func Entries(net, status map[string]any, clipboard bool) []Entry {
	id, _ := str(net, "network_id")
	name, ok := str(net, "name")
	if !ok {
		name = "Red"
	}
	state, ok := str(net, "state")
	up := !ok || state == "detenida" || state == "desconocida"
	op, text := "down", "Desconectar "
	if up {
		op, text = "up", "Conectar "
	}
	entries := []Entry{{ID: op + ":" + id, Text: text + name}}
	if clipboard {
		addrs, _ := status["addrs"].([]any)
		for _, a := range addrs {
			s, _ := a.(string)
			if ip, err := netip.ParseAddr(s); err == nil && ip.Is4() {
				entries = append(entries, Entry{ID: "copy:" + ip.String(), Text: "Copiar mi IP · " + name})
				break
			}
		}
	}
	return entries
}

// Split separa "op:arg" por el primer ':'.
func Split(id string) (op, arg string, ok bool) {
	return strings.Cut(id, ":")
}

// ClipboardProgram devuelve la ruta del programa de portapapeles o "".
func ClipboardProgram(goos string, lookPath func(file string) (string, error)) string {
	var names []string
	switch goos {
	case "darwin":
		names = []string{"pbcopy"}
	case "linux":
		names = []string{"wl-copy", "xclip"}
	}
	for _, name := range names {
		if path, err := lookPath(name); err == nil {
			return path
		}
	}
	return ""
}

// Available indica si hay portapapeles disponible.
func Available() bool { return ClipboardProgram(runtime.GOOS, LookPath) != "" }

// Copy copia una IPv4 al portapapeles.
func Copy(address string) error {
	ip, err := netip.ParseAddr(address)
	if err != nil || !ip.Is4() {
		return errors.New("IP no válida")
	}
	program := ClipboardProgram(runtime.GOOS, LookPath)
	if program == "" {
		return errors.New("portapapeles no disponible")
	}
	var args []string
	if filepath.Base(program) == "xclip" {
		args = []string{"-selection", "clipboard"}
	}
	if Exec(program, args, ip.String()) != nil {
		return errors.New("no se pudo copiar la IP")
	}
	return nil
}
