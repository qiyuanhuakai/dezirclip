package main

import (
	"strings"
	"testing"

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
