package uikit

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestOptionalHotkeyClearRequiresExpandedHelp(t *testing.T) {
	const label, clear = "Privacy session shortcut", "Clear shortcut"
	const help, description = "Setting help: " + label, "Leave empty to disable the shortcut."
	value := "Ctrl+Alt+P"
	disabled := true
	tester := ui.NewTester(func(c *ui.Context) {
		kit := Kit{}
		options := HotkeyRowOptions{HelpLabel: help, Description: description, ClearLabel: clear, Disabled: disabled}
		if value != "" {
			options.OnClear = func() { value = "" }
		}
		kit.HotkeyRowWithOptions(c, label, options, func() {
			kit.HotkeyBinding(c, label, value, "Not set", false, disabled)
		})
	}, 330, 200)
	if tester.HasText(clear) || tester.HasText(description) {
		t.Fatal("optional shortcut help starts open")
	}
	if err := tester.Click(help); err != nil || !tester.HasText(description) {
		t.Fatalf("help did not open: %v", err)
	}
	if err := tester.Click(clear); err != nil || value != "Ctrl+Alt+P" {
		t.Fatalf("disabled clear changed the binding: %v, value %q", err, value)
	}
	disabled = false
	tester.Frame()
	if err := tester.Click(clear); err != nil || value != "" {
		t.Fatalf("clear did not empty the optional binding: %v, value %q", err, value)
	}
	tester.Frame()
	if tester.HasText(clear) || !tester.HasText("NOT SET") {
		t.Fatalf("unexpected shortcut controls after clear: %v", tester.Texts())
	}
}

func TestPrivacyTextAreasResizeVerticallyWithoutChangingTheText(t *testing.T) {
	for _, fixture := range []struct {
		name string
		draw func(Kit, *ui.Context, *string, string, string) ui.Element
	}{
		{"Excluded applications", Kit.SourceExclusionField},
		{"Custom patterns", Kit.PrivacyPatternsField},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			const original = "First line\nSecond line"
			value := original
			tester := ui.NewTester(func(c *ui.Context) {
				fixture.draw(Kit{}, c, &value, fixture.name, "")
			}, 230, 300)
			bounds := func() ui.Rect {
				rect, ok := tester.Find(fixture.name)
				if !ok {
					t.Fatal("textarea missing")
				}
				return rect
			}
			initial := bounds()
			x, y := initial.X+initial.W-5, initial.Y+initial.H-5
			tester.Press(x, y)
			tester.Move(x-32, y+28)
			tester.Move(x-46, y+52)
			tester.Release(x-46, y+52)
			expanded := bounds()
			if expanded.H != initial.H+52 || expanded.W != initial.W || value != original {
				t.Fatalf("drag result %+v, initial %+v, text %q", expanded, initial, value)
			}
			x, y = expanded.X+expanded.W-5, expanded.Y+expanded.H-5
			tester.Press(x, y)
			tester.Move(x, y-100)
			tester.Release(x, y-100)
			if reduced := bounds(); reduced != initial {
				t.Fatalf("resized textarea below its original height: %+v", reduced)
			}
			tester.ClickAt(initial.X+15, initial.Y+15)
			tester.Type("Edited ")
			if value == original {
				t.Fatal("textarea could not be edited after resizing")
			}
		})
	}
}

func TestResizedPrivacyTextAreaKeepsFollowingSettingsReachable(t *testing.T) {
	var scroll ui.ScrollState
	value := "One pattern"
	tester := ui.NewTester(func(c *ui.Context) {
		ui.Scroll(c).Fill().TrackScroll(&scroll).Children(func() {
			ui.Column(c).FillWidth().Padding(6).Gap(10).Children(func() {
				(Kit{}).PrivacyPatternsField(c, &value, "Patterns", "")
				ui.ButtonBase(c).Height(32).Label("Following setting")
			})
		})
	}, 245, 145)
	field, ok := tester.Find("Patterns")
	if !ok {
		t.Fatal("privacy textarea missing")
	}
	x, y := field.X+field.W-5, field.Y+field.H-5
	tester.Press(x, y)
	tester.Move(x, y+51)
	tester.Release(x, y+51)
	if scroll.MaxY <= 0 {
		t.Fatal("the expanded settings did not create scrollable overflow")
	}
	tester.Scroll(4, 90, 0, scroll.MaxY)
	tail, found := tester.Find("Following setting")
	if !found || scroll.Y <= 0 || tail.Y+tail.H > 145 {
		t.Fatalf("could not scroll to the setting below the expanded textarea: %+v at %v", tail, scroll.Y)
	}
}

func TestHotkeyLabelUsesThePlatformModifier(t *testing.T) {
	for _, tt := range []struct{ platform, value, want string }{
		{"windows", "Super+V", "Win+V"},
		{"linux", "Win+V", "Super+V"},
		{"windows", "Ctrl+Meta+V", "Ctrl+Win+V"},
		{"windows", "Ctrl+V", "Ctrl+V"},
		{"darwin", "Super+V", "Super+V"},
	} {
		if got := (Kit{}).WithHotkeyPlatform(tt.platform).hotkeyLabel(tt.value); got != tt.want {
			t.Errorf("%s %q displays as %q, want %q", tt.platform, tt.value, got, tt.want)
		}
	}
}

func TestHotkeyListProtectsOnlyTheSystemBinding(t *testing.T) {
	removed := ""
	view := ui.NewTester(func(c *ui.Context) {
		(Kit{}).WithHotkeyPlatform("windows").HotkeyListWithOptions(c, HotkeyListOptions{
			Values: []string{"Super+V", "Alt+C"}, RemoveLabel: "Remove",
			CanRemove: func(value string) bool { return value != "Super+V" },
			OnRemove:  func(value string) { removed = value },
		})
	}, 420, 180)
	if _, exists := view.Find("Remove Win+V"); exists {
		t.Fatal("the protected Win+V binding has a remove control")
	}
	if err := view.Click("Remove Alt+C"); err != nil || removed != "Alt+C" {
		t.Fatalf("the ordinary binding could not be removed: %v, removed=%q", err, removed)
	}
}

func TestHotkeyListRemoveKeepsSingleBindingAndDoesNotClose(t *testing.T) {
	bindings := []string{"Ctrl+Shift+V", "Alt+F8"}
	closed := false
	kit := (Kit{}).WithHotkeyPlatform("linux")
	view := ui.NewTester(func(c *ui.Context) {
		if kit.Button(c, ButtonOptions{Label: "Close Settings"}).Clicked() {
			closed = true
		}
		kit.HotkeyList(c, bindings, "Not set", "Remove", false, func(value string) {
			bindings = slices.DeleteFunc(bindings, func(binding string) bool { return binding == value })
		})
	}, 420, 180)

	if err := view.Click("Remove Alt+F8"); err != nil {
		t.Fatal(err)
	}
	view.Frame()
	if !slices.Equal(bindings, []string{"Ctrl+Shift+V"}) || closed {
		t.Fatalf("shortcut removal changed the wrong state: bindings=%v closed=%t", bindings, closed)
	}
	if _, ok := view.Find("Remove Ctrl+Shift+V"); ok {
		t.Fatal("the remaining shortcut still displays a removal button")
	}
	if err := view.Click("Close Settings"); err != nil || !closed {
		t.Fatalf("settings close did not remain independently clickable: %v", err)
	}
}
