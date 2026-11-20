package uikit

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestHistoryCardInterpolatesHoverAndSelection(t *testing.T) {
	kit := Kit{Mode: "light"}
	selected := false
	view := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		kit.HistoryCard(c, HistoryCardOptions{Summary: "History fixture", Selected: selected})
	}, 280, 165)
	bounds, ok := view.Find("History fixture")
	if !ok {
		t.Fatal("history card did not render")
	}
	x, y := int(bounds.X+bounds.W-12), int(bounds.Y+bounds.H/2)
	pixel := func() ui.Color {
		p := view.Image().RGBAAt(x, y)
		return ui.Color{R: p.R, G: p.G, B: p.B, A: p.A}
	}
	duration := time.Duration(kit.palette().TransitionMs) * time.Millisecond
	rest := pixel()
	view.Move(float32(x), float32(y))
	view.Advance(duration / 3)
	hoverFrame := pixel()
	view.Advance(duration)
	hover := pixel()
	if rest == hover || hoverFrame == rest || hoverFrame == hover {
		t.Fatalf("hover did not interpolate: rest=%v intermediate=%v hover=%v", rest, hoverFrame, hover)
	}
	selected = true
	view.Frame()
	view.Advance(duration)
	if pixel() == hover {
		t.Fatal("selected card retained the hover appearance")
	}
}
