// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package notify

import (
	"context"
	"strconv"
	"sync"
	"sync/atomic"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/services/notifications"
)

// En macOS el aviso lo emite la propia aplicación (UNUserNotificationCenter): con `osascript` lo emitía Script
// Editor y el clic abría ese programa. El servicio NO se registra en `application.Options.Services`: todo método de
// un servicio vinculado es invocable desde la página (ADR-019 §2) y este solo lo usa el código Go.
// Requiere la aplicación empaquetada (bundle id válido); con `go run` falla y el error llega al registro.
var (
	service   = notifications.New()
	once      sync.Once
	ready     error
	sequence  atomic.Uint64
	activator atomic.Pointer[func()]
)

func init() { Native = deliverNative }

// OnClick fija qué hacer cuando el usuario pulsa un aviso (mostrar la ventana).
func OnClick(f func()) {
	activator.Store(&f)
	service.OnNotificationResponse(func(r notifications.NotificationResult) {
		if r.Error == nil && r.Response.ActionIdentifier == notifications.DefaultActionIdentifier {
			if p := activator.Load(); p != nil {
				(*p)()
			}
		}
	})
}

func deliverNative(text string) error {
	once.Do(func() {
		if ready = service.ServiceStartup(context.Background(), application.ServiceOptions{}); ready != nil {
			return
		}
		granted, err := service.RequestNotificationAuthorization()
		if err == nil && !granted {
			err = errDenied
		}
		ready = err
	})
	if ready != nil {
		return ready
	}
	return service.SendNotification(notifications.NotificationOptions{
		ID:    "atenea-" + strconv.FormatUint(sequence.Add(1), 10),
		Title: "Atenea One",
		Body:  text,
	})
}
