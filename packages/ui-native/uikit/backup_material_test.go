package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

func TestBackupGlassSoftensOnlyTheAreaCoveredByTheDialog(t *testing.T) {
	open := false
	kit := Kit{widgets: map[string]theme.WidgetStyle{
		"backupBackdrop": {Opacity: 1},
		"backupDialog":   {Width: 220, Radius: 16, BackdropBlur: 0.54, Opacity: 1, Fill: [4]uint8{255, 255, 255, 133}},
	}}
	view := ui.NewTester(func(c *ui.Context) {
		ui.Box(c).Fill().Background(ui.RGB(255, 255, 255)).Draw(func(p *ui.Painter, bounds ui.Rect) {
			for _, x := range []float32{25, 90, 140} {
				p.Fill(ui.Rect{X: x, Y: 0, W: 2, H: bounds.H}, ui.RGB(0, 0, 0), 0)
			}
			p.Fill(ui.Rect{X: 180, Y: 0, W: 75, H: bounds.H}, ui.RGB(0, 0, 0), 0)
			p.Fill(ui.Rect{X: 158, Y: 0, W: 1, H: bounds.H}, ui.RGB(0, 0, 0), 0)
		})
		kit.BackupDialog(c, &open, "Export", false, func() {
			ui.Box(c).FillWidth().Height(110)
		}, func() {})
	}, 340, 280)

	withoutDialog := view.Image()
	open = true
	view.Frame()
	withDialog := view.Image()
	outside := [][2]int{{25, 140}, {90, 32}, {190, 245}}
	for _, point := range outside {
		x, y := point[0], point[1]
		if before, after := withoutDialog.At(x, y), withDialog.At(x, y); before != after {
			t.Errorf("pixel outside the sheet changed at (%d,%d): %v -> %v", x, y, before, after)
		}
	}
	stroke := int(withDialog.RGBAAt(141, 140).R)
	edge := int(withDialog.RGBAAt(142, 140).R)
	dark, bright := int(withDialog.RGBAAt(210, 140).R), int(withDialog.RGBAAt(165, 140).R)
	single := int(withDialog.RGBAAt(158, 140).R)
	if bright-single < 65 || bright-stroke < 90 || bright-edge < 4 || edge <= stroke || bright-dark < 115 {
		t.Errorf("glass should retain narrow strokes with softened edges: single=%d, double=%d, edge=%d, dark=%d, light=%d", single, stroke, edge, dark, bright)
	}
}
