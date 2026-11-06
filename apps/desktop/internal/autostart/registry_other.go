// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

//go:build !windows

package autostart

import "errors"

var errNoRegistry = errors.New("el registro de Windows solo existe en Windows")

func windowsGet() (bool, error)     { return false, errNoRegistry }
func windowsSet(string, bool) error { return errNoRegistry }
