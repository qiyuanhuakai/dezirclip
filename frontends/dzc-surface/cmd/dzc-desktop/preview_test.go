package main

import (
	"image"
	"image/color"
	"image/png"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
)

func TestPreviewVisualFixture(t *testing.T) {
	directory := os.Getenv("DZC_VISUAL_DIR")
	if directory == "" {
		t.Skip("visual export directory is not set")
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	for _, dark := range []bool{false, true} {
		mode := "light"
		if dark {
			mode = "dark"
		}
		p := &pinnedPreview{ticket: pinPreviewRequest{Appearance: previewAppearance{Title: "Content preview", Dark: dark, Opacity: 85, Close: "Close preview", OpacityLabel: "Opacity"}},
			opacity: previewOpacityControl{value: 85, slider: 85},
			body: &previewBody{source: "DezirClip", kind: "rich_text", timestamp: 1794222000000,
				formatted: previewDocument(richtext.ParseHTML("<h2>Native content preview</h2><p>Clipboard notes keep <b>bold text</b>, <i>emphasis</i> and paragraph spacing.</p><p>The compact toolbar shares the same visual hierarchy as the main WebView.</p><p>A second paragraph stays readable in both light and dark themes.</p>"))}}
		frame := ui.NewTester(p.view, 480, 320).Image()
		if frame.RGBAAt(0, 0).A != 0 {
			t.Fatal("preview corners must be transparent")
		}
		file, err := os.Create(filepath.Join(directory, "preview-"+mode+".png"))
		if err != nil {
			t.Fatal(err)
		}
		err = png.Encode(file, frame)
		file.Close()
		if err != nil {
			t.Fatal(err)
		}
	}
}

func TestLongPreviewOpensAtFirstParagraph(t *testing.T) {
	const opening = "First paragraph"
	for name, contents := range map[string]richtext.Text{
		"html":  richtext.ParseHTML("<p>" + opening + "</p>" + strings.Repeat("<p>Later paragraph</p>", 40)),
		"plain": richtext.NewText(opening + strings.Repeat("\nLater paragraph", 40)),
	} {
		t.Run(name, func(t *testing.T) {
			doc := previewDocument(contents)
			view := ui.NewTester(func(c *ui.Context) { drawRichPreview(c, doc) }, 360, 160)
			view.ClickAt(30, 12)
			anchor, caret := doc.Selection()
			if anchor != caret || caret <= 0 || caret > len(opening) {
				t.Fatalf("hit near the top selected position %d:%d, outside the first paragraph", anchor, caret)
			}
		})
	}
}

func TestRichPreviewPaintsOnlyOneBackground(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, opacity := range []int{0, 65, 100} {
			a := previewAppearance{Dark: dark, Opacity: opacity}
			a.Background = &struct{ R, G, B uint8 }{55, 89, 124}
			theme := a.theme()
			blank := ui.NewTester(func(c *ui.Context) { c.SetTheme(theme) }, 360, 160).Image().RGBAAt(230, 90)
			doc := previewDocument(richtext.ParseHTML("<p><b>Visible formatted text</b></p>"))
			frame := ui.NewTester(func(c *ui.Context) { c.SetTheme(theme); drawRichPreview(c, doc) }, 360, 160).Image()
			if got := frame.RGBAAt(230, 90); got != blank {
				t.Fatalf("dark=%v opacity=%d: editor background %v, root %v", dark, opacity, got, blank)
			}
			visible := false
			for y := 0; y < 60 && !visible; y++ {
				for x := 0; x < 220; x++ {
					pixel := frame.RGBAAt(x, y)
					if pixel.A >= 220 && pixel != blank {
						visible = true
						break
					}
				}
			}
			if !visible {
				t.Fatalf("dark=%v opacity=%d: formatted text missing", dark, opacity)
			}
		}
	}
}

