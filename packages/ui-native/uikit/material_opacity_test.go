package uikit

import (
	"testing"

	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

func TestMaterialRangeSaturatesBeforeTheSliderEnds(t *testing.T) {
	clear := [4]uint8{}
	neutral := [4]uint8{255, 255, 255, 209}
	opaque := [4]uint8{255, 255, 255, 255}
	catalog := map[string]theme.WidgetStyle{
		"selectMenu": {Fill: neutral, FillRange: &theme.ColorRange{
			Clear: clear, Full: opaque,
			Stops: []theme.ColorStop{{Percent: 0, RGBA: clear}, {Percent: 65, RGBA: neutral},
				{Percent: 80, RGBA: opaque}, {Percent: 100, RGBA: opaque}},
		}},
	}
	kit := NewKit("light", theme.Select("light"), catalog)
	for _, test := range []struct {
		percent int
		alpha   uint8
	}{{-5, 0}, {20, 64}, {65, 209}, {75, 240}, {80, 255}, {92, 255}, {105, 255}} {
		got := kit.WithSurfaceOpacity(test.percent).style("selectMenu").Fill
		if got[3] != test.alpha || test.alpha > 0 && got[0] != 255 {
			t.Errorf("opacity %d%% paints %v, want alpha %d", test.percent, got, test.alpha)
		}
	}
	legacy := &theme.ColorRange{Clear: clear, Full: opaque}
	if alpha := materialColor(neutral, legacy, 80)[3]; alpha != 229 {
		t.Fatalf("legacy catalog interpolation returned alpha %d", alpha)
	}
}
