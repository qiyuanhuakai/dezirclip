package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

var themes = []string{"retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"}

func TestBuiltInPaletteAndWidgetStyles(t *testing.T) {
	for _, name := range themes {
		for _, mode := range []string{"light", "dark"} {
			t.Run(name+"/"+mode, func(t *testing.T) {
				appearance := theme.Appearance{Mode: mode, Opacity: 1, Tokens: theme.Builtin(name, mode)}
				if !appearance.Valid() {
					t.Fatal("invalid built-in appearance")
				}
				for _, widget := range []string{"select", "filter", "searchInput", "help", "disclosure", "progress", "toast", "confirm", "tag"} {
					style := theme.BuiltinWidgetStyle(name, mode, widget)
					if style.Width <= 0 || style.Height <= 0 || style.Opacity <= 0 {
						t.Fatalf("missing computed geometry for %s: %+v", widget, style)
					}
				}
			})
		}
	}
}

func TestSelectReportsChoiceAndRejectsDisabledOption(t *testing.T) {
	kit := Kit{Name: "mica", Mode: "light"}
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
	kit := Kit{Name: "retro", Mode: "light"}
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
	kit := Kit{Name: "acrylic", Mode: "dark"}
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
