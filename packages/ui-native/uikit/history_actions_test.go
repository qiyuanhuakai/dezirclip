package uikit

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

func TestHistoryActionUsesThemeForHoverPressAndSelection(t *testing.T) {
	for _, widget := range []string{"historyAction", "historyActionCompact"} {
		t.Run(widget, func(t *testing.T) {
			colors := []struct {
				state string
				red   uint8
			}{{"", 25}, {"Hover", 50}, {"Pressed", 75}, {"Active", 100}, {"ActiveHover", 125}, {"ActivePressed", 150}}
			widgets := make(map[string]theme.WidgetStyle, len(colors))
			for _, color := range colors {
				widgets[widget+color.state] = theme.WidgetStyle{
					Width: 24, Height: 24, Opacity: 1, Fill: [4]uint8{color.red, 36, 48, 255},
				}
			}
			kit := Kit{widgets: widgets}
			active := false
			view := ui.NewTester(func(c *ui.Context) {
				ui.Column(c).Padding(12).Children(func() {
					kit.HistoryCardActions(c, []HeaderAction{{Icon: "pin", Label: "Pin", Active: active}},
						widget == "historyActionCompact")
				})
			}, 120, 90)
			bounds, ok := view.Find("Pin")
			if !ok {
				t.Fatal("the card action was not rendered")
			}
			x, y := bounds.X+bounds.W/2, bounds.Y+bounds.H/2
			assertColor := func(state string, want uint8) {
				t.Helper()
				view.Advance(210 * time.Millisecond)
				if actual := view.Image().RGBAAt(int(bounds.X)+3, int(bounds.Y)+3).R; actual != want {
					t.Fatalf("%s red channel = %d, want %d", state, actual, want)
				}
			}
			assertColor("resting", 25)
			view.Move(x, y)
			assertColor("hovered", 50)
			view.Press(x, y)
			assertColor("pressed", 75)
			view.Release(x, y)
			view.Move(100, 80)
			active = true
			view.Frame()
			assertColor("selected", 100)
			view.Move(x, y)
			assertColor("selected and hovered", 125)
			view.Press(x, y)
			assertColor("selected and pressed", 150)
			view.Release(x, y)
		})
	}
}
