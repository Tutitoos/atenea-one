// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package trayicon

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"testing"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/health"
)

func TestWithDotPaintsBottomRightCorner(t *testing.T) {
	base := image.NewNRGBA(image.Rect(0, 0, 100, 100))
	grey := color.NRGBA{10, 20, 30, 255}
	for i := 0; i < len(base.Pix); i += 4 {
		copy(base.Pix[i:], []uint8{10, 20, 30, 255})
	}
	var in bytes.Buffer
	if err := png.Encode(&in, base); err != nil {
		t.Fatal(err)
	}
	out, err := WithDot(in.Bytes(), health.Down)
	if err != nil {
		t.Fatal(err)
	}
	img, err := png.Decode(bytes.NewReader(out))
	if err != nil {
		t.Fatal(err)
	}
	conv := func(x, y int) color.NRGBA { return color.NRGBAModel.Convert(img.At(x, y)).(color.NRGBA) }
	// r=17, centro=(82,82): el centro lleva el color del estado, un píxel lejano queda intacto.
	if got := conv(82, 82); got != (color.NRGBA{0xff, 0x45, 0x3a, 255}) {
		t.Errorf("centro: %v", got)
	}
	if got := conv(5, 5); got != grey {
		t.Errorf("lejano: %v", got)
	}
	// x=99 -> d=17.5, borde (r < d <= r+1.5): negro semitransparente.
	if got := conv(99, 82); got != (color.NRGBA{0, 0, 0, 140}) {
		t.Errorf("borde: %v", got)
	}
	// Fuera del borde (d > 18.5) queda intacto.
	if got := conv(82, 60); got != grey {
		t.Errorf("fuera del borde: %v", got)
	}
	// Otro estado, otro color.
	out, _ = WithDot(in.Bytes(), health.Ok)
	img, _ = png.Decode(bytes.NewReader(out))
	if got := conv(82, 82); got != (color.NRGBA{0x34, 0xc7, 0x59, 255}) {
		t.Errorf("ok: %v", got)
	}
	if _, err := WithDot([]byte("no es png"), health.Ok); err == nil {
		t.Error("esperaba error")
	}
}
