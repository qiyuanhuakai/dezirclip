package uikit

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
)

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
