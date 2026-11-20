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
		anchor := k.Button(c, ButtonOptions{Label: "Clipboard entry"}).Absolute().Right(9).Bottom(9)
		if anchor.RightClicked() {
			menu.Open(anchor, func() []MenuAction {
				return []MenuAction{
					{Label: "Text processing", Children: []MenuAction{{Label: "Uppercase", Run: func() { selected += 10 }}}},
					{Label: "Privacy", Children: []MenuAction{{Label: "Manage rules", Run: func() { selected++ }}}},
					{Label: "Delete", Run: func() { selected += 100 }},
				}
			})
		}
		k.ContextMenu(c, &menu, "Close menu")
	}, 260, 310)
	if err := view.RightClick("Clipboard entry"); err != nil || !menu.Active() || len(view.Menu()) != 0 {
		t.Fatalf("right-click did not open the themed surface: %v", err)
	}
	deleteBefore, ok := view.Find("Delete")
	if !ok {
		t.Fatal("the later root item is missing")
	}
	privacy, ok := view.Find("Privacy")
	if !ok {
		t.Fatal("the privacy menu is missing")
	}
	view.Move(privacy.X+privacy.W/2, privacy.Y+privacy.H/2)
	manage, ok := view.Find("Manage rules")
	if !ok || manage.X+manage.W > privacy.X || manage.Y+manage.H > 310 {
		t.Fatalf("the submenu did not fit to the left of the root: parent=%+v child=%+v", privacy, manage)
	}
	if after, _ := view.Find("Delete"); after != deleteBefore {
		t.Fatalf("opening the submenu moved Delete: %+v -> %+v", deleteBefore, after)
	}
	if err := view.Click("Manage rules"); err != nil || selected != 1 || menu.Active() {
		t.Fatalf("submenu did not dispatch exactly one action: selected=%d active=%t, %v", selected, menu.Active(), err)
	}
	if err := view.RightClick("Clipboard entry"); err != nil {
		t.Fatal(err)
	}
	text, ok := view.Find("Text processing")
	if !ok {
		t.Fatal("text processing menu is missing")
	}
	view.Move(text.X+text.W/2, text.Y+text.H/2)
	if _, ok := view.Find("Uppercase"); !ok {
		t.Fatal("text processing submenu is missing")
	}
	if err := view.Click("Delete"); err != nil || selected != 101 {
		t.Fatalf("a lower root item was inaccessible with the submenu open: selected=%d, %v", selected, err)
	}
}
