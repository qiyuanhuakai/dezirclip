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

func TestContextPrivacyActionsAreReachableByKeyboard(t *testing.T) {
	var menu ContextMenu
	var selected []string
	view := ui.NewTester(func(c *ui.Context) {
		kit := Kit{}
		anchor := kit.Button(c, ButtonOptions{Label: "Clipboard entry"}).Absolute().Right(9).Bottom(9).AutoFocus()
		if anchor.RightClicked() {
			menu.Open(anchor, func() []MenuAction {
				return []MenuAction{
					{Label: "Copy"},
					{Label: "Not available", Disabled: true},
					{Label: "Privacy", Children: []MenuAction{
						{Label: "Keep this entry"},
						{Label: "Protected action", Disabled: true},
						{Label: "Ignore this source"},
						{Label: "Protect this source"},
						{Label: "Manage privacy rules", Run: func() { selected = append(selected, "rules") }},
					}},
					{Label: "Delete", Run: func() { selected = append(selected, "delete") }},
				}
			})
		}
		kit.ContextMenu(c, &menu, "Close menu")
	}, 260, 125)

	openPrivacy := func() {
		t.Helper()
		if err := view.RightClick("Clipboard entry"); err != nil {
			t.Fatal(err)
		}
		view.Key(0, ui.KeyDown)
		if !view.Focused("Privacy") {
			t.Fatal("Down did not skip the disabled root action")
		}
		view.Key(0, ui.KeyRight)
		if !view.Focused("Keep this entry") {
			t.Fatal("Right did not open the privacy submenu at its first action")
		}
	}

	openPrivacy()
	view.Key(0, ui.KeyDown)
	if !view.Focused("Ignore this source") {
		t.Fatal("Down did not skip the disabled privacy action")
	}
	view.Key(0, ui.KeyUp)
	view.Key(0, ui.KeyUp)
	bounds, visible := view.Find("Manage privacy rules")
	if !view.Focused("Manage privacy rules") || !visible || bounds.Y < 6 || bounds.Y+bounds.H > 119 {
		t.Fatalf("wrapping to the last action did not scroll it into view: %+v", bounds)
	}
	view.Key(0, ui.KeyEnter)
	if menu.Active() || len(selected) != 1 || selected[0] != "rules" {
		t.Fatalf("Enter did not dispatch the highlighted submenu action once: %v", selected)
	}

	openPrivacy()
	view.Key(0, ui.KeyLeft)
	if view.HasText("Keep this entry") || !view.Focused("Privacy") {
		t.Fatal("Left did not close the submenu and return focus to its parent")
	}
	view.Key(0, ui.KeyDown)
	if !view.Focused("Delete") {
		t.Fatal("the lower root action was not reachable after closing the submenu")
	}
	view.Key(0, ui.KeyEnter)
	if menu.Active() || len(selected) != 2 || selected[1] != "delete" {
		t.Fatalf("Enter did not dispatch the highlighted root action once: %v", selected)
	}

	openPrivacy()
	view.Key(0, ui.KeyEscape)
	if menu.Active() || !view.Focused("Clipboard entry") || len(selected) != 2 {
		t.Fatalf("Escape did not dismiss the entire menu and restore focus: open=%t anchor focused=%t actions=%v", menu.Active(), view.Focused("Clipboard entry"), selected)
	}
}
