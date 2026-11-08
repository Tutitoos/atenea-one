// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package panel

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/netip"
	"net/url"
	"strconv"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/backend"
)

// ServiceURL construye la URL de un servicio publicado por un par (ADR-024) a partir del estado de SU red: la interfaz
// solo aporta el id del par y el puerto, nunca una URL. El par debe estar en el estado, el puerto debe figurar en su
// lista `services` (TCP) y la dirección sale de las `addrs` de ese par (el agente solo acepta configuraciones con
// direcciones dentro de los prefijos de la red): IPv4 si la tiene y, si no, IPv6 entre corchetes. El esquema es el
// publicado, solo http o https.
func ServiceURL(status map[string]any, deviceID string, port int) (string, error) {
	if !backend.ValidUUID(deviceID) {
		return "", errors.New("identificador de dispositivo no válido")
	}
	if port < 1 || port > 65535 {
		return "", errors.New("puerto no válido")
	}
	peers, _ := status["peers"].([]any)
	for _, p := range peers {
		peer, _ := p.(map[string]any)
		if id, ok := peerDeviceID(peer["device_id"]); !ok || id != deviceID {
			continue
		}
		scheme, err := publishedScheme(peer["services"], port)
		if err != nil {
			return "", err
		}
		ip, err := peerAddr(peer["addrs"])
		if err != nil {
			return "", err
		}
		u := url.URL{Scheme: scheme, Host: netip.AddrPortFrom(ip, uint16(port)).String(), Path: "/"}
		return u.String(), nil
	}
	return "", errors.New("el dispositivo no está en esta red")
}

// peerDeviceID acepta el id como lo emite el agente: lista de 16 bytes, o UUID canónico en minúsculas.
func peerDeviceID(v any) (string, bool) {
	switch id := v.(type) {
	case string:
		return id, backend.ValidUUID(id)
	case []any:
		if len(id) != 16 {
			return "", false
		}
		b := make([]byte, 16)
		for i, x := range id {
			n, ok := x.(json.Number)
			if !ok {
				return "", false
			}
			k, err := strconv.ParseUint(n.String(), 10, 8)
			if err != nil {
				return "", false
			}
			b[i] = byte(k)
		}
		return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16]), true
	}
	return "", false
}

// publishedScheme devuelve el esquema con el que el par publica ese puerto TCP; sin él, error.
func publishedScheme(v any, port int) (string, error) {
	list, _ := v.([]any)
	for _, s := range list {
		svc, _ := s.(map[string]any)
		n, ok := svc["port"].(json.Number)
		if !ok || n.String() != strconv.Itoa(port) {
			continue
		}
		if proto, has := svc["proto"].(string); has && proto != "tcp" {
			continue
		}
		scheme, has := svc["scheme"].(string)
		if !has {
			scheme = "http" // el agente publica siempre el esquema; http es su valor por defecto
		}
		if scheme != "http" && scheme != "https" {
			return "", errors.New("el par publica un esquema no admitido")
		}
		return scheme, nil
	}
	return "", errors.New("el par no publica ese servicio")
}

// peerAddr elige la IPv4 del par o, si no tiene, su IPv6. Descarta lo que no es una dirección de túnel unicast.
func peerAddr(v any) (netip.Addr, error) {
	list, _ := v.([]any)
	var v6 netip.Addr
	for _, a := range list {
		s, _ := a.(string)
		ip, err := netip.ParseAddr(s)
		if err != nil || ip.Zone() != "" || ip.Is4In6() || ip.IsUnspecified() || ip.IsLoopback() || ip.IsMulticast() || ip.IsLinkLocalUnicast() {
			continue
		}
		if ip.Is4() {
			return ip, nil
		}
		if !v6.IsValid() {
			v6 = ip
		}
	}
	if !v6.IsValid() {
		return netip.Addr{}, errors.New("el par no tiene una dirección válida en esta red")
	}
	return v6, nil
}
