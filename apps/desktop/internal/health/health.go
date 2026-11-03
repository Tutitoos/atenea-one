// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package health evalúa el estado global de una red a partir del JSON de estado del agente.
package health

import (
	"encoding/json"
	"fmt"
	"math"
	"sync"
	"time"
)

// Health es la salud ordenada de peor a mejor ascendente: el máximo es el peor estado.
type Health int

const (
	Ok Health = iota
	Degraded
	Down
	Stopped
)

// Assessment es el estado global y su resumen.
type Assessment struct {
	Health  Health
	Text    string
	Reasons []string
}

// Level devuelve el nivel que entiende el frontend.
func (a Assessment) Level() string {
	switch a.Health {
	case Ok:
		return "ok"
	case Degraded:
		return "degraded"
	default:
		return "down"
	}
}

// leaseGraceSecs son los segundos que se tolera un lease perdido antes de darlo por caída. Tras suspender
// el equipo el agente lo caduca y tarda en renovarlo (sincroniza cada ~60 s y la red tarda en volver).
const leaseGraceSecs int64 = 90

var (
	clock     = func() int64 { return time.Now().Unix() }
	lostMu    sync.Mutex
	lostSince = map[string]int64{}
)

// Assess evalúa el estado con el reloj real y recuerda, por clave pública, cuándo se vio el lease perdido.
func Assess(status map[string]any) Assessment {
	now := clock()
	key, _ := status["public_key"].(string)
	var since *int64
	lostMu.Lock()
	if b, ok := field(status, "config", "lease_active").(bool); ok && b {
		delete(lostSince, key)
	} else {
		t, ok := lostSince[key]
		if !ok {
			t = now
			lostSince[key] = t
		}
		since = &t
	}
	lostMu.Unlock()
	return AssessAt(status, now, since)
}

// field recorre claves anidadas; lo que falte o no sea objeto es nil (como indexar un Value en Rust).
func field(v any, keys ...string) any {
	for _, k := range keys {
		m, ok := v.(map[string]any)
		if !ok {
			return nil
		}
		v = m[k]
	}
	return v
}

// asInt64 equivale a Value::as_i64: solo enteros que caben en i64.
func asInt64(v any) (int64, bool) {
	switch n := v.(type) {
	case int:
		return int64(n), true
	case int64:
		return n, true
	case json.Number:
		i, err := n.Int64()
		return i, err == nil
	case float64:
		if n == math.Trunc(n) && n >= -(1<<63) && n < 1<<63 {
			return int64(n), true
		}
	}
	return 0, false
}

// AssessAt es Assess con el instante y el inicio del lease perdido inyectados.
func AssessAt(status map[string]any, now int64, leaseLostSince *int64) Assessment {
	health := Ok
	why := []string{}
	worsen := func(h Health, text string) {
		health = max(health, h)
		if text != "" {
			why = append(why, text)
		}
	}
	leaseLost := field(status, "config", "lease_active") != true
	reconnecting := leaseLost && leaseLostSince != nil && now-*leaseLostSince < leaseGraceSecs
	if reconnecting {
		worsen(Degraded, "")
	} else if leaseLost {
		worsen(Down, "sin lease activo")
	}
	if e, ok := field(status, "config", "last_error").(string); ok {
		// «expired» es la propia caducidad del lease: se explica con «Reconectando…», no con un error.
		if !(reconnecting && e == "expired") {
			worsen(Down, "configuración: "+e)
		}
	}
	if e, ok := field(status, "sync", "last_error").(string); ok {
		worsen(Degraded, "control plane: "+e)
	}
	if e, ok := field(status, "coordinator", "last_error").(string); ok {
		worsen(Degraded, "coordinator: "+e)
	}
	if expiry, ok := asInt64(field(status, "server", "issuer_cert_not_after_unix")); ok {
		left := expiry - now
		if now > 0 && left > expiry { // desbordamiento por abajo: satura
			left = math.MinInt64
		} else if now < 0 && left < expiry { // por arriba
			left = math.MaxInt64
		}
		if left <= 0 {
			worsen(Down, "El certificado de la red ha caducado")
		} else if left <= 14*86400 {
			worsen(Degraded, fmt.Sprintf("El certificado de la red caduca en %d días", (left+86399)/86400))
		}
	}
	peers, _ := status["peers"].([]any)
	for _, p := range peers {
		if path, _ := field(p, "path").(string); path == "relay-udp" || path == "relay-tls" {
			worsen(Degraded, "")
			break
		}
	}
	var text string
	switch {
	case health == Ok && len(peers) == 0:
		text = "Conectado (sin otros dispositivos)"
	case health == Ok:
		text = "Conectado"
	case health == Degraded && reconnecting:
		text = "Reconectando…"
	case health == Degraded:
		text = "Degradado: " + first(why, "algún dispositivo por relay")
	default:
		text = "Sin conexión: " + first(why, "sin detalle")
	}
	return Assessment{Health: health, Text: text, Reasons: why}
}

func first(s []string, def string) string {
	if len(s) == 0 {
		return def
	}
	return s[0]
}

// Source es lo que responde cada red unida.
type Source struct {
	Label  string
	Health Health
	Text   string
}

// Line es una línea del menú de la bandeja.
type Line struct{ ID, Text string }

// Summarize da icono, tooltip y líneas del menú a partir de las redes que responden (joined = unidas). Una
// red detenida a propósito no responde y no cuenta: no pone rojo el icono de las que sí están conectadas.
func Summarize(joined int, sources []Source) (Health, string, []Line) {
	switch len(sources) {
	case 0:
		text := "Redes paradas o sin acceso al estado"
		if joined == 0 {
			text = "Sin redes"
		}
		return Stopped, text, []Line{{"*", text}}
	case 1:
		return sources[0].Health, sources[0].Text, []Line{{"*", sources[0].Text}}
	}
	worst := sources[0]
	lines := make([]Line, 0, len(sources))
	for _, s := range sources {
		if s.Health >= worst.Health { // max_by_key de Rust devuelve el último máximo
			worst = s
		}
		lines = append(lines, Line{s.Label, s.Label + ": " + s.Text})
	}
	return worst.Health, fmt.Sprintf("%d redes · %s", len(sources), worst.Text), lines
}
