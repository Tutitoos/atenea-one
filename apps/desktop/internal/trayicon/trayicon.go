// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package trayicon pinta el punto de estado sobre el icono de la bandeja.
package trayicon

import (
	"bytes"
	"image"
	"image/draw"
	"image/png"
	"math"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/health"
)

var colors = map[health.Health][4]uint8{
	health.Ok:       {0x34, 0xc7, 0x59, 255},
	health.Degraded: {0xff, 0x9f, 0x0a, 255},
	health.Down:     {0xff, 0x45, 0x3a, 255},
	health.Stopped:  {0x8e, 0x8e, 0x93, 255},
}

// WithDot decodifica el PNG base y pinta un punto del color del estado en la esquina inferior derecha.
func WithDot(basePNG []byte, h health.Health) ([]byte, error) {
	src, err := png.Decode(bytes.NewReader(basePNG))
	if err != nil {
		return nil, err
	}
	b := src.Bounds()
	img := image.NewNRGBA(image.Rect(0, 0, b.Dx(), b.Dy()))
	draw.Draw(img, img.Bounds(), src, b.Min, draw.Src)
	w, ht := b.Dx(), b.Dy()
	r := float32(w) * 0.17
	cx, cy := float32(w)-r-1, float32(ht)-r-1
	color := colors[h]
	for y := range ht {
		for x := range w {
			dx, dy := float32(x)+0.5-cx, float32(y)+0.5-cy
			d := float32(math.Sqrt(float64(dx*dx + dy*dy)))
			var c [4]uint8
			switch {
			case d <= r:
				c = color
			case d <= r+1.5:
				c = [4]uint8{0, 0, 0, 140}
			default:
				continue
			}
			copy(img.Pix[y*img.Stride+x*4:], c[:])
		}
	}
	var out bytes.Buffer
	if err := png.Encode(&out, img); err != nil {
		return nil, err
	}
	return out.Bytes(), nil
}
