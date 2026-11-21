package uikit

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSettingsChoicesFitAndKeepSelectionInNarrowFields(t *testing.T) {
	active, disabled := "preview", false
	var changes []string
	options := []SelectOption{
		{Value: "stable", Label: "Stable releases"},
		{Value: "preview", Label: "Preview releases"},
	}
	view := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Width(112).Children(func() {
			Kit{}.SettingsChoices(c, "Update channel", active, options, true, disabled, func(value string) {
				changes = append(changes, value)
				active = value
			})
		})
	}, 160, 140)
	left, leftVisible := view.Find(options[0].Label)
	right, rightVisible := view.Find(options[1].Label)
	if !leftVisible || !rightVisible || left.W <= 0 || right.W <= 0 || right.X+right.W > 112 || left.Y != right.Y {
		t.Fatalf("segments must stay side by side within the field: %+v and %+v", left, right)
	}
	for _, label := range []string{options[1].Label, options[0].Label, options[0].Label} {
		if err := view.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	disabled = true
	view.Frame()
	view.ClickAt(right.X+right.W/2, right.Y+right.H/2)
	if !slices.Equal(changes, []string{"stable"}) || active != "stable" {
		t.Fatalf("active or disabled segments triggered an extra change: %v", changes)
	}
}

func TestSettingsChoiceCardsWrapAtTheirLabelWidths(t *testing.T) {
	const system = "Follow system appearance"
	view := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Width(168).Children(func() {
			Kit{}.SettingsChoices(c, "Color mode", "light", []SelectOption{
				{Value: "system", Label: system}, {Value: "light", Label: "Light"}, {Value: "dark", Label: "Dark"},
			}, false, false, nil)
		})
	}, 190, 150)
	first, firstVisible := view.Find(system)
	light, lightVisible := view.Find("Light")
	dark, darkVisible := view.Find("Dark")
	if !firstVisible || !lightVisible || !darkVisible ||
		first.X+first.W > 168 || first.W < 166 || light.Y <= first.Y || dark.Y != light.Y {
		t.Fatalf("long option should take a full row before the two short cards: %+v, %+v, %+v", first, light, dark)
	}
}

func TestSettingsHotkeyBindingWrapsLongChords(t *testing.T) {
	kit := Kit{}.WithHotkeyPlatform("windows")
	view := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).FillWidth().Padding(8).Children(func() {
			kit.HotkeyRow(c, "Search hotkey", func() {
				kit.HotkeyBinding(c, "Search hotkey", "Ctrl+Alt+Shift+Super+NumpadSubtract", "", false, false)
			})
		})
	}, 230, 180)
	control, visible := view.Find("Search hotkey")
	if !visible || control.X < 0 || control.X+control.W > 230 {
		t.Fatalf("shortcut recorder escaped the narrow settings card: %+v, visible=%t", control, visible)
	}
	first, firstVisible := view.Find("CTRL")
	last, lastVisible := view.Find("NUMPADSUBTRACT")
	if !firstVisible || !lastVisible || last.W < 75 || last.X+last.W > 230 || last.Y <= first.Y {
		t.Fatalf("long shortcut did not wrap its keycaps: first=%+v, last=%+v, visible=%t/%t, texts=%v", first, last, firstVisible, lastVisible, view.Texts())
	}
}

func TestSettingsNumberFieldCommitsAfterFocusLeaves(t *testing.T) {
	value := "30"
	var commits []string
	view := ui.NewTester(func(c *ui.Context) {
		kit := Kit{}
		ui.Column(c).Children(func() {
			field, commit := kit.SettingsNumberField(c, &value, "Idle timeout", false)
			field.Width(112)
			if commit {
				commits = append(commits, value)
			}
			ui.Button(c, "Other setting")
		})
	}, 320, 140)
	if err := view.Click("Idle timeout"); err != nil {
		t.Fatal(err)
	}
	view.Type("60")
	if value != "60" || len(commits) != 0 {
		t.Fatalf("expected to replace the existing value without committing, got %q and %v", value, commits)
	}
	if err := view.Click("Other setting"); err != nil {
		t.Fatal(err)
	}
	if len(commits) != 1 || commits[0] != "60" {
		t.Fatalf("expected one commit with the edited value, got %v", commits)
	}
}
