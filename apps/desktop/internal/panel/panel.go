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

// Launch ejecuta el programa con sus argumentos; sin shell. Variable para las pruebas.
var Launch = func(program string, args ...string) error {
	return exec.Command(program, args...).Run()
}

// command devuelve el programa y los argumentos que abren la URL, siempre como un único argumento aparte. En
// Windows no se usa `cmd /c start` (interpretaría `&` y `^` de la URL): rundll32 recibe la URL tal cual.
func command(goos, rawURL string) (string, []string) {
	switch goos {
	case "windows":
		return "rundll32.exe", []string{"url.dll,FileProtocolHandler", rawURL}
	case "darwin":
		return "/usr/bin/open", []string{rawURL}
	}
	return "xdg-open", []string{rawURL}
}

// Open abre la URL en el navegador del sistema.
func Open(rawURL string) error { return open(runtime.GOOS, rawURL) }

func open(goos, rawURL string) error {
	p, args := command(goos, rawURL)
	if err := Launch(p, args...); err != nil {
		var exit *exec.ExitError
		if errors.As(err, &exit) {
			return errors.New("no se pudo abrir el panel")
		}
		return err
	}
	return nil
}
