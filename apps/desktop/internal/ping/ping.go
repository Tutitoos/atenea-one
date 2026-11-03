// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package ping acota el ping a las direcciones de túnel publicadas por los pares.
package ping

import (
	"bytes"
	"errors"
	"fmt"
	"math"
	"net/netip"
	"os"
	"os/exec"
	"runtime"
	"strconv"
	"strings"
	"time"
)

var errBad = errors.New("respuesta de ping no válida")

// Validate acepta solo una IP numérica que sea una dirección de túnel de un par de la red
// (status["peers"][*]["addrs"]): ni nombres, ni la IP propia, ni ajenas.
func Validate(address string, status map[string]any) (netip.Addr, error) {
	ip, err := netip.ParseAddr(address)
	if err != nil || ip.Zone() != "" {
		return netip.Addr{}, errors.New("dirección IP no válida")
	}
	peers, _ := status["peers"].([]any)
	for _, p := range peers {
		peer, _ := p.(map[string]any)
		addrs, _ := peer["addrs"].([]any)
		for _, a := range addrs {
			s, _ := a.(string)
			if got, err := netip.ParseAddr(s); err == nil && got == ip {
				return ip, nil
			}
		}
	}
	return netip.Addr{}, errors.New("la dirección no pertenece a un par de esta red")
}

// Result es el resumen de cinco paquetes; los tiempos son nulos si no llegó ninguno.
type Result struct {
	Sent     uint32   `json:"sent"`
	Received uint32   `json:"received"`
	LossPct  float64  `json:"loss_pct"`
	MinMs    *float64 `json:"min_ms"`
	AvgMs    *float64 `json:"avg_ms"`
	MaxMs    *float64 `json:"max_ms"`
}

// num imita serde_json: un float entero lleva «.0».
func num(v *float64) string {
	if v == nil {
		return "null"
	}
	s := strconv.FormatFloat(*v, 'f', -1, 64)
	if !strings.Contains(s, ".") {
		s += ".0"
	}
	return s
}

// MarshalJSON emite las claves en orden alfabético, como el Value de serde_json del Rust.
func (r Result) MarshalJSON() ([]byte, error) {
	return fmt.Appendf(nil, `{"avg_ms":%s,"loss_pct":%s,"max_ms":%s,"min_ms":%s,"received":%d,"sent":%d}`,
		num(r.AvgMs), num(&r.LossPct), num(r.MaxMs), num(r.MinMs), r.Received, r.Sent), nil
}

func parseCount(field string) (uint32, error) {
	f := strings.Fields(field)
	if len(f) == 0 {
		return 0, errBad
	}
	s := strings.TrimPrefix(f[0], "+") // u32::from_str admite un «+» inicial
	if s == "" || s[0] == '+' || s[0] == '-' {
		return 0, errBad
	}
	n, err := strconv.ParseUint(s, 10, 32)
	if err != nil {
		return 0, errBad
	}
	return uint32(n), nil
}

func parseFloat(s string) (float64, error) {
	if strings.ContainsAny(s, "xX_pP") { // Go admite hexadecimales y guiones bajos; Rust no
		return 0, errBad
	}
	v, err := strconv.ParseFloat(s, 64)
	if err != nil {
		return 0, errBad
	}
	return v, nil
}

func finite(v float64) bool { return !math.IsInf(v, 0) && !math.IsNaN(v) }

