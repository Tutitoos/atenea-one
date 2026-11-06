// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build !windows

package backend

import (
	"fmt"
	"net"
	"time"
)

// Ask conecta con el socket de estado de un agente, pide `status` y devuelve la respuesta validada.
func Ask(socket string, limit int64, timeout time.Duration) (string, error) {
	conn, err := net.DialTimeout("unix", socket, timeout)
	if err != nil {
		return "", fmt.Errorf("%s: %w", socket, err)
	}
	defer conn.Close()
	if err := conn.SetDeadline(time.Now().Add(timeout)); err != nil {
		return "", err
	}
	if _, err := conn.Write([]byte("status\n")); err != nil {
		return "", err
	}
	return readStatus(conn, limit)
}
