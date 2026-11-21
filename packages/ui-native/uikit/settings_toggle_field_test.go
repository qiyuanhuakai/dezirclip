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

func TestPrivacyDetectionSwitchesKeepTheirValuesWhenWrapping(t *testing.T) {
	options := []PrivacyDetectionOption{
		{Value: "phone", Label: "Phone numbers", Checked: true},
		{Value: "identity", Label: "Identity numbers"},
	}
	tester := ui.NewTester(func(c *ui.Context) {
		(Kit{}).PrivacyDetectionOptions(c, options, false, func(value string, checked bool) {
			if value == options[1].Value {
				options[1].Checked = checked
			}
		})
	}, 150, 110)
	phone, foundPhone := tester.Find(options[0].Label)
	identity, foundIdentity := tester.Find(options[1].Label)
	if !foundPhone || !foundIdentity || identity.Y <= phone.Y {
		t.Fatalf("privacy detection controls did not wrap: %+v %+v", phone, identity)
	}
	if err := tester.Click(options[1].Label); err != nil || !options[1].Checked || !options[0].Checked {
		t.Fatalf("wrapped control did not toggle only its setting: %+v, %v", options, err)
	}
}
