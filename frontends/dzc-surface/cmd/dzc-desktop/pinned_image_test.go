package main

import (
	"image"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestPinnedImageFitsThinPanoramas(t *testing.T) {
	for _, size := range []image.Point{{X: 64000, Y: 1}, {X: 1, Y: 64000}} {
		img := image.NewRGBA(image.Rectangle{Max: size})
		for i := 0; i < len(img.Pix); i += 4 {
			img.Pix[i+1], img.Pix[i+3] = 255, 255
		}
		bitmap := ui.NewBitmap(img)
		v := pinnedImageView{}
		view := ui.NewTester(func(c *ui.Context) {
			ui.Column(c).Fill().Children(func() { v.draw(c, bitmap, previewAppearance{FitLabel: "Fit"}) })
		}, 320, 220)
		frame := view.Image()
		if v.fit <= 0 || v.fit >= 0.01 {
			t.Fatalf("%v did not fit at its true scale: %v", size, v.fit)
		}
		var visible image.Rectangle
		for y := int(220 - v.height); y < 220; y++ {
			for x := 0; x < 320; x++ {
				pixel := frame.RGBAAt(x, y)
				if int(pixel.G)-int(pixel.R) > 50 {
					visible = visible.Union(image.Rect(x, y, x+1, y+1))
				}
			}
		}
		if size.X > size.Y {
			if visible.Dx() != int(v.width) || visible.Dy() < 1 || visible.Dy() > 2 {
				t.Fatalf("wide panorama coverage %v", visible)
			}
		} else if visible.Dy() != int(v.height) || visible.Dx() < 1 || visible.Dx() > 2 {
			t.Fatalf("tall panorama coverage %v", visible)
		}
	}
}
