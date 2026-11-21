package uikit

import (
	"image"
	"image/color"
	"math"
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
		v := PreviewImage{}
		view := ui.NewTester(func(c *ui.Context) {
			ui.Column(c).Fill().Children(func() {
				v.ZoomControls(c, ImageZoomLabels{FitLabel: "Fit"})
				v.Draw(c, bitmap, "Image")
			})
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

func TestPinnedImageZoomAndPan(t *testing.T) {
	img := image.NewRGBA(image.Rect(0, 0, 640, 320))
	for y := 0; y < 320; y++ {
		for x := 0; x < 640; x++ {
			img.SetRGBA(x, y, color.RGBA{R: uint8(x / 3), G: uint8(y / 2), B: 50, A: 255})
		}
	}
	bitmap := ui.NewBitmap(img)
	var v PreviewImage
	appearance := ImageZoomLabels{In: "Zoom in", Out: "Zoom out", Fit: "Fit image", FitLabel: "Fit", Actual: "Actual size"}
	view := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() {
			v.ZoomControls(c, appearance)
			v.Draw(c, bitmap, "Picture")
		})
	}, 320, 220)
	pixel := func() color.RGBA { return view.Image().RGBAAt(120, 120) }
	before := pixel()
	if v.factor() != 0.5 || before.B != 50 {
		t.Fatalf("initial fit %g pixel %v", v.factor(), before)
	}
	view.Wheel(120, 120, 0, -30)
	after := pixel()
	if math.Abs(float64(v.zoom-0.6)) > 0.0001 || math.Abs(float64(before.R)-float64(after.R)) > 1 || math.Abs(float64(before.G)-float64(after.G)) > 1 {
		t.Fatalf("wheel failed to hold the image at the pointer: %g %v -> %v", v.zoom, before, after)
	}
	click := func(label string) {
		t.Helper()
		if err := view.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	click("100%")
	if v.zoom != 1 {
		t.Fatal("actual size", v.zoom)
	}
	before = pixel()
	view.Press(120, 120)
	view.Move(165, 120)
	view.Release(165, 120)
	after = pixel()
	if after.R+10 >= before.R {
		t.Fatalf("drag did not move the rendered image: %v -> %v", before, after)
	}
	click("+")
	if v.zoom != 1.25 {
		t.Fatal("zoom in", v.zoom)
	}
	click("−")
	if v.zoom != 1 {
		t.Fatal("zoom out", v.zoom)
	}
	view.Pinch(120, 120, 20)
	if v.zoom != 8 {
		t.Fatal("upper zoom limit", v.zoom)
	}
	view.Pinch(120, 120, 0.001)
	if v.zoom != 0.1 {
		t.Fatal("lower zoom limit", v.zoom)
	}
	click("Fit")
	if v.zoom != 0 || v.factor() != 0.5 || v.pan != (ui.Point{}) || pixel().B != 50 {
		t.Fatalf("fit did not reset image: %+v", v)
	}
}

func TestPreviewToolbarZoomOutUsesActualSizeBeforeImageLayout(t *testing.T) {
	const zoomOut = "Zoom out"
	var image PreviewImage
	tester := ui.NewTester(func(c *ui.Context) {
		(Kit{}).PreviewToolbar(c, PreviewToolbarOptions{
			Title: "Content preview", Pinned: true, Image: &image,
			ImageLabels: ImageZoomLabels{Out: zoomOut, In: "Zoom in", Fit: "Fit image", FitLabel: "Fit", Actual: "Actual size"},
		})
	}, 560, 100)
	if err := tester.Click(zoomOut); err != nil {
		t.Fatal(err)
	}
	if scale, fitted := image.Zoom(); scale != 0.8 || fitted {
		t.Fatalf("after zooming out: scale = %g, fitted = %t", scale, fitted)
	}

	image.SetZoom(0)
	image.fit = 0.05
	tester.Image()
	if err := tester.Click(zoomOut); err != nil {
		t.Fatal(err)
	}
	if scale, fitted := image.Zoom(); scale != 0.05 || !fitted {
		t.Fatalf("below the minimum: scale = %g, fitted = %t", scale, fitted)
	}
}
