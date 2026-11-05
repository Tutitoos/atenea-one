// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package app

import (
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/health"
)

func TestPollReadsNetworksInParallelAndKeepsTheResult(t *testing.T) {
	list := fakeCLI(t)
	writeList(t, list, `[{"network_id":"`+id1+`","name":"casa"},{"network_id":"`+id2+`","name":"oficina"}]`)
	now := time.Unix(1_000_000, 0)
	var delivered []string
	const slow = 600 * time.Millisecond
	a := trayApp(t, func(socket string) (string, error) {
		time.Sleep(slow) // cada lectura tarda; en serie serían 2×
		if filepath.Base(filepath.Dir(socket)) == "456789ab" {
			return statusJSON("direct"), nil
		}
		return "", errors.New("sin acceso al estado")
	}, &now, &delivered)

	start := time.Now()
	read := a.Poll(&TrayState{})
	if d := time.Since(start); d >= 2*slow-100*time.Millisecond {
		t.Errorf("las redes deben leerse en paralelo: %v", d)
	}
	if read.Health != health.Ok || !strings.HasPrefix(read.Text, "Conectado") {
		t.Errorf("solo cuenta la red que responde: %+v", read)
	}
}