func TestPreviewNeverCoversItsSourceRow(t *testing.T) {
	for _, c := range []struct {
		area, row mygo.Rectangle
		point     mygo.Point
		w, h      int
	}{
		{mygo.Rectangle{Width: 800, Height: 600}, mygo.Rectangle{X: 224, Y: 290, Width: 352, Height: 40}, mygo.Point{X: 430, Y: 130}, 480, 286},
		{mygo.Rectangle{X: -300, Width: 300, Height: 320}, mygo.Rectangle{X: -300, Y: 100, Width: 300, Height: 40}, mygo.Point{X: -290, Y: 90}, 300, 176},
		{mygo.Rectangle{Width: 200, Height: 150}, mygo.Rectangle{Width: 200, Height: 150}, mygo.Point{}, 0, 0},
	} {
		r := previewBounds(c.area, c.row, c.point)
		if r.Width != c.w || r.Height != c.h {
			t.Fatalf("unexpected fitted size: %+v", r)
		}
		if r.Width == 0 {
			continue
		}
		if r.X < c.area.X || r.Y < c.area.Y || r.X+r.Width > c.area.X+c.area.Width || r.Y+r.Height > c.area.Y+c.area.Height ||
			(r.X < c.row.X+c.row.Width && r.X+r.Width > c.row.X && r.Y < c.row.Y+c.row.Height && r.Y+r.Height > c.row.Y) {
			t.Fatalf("invalid preview area %+v for row %+v on %+v", r, c.row, c.area)
		}
	}
}

func TestMixedDPIPreviewUsesParentPhysicalWorkArea(t *testing.T) {
	for _, scenario := range []struct {
		parent, work mygo.Rectangle
		origin       mygo.Point
		dpi          int
	}{
		{mygo.Rectangle{X: 1500, Y: 800, Width: 352, Height: 380}, mygo.Rectangle{Width: 1920, Height: 1080}, mygo.Point{X: 1500, Y: 800}, 96},
		{mygo.Rectangle{X: 1470, Y: 320, Width: 352, Height: 380}, mygo.Rectangle{X: 1920, Width: 2560, Height: 1440}, mygo.Point{X: 1838, Y: 400}, 120},
	} {
		parent := scenario.parent
		area := previewParentArea(parent, scenario.origin, scenario.work, float64(scenario.dpi)/96)
		row := mygo.Rectangle{X: parent.X, Y: parent.Y + 100, Width: 352, Height: 40}
		r := previewBounds(area, row, mygo.Point{X: 1864, Y: 740})
		px := func(dip int) int { return (dip*scenario.dpi + 48) / 96 } // MyGo Windows Flyout.Place conversion.
		x, y := scenario.origin.X+px(r.X-parent.X), scenario.origin.Y+px(r.Y-parent.Y)
		endX, endY := x+px(r.Width), y+px(r.Height)
		work := scenario.work
		if r.Width <= 0 || r.Height <= 0 || x < work.X || y < work.Y || endX > work.X+work.Width || endY > work.Y+work.Height ||
			(x < scenario.origin.X+px(row.Width) && endX > scenario.origin.X && y < scenario.origin.Y+px(140) && endY > scenario.origin.Y+px(100)) {
			t.Fatalf("native anchor could flip or obscure its source: (%d,%d)-(%d,%d), work=%+v", x, y, endX, endY, work)
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
	var v pinnedImageView
	appearance := previewAppearance{ZoomIn: "Zoom in", ZoomOut: "Zoom out", Fit: "Fit image", FitLabel: "Fit", Actual: "Actual size", ImageZoom: "Picture"}
	view := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() { v.draw(c, bitmap, appearance) })
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

func TestPinnedWindowRepaintsBackground(t *testing.T) {
	p := &pinnedPreview{ticket: pinPreviewRequest{Appearance: previewAppearance{Opacity: 40, Close: "Close", OpacityLabel: "Opacity"}}, opacity: previewOpacityControl{value: 40, slider: 40}, body: &previewBody{text: "content"}}
	view := ui.NewTester(p.view, 360, 240)
	before := view.Image().RGBAAt(350, 230)
	p.opacity.value, p.opacity.slider = 77, 77
	view.Frame()
	after := view.Image().RGBAAt(350, 230)
	if before.A != 102 || after.A != 196 {
		t.Fatalf("expected alpha to change from 102 to 196, got %v -> %v", before, after)
	}
}
