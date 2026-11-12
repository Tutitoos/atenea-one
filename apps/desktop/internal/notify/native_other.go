// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build !darwin

package notify

// OnClick no hace nada fuera de macOS: `notify-send` no devuelve clics.
func OnClick(func()) {}
