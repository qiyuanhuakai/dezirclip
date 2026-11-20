package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestContextMenuKeepsSubmenuInSameSurface(t *testing.T) {
	var menu ContextMenu
	var selected int
	view := ui.NewTester(func(c *ui.Context) {
		k := Kit{}
		anchor := k.Button(c, ButtonOptions{Label: "Clipboard entry"})
		if anchor.RightClicked() {
			menu.Open(anchor, func() []MenuAction {
				return []MenuAction{{Label: "Privacy", Children: []MenuAction{{Label: "Manage rules", Run: func() { selected++ }}}}}
			})
		}
		k.ContextMenu(c, &menu, "Close menu")
	}, 260, 310)
	if err := view.RightClick("Clipboard entry"); err != nil || !menu.Active() || len(view.Menu()) != 0 {
		t.Fatalf("right-click did not open the themed surface: %v", err)
	}
	if err := view.Click("Privacy"); err != nil {
		t.Fatal(err)
	}
	if err := view.Click("Manage rules"); err != nil || selected != 1 || menu.Active() {
		t.Fatalf("submenu did not dispatch exactly one action: selected=%d active=%t, %v", selected, menu.Active(), err)
	}
}
