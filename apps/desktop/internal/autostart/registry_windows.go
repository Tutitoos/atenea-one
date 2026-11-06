// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build windows

package autostart

import (
	"errors"

	"golang.org/x/sys/windows/registry"
)

// runKeyPath es la clave `Run` del propio usuario (HKCU): sin privilegios y sin servicio residente. Es una variable
// para que las pruebas usen una clave temporal en lugar de la real.
var runKeyPath = `Software\Microsoft\Windows\CurrentVersion\Run`

// valueName es el nombre del valor bajo `Run`; su existencia es el estado, como el fichero de arranque en macOS y Linux.
const valueName = "Atenea One"

func windowsGet() (bool, error) {
	k, err := registry.OpenKey(registry.CURRENT_USER, runKeyPath, registry.QUERY_VALUE)
	if errors.Is(err, registry.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	defer k.Close()
	if _, _, err := k.GetStringValue(valueName); errors.Is(err, registry.ErrNotExist) {
		return false, nil
	} else if err != nil {
		return false, err
	}
	return true, nil
}

// windowsSet escribe (on) o borra (!on) el valor. Borrar lo que no existe no es un error.
func windowsSet(command string, on bool) error {
	if !on {
		k, err := registry.OpenKey(registry.CURRENT_USER, runKeyPath, registry.SET_VALUE)
		if errors.Is(err, registry.ErrNotExist) {
			return nil
		}
		if err != nil {
			return err
		}
		defer k.Close()
		if err := k.DeleteValue(valueName); err != nil && !errors.Is(err, registry.ErrNotExist) {
			return err
		}
		return nil
	}
	k, _, err := registry.CreateKey(registry.CURRENT_USER, runKeyPath, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()
	return k.SetStringValue(valueName, command)
}