// Parse valida el resumen de `ping -c 5` (macOS y Linux).
func Parse(text string) (Result, error) {
	lines := strings.Split(strings.ReplaceAll(text, "\r\n", "\n"), "\n")
	summary := ""
	found := false
	for _, l := range lines {
		if strings.Contains(l, "packets transmitted,") {
			summary, found = l, true
			break
		}
	}
	if !found {
		return Result{}, errBad
	}
	fields := strings.Split(summary, ",")
	sent, err := parseCount(fields[0])
	if err != nil {
		return Result{}, err
	}
	if len(fields) < 2 {
		return Result{}, errBad
	}
	received, err := parseCount(fields[1])
	if err != nil {
		return Result{}, err
	}
	lossText, ok := "", false
	for _, f := range fields {
		if strings.Contains(f, "packet loss") {
			lossText, _, _ = strings.Cut(f, "%")
			ok = true
			break
		}
	}
	if !ok {
		return Result{}, errBad
	}
	loss, err := parseFloat(strings.TrimSpace(lossText))
	if err != nil {
		return Result{}, err
	}
	if sent != 5 || received > sent || !finite(loss) || loss < 0 || loss > 100 ||
		math.Abs(loss-100*float64(sent-received)/float64(sent)) > 0.1 {
		return Result{}, errBad
	}
	res := Result{Sent: sent, Received: received, LossPct: loss}
	if received > 0 {
		line, ok := "", false
		for _, l := range lines {
			if strings.HasPrefix(l, "round-trip ") || strings.HasPrefix(l, "rtt ") {
				line, ok = l, true
				break
			}
		}
		if !ok {
			return Result{}, errBad
		}
		_, rest, ok := strings.Cut(line, "=")
		if !ok {
			return Result{}, errBad
		}
		w := strings.Fields(rest)
		if len(w) == 0 {
			return Result{}, errBad
		}
		var values []float64
		for _, p := range strings.Split(w[0], "/") {
			v, err := parseFloat(p)
			if err != nil {
				return Result{}, err
			}
			values = append(values, v)
		}
		if len(values) < 3 || values[0] > values[1] || values[1] > values[2] {
			return Result{}, errBad
		}
		for _, v := range values {
			if !finite(v) || v < 0 {
				return Result{}, errBad
			}
		}
		res.MinMs, res.AvgMs, res.MaxMs = &values[0], &values[1], &values[2]
	}
	return res, nil
}

// Command elige programa y argumentos según el sistema operativo; sin shell.
func Command(goos string, ip netip.Addr) (string, []string, error) {
	if goos == "windows" {
		return "", nil, errors.New("no soportado en Windows")
	}
	mac := goos == "darwin"
	program := "ping"
	if mac {
		program = "/sbin/ping"
		if ip.Is6() {
			program = "/sbin/ping6"
		}
	}
	var args []string
	if !mac && ip.Is6() {
		args = append(args, "-6")
	}
	args = append(args, "-n", "-c", "5", "-i", "0.2")
	if mac {
		args = append(args, "-W", "1000")
	} else {
		args = append(args, "-w", "8", "-W", "1")
	}
	return program, append(args, ip.String()), nil
}

const deadline = 9 * time.Second

// Exec lanza el programa con LC_ALL=C y devuelve su salida estándar; mata el proceso al vencer el plazo.
// Es una variable para sustituirla en las pruebas.
var Exec = execute

func execute(program string, args []string, timeout time.Duration) (string, error) {
	cmd := exec.Command(program, args...)
	cmd.Env = append(os.Environ(), "LC_ALL=C")
	var out bytes.Buffer
	cmd.Stdout = &out
	if err := cmd.Start(); err != nil {
		return "", fmt.Errorf("no se pudo ejecutar ping: %w", err)
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	select {
	case err := <-done:
		_ = err // como el Rust, solo importa la salida: parse decide
		return out.String(), nil
	case <-time.After(timeout):
		_ = cmd.Process.Kill()
		<-done
		return "", errors.New("ping agotó el plazo de 9 s")
	}
}

// Run hace cinco pings y devuelve el resumen como JSON en texto.
func Run(ip netip.Addr) (string, error) { return run(runtime.GOOS, ip, deadline) }

func run(goos string, ip netip.Addr, timeout time.Duration) (string, error) {
	program, args, err := Command(goos, ip)
	if err != nil {
		return "", err
	}
	text, err := Exec(program, args, timeout)
	if err != nil {
		return "", err
	}
	res, err := Parse(text)
	if err != nil {
		return "", err
	}
	b, err := res.MarshalJSON()
	return string(b), err
}
