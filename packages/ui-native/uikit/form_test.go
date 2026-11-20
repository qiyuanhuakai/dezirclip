package uikit

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestHotkeyListRemoveKeepsSingleBindingAndDoesNotClose(t *testing.T) {
	bindings := []string{"Ctrl+Shift+V", "Alt+F8"}
	closed := false
	kit := Kit{}
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
