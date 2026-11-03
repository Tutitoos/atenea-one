// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package panel: el panel usa la raíz y parámetros de consulta, no rutas por dispositivo.
package panel

import (
	"errors"
	"net/url"
	"os/exec"
	"runtime"
	"strings"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/backend"
)

// URL construye https://<host>/?net=<red>&view=Dispositivos. device, si no está vacío, solo se valida.
func URL(base, network, device string) (string, error) {
	if !backend.ValidUUID(network) || (device != "" && !backend.ValidUUID(device)) {
		return "", errors.New("identificador no válido")
	}
	u, err := url.Parse(base)
	if err != nil {
		return "", errors.New("URL del servidor no válida")
	}
	pass, hasPass := "", false
	user := ""
	if u.User != nil {
		user = u.User.Username()
		pass, hasPass = u.User.Password()
	}
	if u.Scheme != "https" || u.Hostname() == "" || user != "" || hasPass || pass != "" {
		return "", errors.New("el panel necesita una URL HTTPS sin credenciales")
	}
	host := strings.TrimSuffix(strings.ToLower(u.Host), ":443")
	q := url.Values{"net": {network}, "view": {"Dispositivos"}}
	return "https://" + host + "/?" + q.Encode(), nil
}

// Launch ejecuta el programa con un único argumento; sin shell. Variable para las pruebas.
var Launch = func(program, arg string) error {
	return exec.Command(program, arg).Run()
}

func program(goos string) (string, error) {
	switch goos {
	case "windows":
		return "", errors.New("no soportado en Windows")
	case "darwin":
		return "/usr/bin/open", nil
	}
	return "xdg-open", nil
}

// Open abre la URL en el navegador del sistema.
func Open(rawURL string) error { return open(runtime.GOOS, rawURL) }

func open(goos, rawURL string) error {
	p, err := program(goos)
	if err != nil {
		return err
	}
	if err := Launch(p, rawURL); err != nil {
		var exit *exec.ExitError
		if errors.As(err, &exit) {
			return errors.New("no se pudo abrir el panel")
		}
		return err
	}
	return nil
}
