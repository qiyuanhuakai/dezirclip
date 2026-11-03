package main

import (
	"strings"
	"testing"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func TestLongRichPreviewOpensAtFirstParagraph(t *testing.T) {
	const opening = "First paragraph"
	doc := richPreviewDocument("<p>" + opening + "</p>" + strings.Repeat("<p>Later paragraph</p>", 40))
	view := ui.NewTester(func(c *ui.Context) { drawRichPreview(c, doc) }, 360, 160)
	view.ClickAt(30, 12)
	anchor, caret := doc.Selection()
	if anchor != caret || caret <= 0 || caret > len(opening) {
		t.Fatalf("hit near the top of the first frame selected position %d:%d, outside the first paragraph", anchor, caret)
	}
}

func TestRichPreviewPaintsOnlyOneBackground(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, opacity := range []int{0, 65, 100} {
			a := previewAppearance{Dark: dark, Opacity: opacity}
			a.Background = &struct{ R, G, B uint8 }{55, 89, 124}
			theme := a.theme()
			blank := ui.NewTester(func(c *ui.Context) { c.SetTheme(theme) }, 360, 160).Image().RGBAAt(230, 90)
			doc := richPreviewDocument("<p><b>Visible formatted text</b></p>")
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
		{mygo.Rectangle{Width: 800, Height: 600}, mygo.Rectangle{X: 224, Y: 290, Width: 352, Height: 40}, mygo.Point{X: 430, Y: 130}, 360, 200},
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
