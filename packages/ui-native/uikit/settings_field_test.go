package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

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
