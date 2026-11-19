package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSelectReportsChoiceAndRejectsDisabledOption(t *testing.T) {
	kit := Kit{}
	choice := "a"
	options := []SelectOption{
		{Value: "a", Label: "First"},
		{Value: "b", Label: "Unavailable", Disabled: true},
		{Value: "c", Label: "Last"},
	}
	tester := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		kit.Select(c, &choice, "Choice", options, false)
	}, 240, 180)
	mustClick := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	mustClick("First")
	mustClick("Unavailable")
	if choice != "a" {
		t.Fatal("disabled option updated selection")
	}
	mustClick("Last")
	if choice != "c" {
		t.Fatal("choosing an enabled option failed")
	}
}

func TestSettingsNavigationAndFooterUseApplicationActions(t *testing.T) {
	kit := Kit{}
	active, action := "appearance", ""
	tester := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		ui.Column(c).Fill().Children(func() {
			kit.SettingsRail(c, "Settings", active, []SettingsSection{
				{ID: "appearance", Label: "Appearance"},
				{ID: "general", Label: "General"},
			}, func(id string) { active = id })
			kit.SettingsFooter(c, SettingsFooterOptions{
				ProductName: "DezirClip", Feedback: "Feedback", Reset: "Reset defaults", Website: "Official website",
				OnFeedback: func() { action = "feedback" },
				OnReset:    func() { action = "reset" },
				OnWebsite:  func() { action = "website" },
			})
		})
	}, 560, 420)
	if err := tester.Click("General"); err != nil {
		t.Fatal(err)
	}
	if active != "general" {
		t.Fatalf("selected %q", active)
	}
	for _, item := range []struct{ label, expected string }{
		{"Feedback", "feedback"}, {"Reset defaults", "reset"},
		{"Official website", "website"}, {"GitHub", "website"},
	} {
		action = ""
		if err := tester.Click(item.label); err != nil {
			t.Fatal(err)
		}
		if action != item.expected {
			t.Fatalf("%s called %q", item.label, action)
		}
	}
}

func TestDisclosureAndModalKeyboardBehavior(t *testing.T) {
	kit := Kit{Mode: "dark"}
	open, dialog, busy := false, true, true
	tester := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		kit.SettingsDisclosure(c, "Settings", &open, func() { ui.Text(c, "Preferences") })
		kit.Dialog(c, &dialog, "Create tag", "Close", busy, func() { ui.Text(c, "Name") })
	}, 420, 360)
	tester.Key(0, ui.KeyEscape)
	if !dialog {
		t.Fatal("busy dialog accepted Escape")
	}
	busy = false
	tester.Frame()
	tester.Key(0, ui.KeyEscape)
	if dialog {
		t.Fatal("dialog did not accept Escape")
	}
	if err := tester.Click("Settings"); err != nil {
		t.Fatal(err)
	}
	if !open || !tester.HasText("Preferences") {
		t.Fatal("expanded disclosure did not render its content")
	}
}

func TestTagDialogCloseButtonAndNameField(t *testing.T) {
	kit := Kit{}
	open, busy, name := true, true, "Work"
	tester := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		kit.Dialog(c, &open, "Create tag", "Close", busy, func() {
			kit.TagNameField(c, &name, "Name", busy)
		})
	}, 420, 360)
	pressClose := func() {
		t.Helper()
		bounds, ok := tester.Find("Close")
		if !ok || bounds.W < 24 || bounds.H < 24 {
			t.Fatalf("close button has no usable hit area: %+v", bounds)
		}
		x, y := bounds.X+bounds.W/2, bounds.Y+bounds.H/2
		tester.Press(x, y)
		tester.Release(x, y)
	}
	pressClose()
	if !open {
		t.Fatal("busy tag dialog closed")
	}
	busy = false
	tester.Frame()
	if !tester.HasText("Name") {
		t.Fatal("tag name field did not render")
	}
	pressClose()
	if open {
		t.Fatal("close button did not dismiss the tag dialog")
	}
}
