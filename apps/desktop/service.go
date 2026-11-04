// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/app"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/backend"
)

// AteneaService es el servicio vinculado de Wails y la ÚNICA superficie que la página puede invocar (ADR-019 §2 y
// decisión 10): todo método exportado de un servicio registrado es invocable desde JavaScript, así que este tipo
// tiene exactamente estos doce métodos (el duodécimo, ClientStatus, es de solo lectura: ADR-021) y la lógica vive en
// `internal/app`, sin vincular. Añadir un método exportado es un cambio de seguridad: rompe TestServiceSurface y
// exige revisión.
type AteneaService struct {
	core *app.App
	// onCall es la salida de la prueba de humo (ATENEA_SMOKE=1); en producción es nil.
	onCall func(method string)
}

// seen avisa a la prueba de humo de que la página llamó a un método. No es exportado: no forma parte de la superficie.
func (s *AteneaService) seen(method string) {
	if s.onCall != nil {
		s.onCall(method)
	}
}

// Status es el estado y la salud de una red unida.
func (s *AteneaService) Status(network string) (string, error) {
	s.seen("Status")
	return s.core.Status(network)
}

// NetworkList son las redes unidas.
func (s *AteneaService) NetworkList() (string, error) {
	s.seen("NetworkList")
	return s.core.NetworkList()
}

// NetworkLegacy informa de si hay una instalación heredada pendiente de importar.
func (s *AteneaService) NetworkLegacy() (string, error) {
	s.seen("NetworkLegacy")
	return s.core.NetworkLegacy()
}

// ClientStatus es el sistema, si la aplicación puede gestionar redes en él y si el CLI está instalado (solo lectura).
func (s *AteneaService) ClientStatus() backend.ClientInfo {
	s.seen("ClientStatus")
	return s.core.ClientStatus()
}

// NetworkInspect decodifica un código de red sin tocar disco ni red.
func (s *AteneaService) NetworkInspect(code string) (string, error) {
	s.seen("NetworkInspect")
	return s.core.NetworkInspect(code)
}

// NetworkApply une, conecta, desconecta, saca de una red o importa la instalación heredada, con la autorización del sistema.
func (s *AteneaService) NetworkApply(op, network, code, name string) (string, error) {
	s.seen("NetworkApply")
	return s.core.NetworkApply(op, network, code, name)
}

// DevicePing prueba la conexión con un par de una red unida.
func (s *AteneaService) DevicePing(network, address string) (string, error) {
	s.seen("DevicePing")
	return s.core.DevicePing(network, address)
}

// OpenPanel abre el panel web del servidor de una red unida.
func (s *AteneaService) OpenPanel(network, deviceID string) error {
	s.seen("OpenPanel")
	return s.core.OpenPanel(network, deviceID)
}

// NotificationsGet lee la preferencia de avisos del sistema.
func (s *AteneaService) NotificationsGet() (bool, error) {
	s.seen("NotificationsGet")
	return s.core.NotificationsGet()
}

// NotificationsSet guarda la preferencia de avisos del sistema.
func (s *AteneaService) NotificationsSet(on bool) error {
	s.seen("NotificationsSet")
	return s.core.NotificationsSet(on)
}

// AutostartGet dice si la aplicación se abre al iniciar sesión (nil si el sistema no lo admite).
func (s *AteneaService) AutostartGet() (*bool, error) {
	s.seen("AutostartGet")
	return s.core.AutostartGet()
}

// AutostartSet activa o desactiva la apertura al iniciar sesión.
func (s *AteneaService) AutostartSet(on bool) error {
	s.seen("AutostartSet")
	return s.core.AutostartSet(on)
}
