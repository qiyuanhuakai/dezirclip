package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSettingsToggleFieldDoesNotChangeTheValueWhenHelpOpens(t *testing.T) {
	enabled, changes := true, 0
	const label = "Move pasted entries to top"
	const help = "Setting help: " + label
	const description = "After pasting, move the item to the top of the list."
	tester := ui.NewTester(func(c *ui.Context) {
		if (Kit{}).SettingsToggleField(c, &enabled, label, help, description, false) {
			changes++
		}
	}, 350, 120)
	if tester.HasText(description) {
		t.Fatal("description opened without interaction")
	}
	if err := tester.Click(help); err != nil || !tester.HasText(description) || !enabled || changes != 0 {
		t.Fatalf("opening help changed the switch: enabled=%t changes=%d err=%v", enabled, changes, err)
	}
	if err := tester.Click(help); err != nil || tester.HasText(description) || !enabled || changes != 0 {
		t.Fatalf("closing help changed the switch: enabled=%t changes=%d err=%v", enabled, changes, err)
	}
	tester.ClickAt(315, 18)
	if enabled || changes != 1 {
		t.Fatalf("switch click set enabled=%t changes=%d", enabled, changes)
	}
}
