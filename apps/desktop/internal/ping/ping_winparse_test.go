// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package ping

import "testing"

// Salidas sintéticas de ping.exe en dos idiomas: el parser no debe depender del texto traducido.
const (
	winEN = "\r\nPinging 10.99.0.2 with 32 bytes of data:\r\n" +
		"Reply from 10.99.0.2: bytes=32 time=12ms TTL=64\r\n" +
		"Reply from 10.99.0.2: bytes=32 time<1ms TTL=64\r\n" +
		"Request timed out.\r\n" +
		"Reply from 10.99.0.2: bytes=32 time=30ms TTL=64\r\n" +
		"Request timed out.\r\n\r\n" +
		"Ping statistics for 10.99.0.2:\r\n    Packets: Sent = 5, Received = 3, Lost = 2 (40% loss),\r\n" +
		"Approximate round trip times in milli-seconds:\r\n    Minimum = 0ms, Maximum = 30ms, Average = 14ms\r\n"
	winES = "\r\nHaciendo ping a 10.99.0.2 con 32 bytes de datos:\r\n" +
		"Respuesta desde 10.99.0.2: bytes=32 tiempo=12ms TTL=64\r\n" +
		"Respuesta desde 10.99.0.2: bytes=32 tiempo<1m TTL=64\r\n" +
		"Tiempo de espera agotado para esta solicitud.\r\n" +
		"Respuesta desde 10.99.0.2: bytes=32 tiempo=30ms TTL=64\r\n" +
		"Tiempo de espera agotado para esta solicitud.\r\n\r\n" +
		"Estadísticas de ping para 10.99.0.2:\r\n    Paquetes: enviados = 5, recibidos = 3, perdidos = 2 (40% perdidos),\r\n" +
		"Tiempos aproximados de ida y vuelta en milisegundos:\r\n    Mínimo = 0ms, Máximo = 30ms, Media = 14ms\r\n"
	winLost = "Pinging 10.99.0.2 with 32 bytes of data:\r\nRequest timed out.\r\nRequest timed out.\r\n" +
		"Request timed out.\r\nRequest timed out.\r\nRequest timed out.\r\n"
)

func TestParseWindowsIsLanguageIndependent(t *testing.T) {
	for name, text := range map[string]string{"inglés": winEN, "español": winES} {
		got, err := ParseWindows(text, 5)
		if err != nil || got.Sent != 5 || got.Received != 3 || got.LossPct != 40 {
			t.Fatalf("%s: %+v, %v", name, got, err)
		}
		// El resumen («Minimum = 0ms…») no cuenta como respuesta; <1 se toma como 0.
		if *got.MinMs != 0 || *got.MaxMs != 30 || *got.AvgMs != 14 {
			t.Errorf("%s: tiempos %v/%v/%v", name, *got.MinMs, *got.AvgMs, *got.MaxMs)
		}
	}
}

func TestParseWindowsAllLostAndEmpty(t *testing.T) {
	got, err := ParseWindows(winLost, 5)
	if err != nil || got.Received != 0 || got.LossPct != 100 || got.AvgMs != nil {
		t.Fatalf("perdidos: %+v, %v", got, err)
	}
	if _, err := ParseWindows("  \r\n", 5); err == nil {
		t.Error("una salida vacía no es 100 % de pérdida")
	}
	if _, err := ParseWindows(winEN, 2); err == nil {
		t.Error("más respuestas que envíos debía rechazarse")
	}
}
