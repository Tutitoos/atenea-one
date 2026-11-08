// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package panel

import (
	"bytes"
	"encoding/json"
	"testing"
)

const peerID = "0199ab12-3456-7def-8abc-0123456789ab"

// peerIDBytes es peerID como lo emite el agente: lista de 16 bytes.
const peerIDBytes = `[1,153,171,18,52,86,125,239,138,188,1,35,69,103,137,171]`

func status(t *testing.T, peer string) map[string]any {
	t.Helper()
	dec := json.NewDecoder(bytes.NewReader([]byte(`{"peers":[` + peer + `]}`)))
	dec.UseNumber()
	var v map[string]any
	if err := dec.Decode(&v); err != nil {
		t.Fatal(err)
	}
	return v
}

func TestServiceURLUsesThePublishedSchemeAndTheAddressFamily(t *testing.T) {
	cases := []struct {
		name, peer, want string
		port             int
	}{
		{"http IPv4", `{"device_id":"` + peerID + `","addrs":["fd99::2","10.99.0.2"],"services":[{"port":8080,"proto":"tcp","scheme":"http"}]}`, "http://10.99.0.2:8080/", 8080},
		{"https IPv4", `{"device_id":"` + peerID + `","addrs":["10.99.0.2"],"services":[{"port":443,"proto":"tcp","scheme":"https","label":"Panel"}]}`, "https://10.99.0.2:443/", 443},
		{"solo IPv6", `{"device_id":"` + peerID + `","addrs":["fd99::2"],"services":[{"port":3000,"proto":"tcp","scheme":"http"}]}`, "http://[fd99::2]:3000/", 3000},
		{"id como bytes", `{"device_id":` + peerIDBytes + `,"addrs":["10.99.0.2"],"services":[{"port":22,"scheme":"https"}]}`, "https://10.99.0.2:22/", 22},
		{"sin esquema es http", `{"device_id":"` + peerID + `","addrs":["10.99.0.2"],"services":[{"port":80,"proto":"tcp"}]}`, "http://10.99.0.2:80/", 80},
	}
	for _, c := range cases {
		got, err := ServiceURL(status(t, c.peer), peerID, c.port)
		if err != nil || got != c.want {
			t.Errorf("%s: %q, %v (esperaba %q)", c.name, got, err, c.want)
		}
	}
}

func TestServiceURLRejectsEverythingThePeerDidNotPublish(t *testing.T) {
	ok := `{"device_id":"` + peerID + `","addrs":["10.99.0.2"],"services":[{"port":8080,"proto":"tcp","scheme":"http"}]}`
	other := "0199ab12-3456-7def-8abc-0123456789ac"
	cases := []struct {
		name, peer, id string
		port           int
	}{
		{"par desconocido", ok, other, 8080},
		{"id no válido", ok, "no-es-uuid", 8080},
		{"id en mayúsculas", ok, "0199AB12-3456-7DEF-8ABC-0123456789AB", 8080},
		{"puerto no publicado", ok, peerID, 22},
		{"puerto 0", ok, peerID, 0},
		{"puerto fuera de rango", ok, peerID, 65536},
		{"sin servicios", `{"device_id":"` + peerID + `","addrs":["10.99.0.2"]}`, peerID, 8080},
		{"proto no tcp", `{"device_id":"` + peerID + `","addrs":["10.99.0.2"],"services":[{"port":8080,"proto":"udp"}]}`, peerID, 8080},
		{"esquema ajeno", `{"device_id":"` + peerID + `","addrs":["10.99.0.2"],"services":[{"port":8080,"scheme":"file"}]}`, peerID, 8080},
		{"sin direcciones", `{"device_id":"` + peerID + `","addrs":[],"services":[{"port":8080,"scheme":"http"}]}`, peerID, 8080},
		{"dirección de bucle", `{"device_id":"` + peerID + `","addrs":["127.0.0.1","::1","0.0.0.0","224.0.0.1","fe80::1%eth0"],"services":[{"port":8080,"scheme":"http"}]}`, peerID, 8080},
		{"dirección no numérica", `{"device_id":"` + peerID + `","addrs":["ejemplo.test"],"services":[{"port":8080,"scheme":"http"}]}`, peerID, 8080},
		{"id de par malformado", `{"device_id":[1,2,3],"addrs":["10.99.0.2"],"services":[{"port":8080}]}`, peerID, 8080},
	}
	for _, c := range cases {
		if got, err := ServiceURL(status(t, c.peer), c.id, c.port); err == nil {
			t.Errorf("%s: debía rechazarse y dio %q", c.name, got)
		}
	}
	// Con otro par en la lista, los servicios del otro no valen para este.
	two := status(t, `{"device_id":"`+other+`","addrs":["10.99.0.3"],"services":[{"port":22,"scheme":"http"}]},`+ok)
	if got, err := ServiceURL(two, peerID, 22); err == nil {
		t.Errorf("el servicio de otro par no debe valer: %q", got)
	}
}
