// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package notify decide los avisos con reloj explícito y los entrega al SO.
package notify

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"os/exec"
	"runtime"
	"strings"
	"time"
)

var errDenied = errors.New("el sistema no permite avisos de Atenea One (Ajustes del Sistema > Notificaciones)")

type peer struct {
	direct     bool
	relaySince *int64
	notified   bool
}

// State guarda las decisiones de aviso por red. El valor cero es usable.
type State struct {
	initialized   bool
	connected     bool
	lost          bool
	peers         map[string]*peer
	certificateAt *int64
}

// asInt64 equivale a Value::as_i64: solo enteros que caben en int64.
func asInt64(v any) (int64, bool) {
	switch n := v.(type) {
	case json.Number:
		i, err := n.Int64()
		return i, err == nil
	case int64:
		return n, true
	case int:
		return int64(n), true
	case float64:
		if n == math.Trunc(n) && n >= -(1<<63) && n < 1<<63 {
			return int64(n), true
		}
	}
	return 0, false
}

func sub(a, b int64) int64 {
	if d := a - b; (a >= b) == (d >= 0) {
		return d
	}
	if a < b {
		return math.MinInt64
	}
	return math.MaxInt64
}

func obj(v any, key string) any {
	m, _ := v.(map[string]any)
	return m[key]
}

// Sample decide los avisos de una muestra; status puede ser nil.
func (s *State) Sample(networkState string, status map[string]any, now int64) []string {
	var st any
	if status != nil {
		st = status
	}
	lease, _ := obj(obj(st, "config"), "lease_active").(bool)
	connected := networkState != "conflicto" && status != nil && lease
	var messages []string
	if s.initialized {
		if s.connected && !connected && networkState != "detenida" {
			s.lost = true
			messages = append(messages, "La red ha perdido la conexión")
		} else if connected && s.lost {
			s.lost = false
			messages = append(messages, "La red vuelve a estar conectada")
		}
	}
	s.connected = connected

	peers, _ := obj(st, "peers").([]any)
	present := map[string]bool{}
	for _, p := range peers {
		if key, ok := obj(p, "public_key").(string); ok {
			present[key] = true
		}
	}
	for key := range s.peers {
		if !present[key] {
			delete(s.peers, key)
		}
	}
	for _, p := range peers {
		key, ok := obj(p, "public_key").(string)
		if !ok {
			continue
		}
		if s.peers == nil {
			s.peers = map[string]*peer{}
		}
		prev := s.peers[key]
		if prev == nil {
			prev = &peer{}
			s.peers[key] = prev
		}
		path, ok := obj(p, "path").(string)
		if !ok {
			path = "none"
		}
		switch {
		case path == "direct":
			*prev = peer{direct: true}
		case strings.HasPrefix(path, "relay-") && prev.direct:
			if prev.relaySince == nil {
				t := now
				prev.relaySince = &t
			}
			if s.initialized && sub(now, *prev.relaySince) >= 60 && !prev.notified {
				prev.notified = true
				name, ok := obj(p, "name").(string)
				if !ok {
					name = "Un dispositivo"
				}
				messages = append(messages, name+" lleva un minuto por relay")
			}
		default:
			*prev = peer{}
		}
	}

	if s.initialized {
		if expiry, ok := asInt64(obj(obj(st, "server"), "issuer_cert_not_after_unix")); ok &&
			sub(expiry, now) <= 14*86400 &&
			(s.certificateAt == nil || sub(now, *s.certificateAt) >= 86400) {
			t := now
			s.certificateAt = &t
			messages = append(messages, "El certificado de la red caduca en 14 días o menos")
		}
	}
	s.initialized = true
	return messages
}

// Sanitize quita controles, comillas, barra invertida y marcas bidi; máximo 180 runas.
func Sanitize(text string) string {
	var b strings.Builder
	n := 0
	for _, r := range text {
		if n == 180 {
			break
		}
		if r < 0x20 || (r >= 0x7f && r <= 0x9f) || r == '\'' || r == '"' || r == '\\' ||
			(r >= 0x202a && r <= 0x202e) || (r >= 0x2066 && r <= 0x2069) {
			continue
		}
		b.WriteRune(r)
		n++
	}
	return b.String()
}

// markup escapa lo que el cuerpo de `notify-send` puede interpretar como marcado (<b>, <a href>…, según la especificación
// de notificaciones). Sanitize no lo toca: el aviso de macOS es texto plano.
var markup = strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;")

// Command devuelve el programa y argumentos que muestran el aviso en el SO dado (macOS usa Native).
func Command(goos, text string) (program string, args []string, ok bool) {
	text = Sanitize(text)
	if goos == "linux" {
		return "notify-send", []string{"--", "Atenea One", markup.Replace(text)}, true
	}
	return "", nil, false
}

// Run lanza el programa descartando entrada y salida, con tope de 10 s.
var Run = func(program string, args []string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	return exec.CommandContext(ctx, program, args...).Run()
}

// Native entrega el aviso desde la propia aplicación; solo existe en macOS.
var Native func(text string) error

// Deliver entrega el aviso; devuelve nil si el SO no lo admite.
func Deliver(text string) error {
	if Native != nil {
		return Native(Sanitize(text))
	}
	program, args, ok := Command(runtime.GOOS, text)
	if !ok {
		return nil
	}
	return Run(program, args)
}
