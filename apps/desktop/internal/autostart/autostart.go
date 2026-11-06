// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package autostart implementa «Abrir al iniciar sesión»: el estado es la existencia de un arranque del propio
// usuario (LaunchAgent en macOS, .desktop XDG en Linux, valor bajo la clave `Run` de HKCU en Windows). Sin
// privilegios, sin servicio residente y sin preferencia aparte que pueda desincronizarse.
package autostart

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"unicode"
	"unicode/utf8"
)

const label = "one.atenea.desktop"

// entryPath es el fichero de arranque del usuario; "" donde no hay mecanismo soportado.
func entryPath(goos, home, xdgConfig string) string {
	switch goos {
	case "darwin":
		return filepath.Join(home, "Library/LaunchAgents/"+label+".plist")
	case "linux":
		base := filepath.Join(home, ".config")
		if xdgConfig != "" && filepath.IsAbs(xdgConfig) {
			base = xdgConfig
		}
		return filepath.Join(base, "autostart/"+label+".desktop")
	}
	return ""
}

// Get devuelve nil donde el sistema operativo no lo soporta.
func Get(home, xdgConfig string) (*bool, error) { return getFor(runtime.GOOS, home, xdgConfig) }

func getFor(goos, home, xdgConfig string) (*bool, error) {
	if goos == "windows" {
		on, err := windowsGet()
		if err != nil {
			return nil, err
		}
		return &on, nil
	}
	path := entryPath(goos, home, xdgConfig)
	if path == "" {
		return nil, nil
	}
	_, err := os.Lstat(path)
	on := err == nil
	if err != nil && !errors.Is(err, fs.ErrNotExist) {
		return nil, err
	}
	return &on, nil
}

func Set(home, xdgConfig, exe string, on bool) error {
	return setFor(runtime.GOOS, home, xdgConfig, exe, on)
}

func setFor(goos, home, xdgConfig, exe string, on bool) error {
	if goos == "windows" {
		if !on {
			return windowsSet("", false) // desactivar no depende de la ruta del ejecutable
		}
		command, err := windowsCommand(exe)
		if err != nil {
			return err
		}
		return windowsSet(command, true)
	}
	path := entryPath(goos, home, xdgConfig)
	if path == "" {
		return errors.New("«Abrir al iniciar sesión» no está disponible en este sistema operativo")
	}
	if !on {
		if err := os.Remove(path); err != nil && !errors.Is(err, fs.ErrNotExist) {
			return err
		}
		return nil
	}
	body, err := render(goos, exe)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	// Como Path::with_extension("tmp") del Rust: sustituye la última extensión.
	tmp := strings.TrimSuffix(path, filepath.Ext(path)) + ".tmp"
	if err := os.WriteFile(tmp, []byte(body), 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

// checkedExe exige una ruta absoluta sin caracteres que cambien el sentido de la línea de arranque
// (comillas, \, %, $, comilla invertida, saltos o controles).
func checkedExe(exe string) (string, error) {
	if exe == "" || !utf8.ValidString(exe) || !filepath.IsAbs(exe) {
		return "", errors.New("ruta del ejecutable no válida")
	}
	for _, c := range exe {
		if unicode.IsControl(c) || strings.ContainsRune(`"\%$`+"`"+`<>&'`, c) {
			return "", errors.New("la ruta del ejecutable contiene caracteres no admitidos")
		}
	}
	return exe, nil
}

// windowsDriveAbs es una ruta absoluta con letra de unidad. La comprobación es propia (no `filepath.IsAbs`) para que se
// pueda probar en cualquier sistema.
var windowsDriveAbs = regexp.MustCompile(`^[A-Za-z]:\\`)

// windowsCommand es la línea de arranque: la ruta entre comillas, sin argumentos (la aplicación arranca oculta, en la
// bandeja, salvo la primera vez). Rechaza lo que cambiaría el sentido de la línea: comillas, `%` (se expandiría), `&`,
// `|`, `<`, `>`, `^`, controles y rutas que no sean de unidad con letra (relativas, UNC, de dispositivo).
func windowsCommand(exe string) (string, error) {
	if exe == "" || !utf8.ValidString(exe) || !windowsDriveAbs.MatchString(exe) {
		return "", errors.New("ruta del ejecutable no válida")
	}
	for _, c := range exe {
		if unicode.IsControl(c) || strings.ContainsRune(`"%&|<>^`+"`", c) {
			return "", errors.New("la ruta del ejecutable contiene caracteres no admitidos")
		}
	}
	return `"` + exe + `"`, nil
}

func render(goos, exe string) (string, error) {
	exe, err := checkedExe(exe)
	if err != nil {
		return "", err
	}
	if goos == "darwin" {
		return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n" +
			"<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n" +
			"<plist version=\"1.0\"><dict>\n  <key>Label</key><string>" + label + "</string>\n  " +
			"<key>ProgramArguments</key><array><string>" + exe + "</string></array>\n  " +
			"<key>RunAtLoad</key><true/>\n</dict></plist>\n", nil
	}
	return "[Desktop Entry]\nType=Application\nName=Atenea One\nExec=\"" + exe + "\"\nTerminal=false\n" +
		"X-GNOME-Autostart-enabled=true\n", nil
}
