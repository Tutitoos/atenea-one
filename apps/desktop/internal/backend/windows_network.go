// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"encoding/json"
	"strings"
	"time"
)

// WindowsStatusPipe es la tubería de estado del agente de Windows (`--status-gid 545`): legible sin elevar.
const WindowsStatusPipe = `\\.\pipe\atenea-status`

// WindowsNetworkID es el id fijo de la única red de Windows (la heredada). Tiene forma de uuid para que pase
// ValidUUID, pero no es el de ninguna red real: el estado de Windows no lo publica.
const WindowsNetworkID = "00000000-0000-4000-8000-000000000001"

const windowsAskTimeout = 2 * time.Second

// windowsAgentResponds es variable para las pruebas (en macOS no hay tubería a la que preguntar).
var windowsAgentResponds = func() bool {
	_, err := Ask(WindowsStatusPipe, MaxStatusBytes, windowsAskTimeout)
	return err == nil
}

// windowsList representa la red única de Windows a partir del estado del agente. Si no responde, la lista está
// vacía (no es un error: el servicio puede no estar instalado).
func windowsList(ask func(string, int64, time.Duration) (string, error)) string {
	text, err := ask(WindowsStatusPipe, MaxStatusBytes, windowsAskTimeout)
	if err != nil {
		return "[]"
	}
	var st struct {
		Config struct {
			LeaseActive bool    `json:"lease_active"`
			LastError   *string `json:"last_error"`
		} `json:"config"`
		Sync struct {
			URL *string `json:"url"`
		} `json:"sync"`
	}
	if json.Unmarshal([]byte(text), &st) != nil {
		return "[]"
	}
	state, join := "iniciada_sin_configuracion", "pendiente"
	if st.Config.LeaseActive {
		state, join = "conectada", "aprobada"
	}
	net := map[string]any{
		"network_id":    WindowsNetworkID,
		"name":          "Red de este equipo",
		"server":        "",
		"url":           "",
		"interface":     "atenea0",
		"listen_port":   0,
		"join_status":   join,
		"state":         state,
		"status_socket": WindowsStatusPipe,
	}
	if st.Sync.URL != nil {
		net["url"] = *st.Sync.URL // OpenPanel lo valida (solo https, sin credenciales)
		net["server"] = strings.TrimPrefix(strings.TrimPrefix(*st.Sync.URL, "https://"), "http://")
	}
	if !st.Config.LeaseActive && st.Config.LastError != nil && *st.Config.LastError != "" {
		net["detail"] = *st.Config.LastError
	}
	out, _ := json.Marshal([]any{net})
	return string(out)
}
