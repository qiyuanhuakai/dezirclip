package quickpaste

import (
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestPanelPaintsSurfaceOpacityOnce(t *testing.T) {
	for _, opacity := range []float64{0, .65, 1} {
		panel := &Panel{mode: "light", locale: "en", opacity: opacity}
		panel.model.Visible = true
		frame := ui.NewTester(panel.view, 560, 440).Image()
		got := frame.RGBAAt(550, 220).A
		if math.Abs(float64(got)-opacity*255) > 2 {
			t.Fatalf("opacity %v painted alpha %d", opacity, got)
		}
		if corner := frame.RGBAAt(0, 0).A; corner != 0 {
			t.Fatalf("rounded panel left an opaque root corner: %d", corner)
		}
	}
}
