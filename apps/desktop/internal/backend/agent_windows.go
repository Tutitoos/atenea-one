// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

package backend

import (
	"errors"
	"fmt"
	"os"
	"strings"
	"syscall"
	"time"
)

// Acceso que concede la tubería de estado a Users (0x12008b): FILE_GENERIC_READ | FILE_WRITE_DATA. Pedir
// GENERIC_WRITE (lo que haría os.OpenFile) incluye bits que la tubería no concede y daría «acceso denegado»;
// es el mismo acceso que pide `atenea_windows::connect_pipe`.
const pipeAccess = 0x120089 | 0x2

// Banderas de CreateFile que el paquete syscall no exporta: el servidor solo puede identificar al cliente, no
// suplantarlo (SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION), como hace el CLI Rust.
const (
	sqosPresent            = 0x00100000
	securityIdentification = 0x00010000
)

// Ask abre la tubería de estado del agente como un fichero, pide `status` y devuelve la respuesta validada.
// shortcut: no comprueba que el propietario de la tubería sea Administradores (lo hace el CLI Rust con
// GetSecurityInfo); hacerlo exige FFI, vetado en esta app. Solo se muestra lo que responda.
func Ask(socket string, limit int64, timeout time.Duration) (string, error) {
	const prefix = `\\.\pipe\`
	if !strings.HasPrefix(socket, prefix) || len(socket) == len(prefix) || strings.ContainsAny(socket[len(prefix):], `\/`+"\x00") {
		return "", errors.New("se exige \\\\.\\pipe\\<nombre> local")
	}
	name, err := syscall.UTF16PtrFromString(socket)
	if err != nil {
		return "", err
	}
	h, err := syscall.CreateFile(name, pipeAccess, 0, nil, syscall.OPEN_EXISTING,
		sqosPresent|securityIdentification, 0)
	if err != nil {
		return "", fmt.Errorf("%s: %w", socket, err)
	}
	f := os.NewFile(uintptr(h), socket)
	type reply struct {
		text string
		err  error
	}
	done := make(chan reply, 1) // con búfer: la goroutine termina aunque nadie la espere
	go func() {
		if _, err := f.Write([]byte("status\n")); err != nil {
			done <- reply{err: err}
			return
		}
		text, err := readStatus(f, limit)
		done <- reply{text, err}
	}()
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	select {
	case r := <-done:
		f.Close()
		return r.text, r.err
	case <-timer.C:
		// Una lectura síncrona de tubería no admite plazo: se cierra el manejador y la goroutine acaba cuando el
		// agente responda o cierre su lado.
		f.Close()
		return "", fmt.Errorf("%s: tiempo de espera agotado", socket)
	}
}
