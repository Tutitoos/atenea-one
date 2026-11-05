// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os/exec"
	"strings"
	"time"
)

// MaxStatusBytes es el tope de la respuesta del agente (ADR-011 §2).
const MaxStatusBytes = 4 << 20

// Ask conecta con el socket de estado de un agente, pide `status` y devuelve la respuesta validada. Igual que
// el cliente Rust: lee a lo sumo `limit` bytes y solo la primera línea, así que una respuesta mayor queda
// truncada y falla por JSON inválido.
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
	line, err := bufio.NewReader(io.LimitReader(conn, limit)).ReadString('\n')
	if err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	return Checked(line)
}

// Checked rechaza una respuesta vacía, inválida o de error del agente.
func Checked(line string) (string, error) {
	var v any
	if err := json.Unmarshal([]byte(line), &v); err != nil {
		return "", errors.New("respuesta vacía o JSON inválido")
	}
	if obj, ok := v.(map[string]any); ok {
		if e, has := obj["error"]; has {
			if s, ok := e.(string); ok {
				return "", fmt.Errorf("agente: %s", s)
			}
			return "", errors.New("agente: orden rechazada")
		}
	}
	return strings.TrimSpace(line), nil
}

// ReadTimeout acota las lecturas del CLI (`list`, `legacy`, `inspect`): un CLI colgado devuelve error en vez de
// bloquear la interfaz. Es una variable para que las pruebas la acorten.
var ReadTimeout = 5 * time.Second

// Run ejecuta una lectura del CLI con ReadTimeout. Para operaciones con diálogo de administrador usa RunElevated.
func Run(program string, args ...string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), ReadTimeout)
	defer cancel()
	return runCtx(ctx, program, args...)
}

// RunElevated ejecuta un programa sin plazo: el diálogo de administrador tarda lo que tarde el usuario.
func RunElevated(program string, args ...string) (string, error) {
	return runCtx(context.Background(), program, args...)
}

// runCtx ejecuta un programa sin shell y con la entrada vacía. El usuario que cierra el diálogo de
// administrador devuelve «Cancelado»: en macOS `osascript` falla con el error (-128), cuyo texto sale traducido
// («User canceled», «Cancelado por el usuario»…), así que se reconoce por el código y no por el texto; polkit usa 126/127.
func runCtx(ctx context.Context, program string, args ...string) (string, error) {
	cmd := exec.CommandContext(ctx, program, args...)
	cmd.WaitDelay = time.Second // un nieto que conserve las tuberías no puede retener la espera
	var stdout, stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	err := cmd.Run()
	if err != nil && ctx.Err() != nil {
		return "", fmt.Errorf("%s: tiempo de espera agotado", program)
	}
	out := strings.TrimSpace(stdout.String())
	if err == nil {
		return out, nil
	}
	errText := strings.TrimSpace(stderr.String())
	var exit *exec.ExitError
	if errors.As(err, &exit) {
		code := exit.ExitCode()
		if strings.Contains(errText, "(-128)") || code == 126 || code == 127 {
			return "", errors.New("Cancelado: no se ha cambiado nada")
		}
		if errText == "" {
			return "", errors.New(out)
		}
		return "", errors.New(errText)
	}
	return "", fmt.Errorf("%s: %w", program, err)
}
