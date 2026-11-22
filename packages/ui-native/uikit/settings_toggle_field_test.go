package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
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

func TestNarrowSettingsToggleLetsTheLabelAndHelpUseTheWholeRow(t *testing.T) {
	enabled := true
	const label = "Move pasted entries to top"
	const help = "Setting help: " + label
	const description = "After pasting, move the item to the top of the list."
	tester := ui.NewTester(func(c *ui.Context) {
		(Kit{}).SettingsToggleField(c, &enabled, label, help, description, false)
	}, 150, 180)
	bounds, visible := tester.Find(label)
	if !visible || bounds.W < 149 {
		t.Fatalf("the switch squeezed the label in the narrow field: %+v", bounds)
	}
	if err := tester.Click(help); err != nil {
		t.Fatal(err)
	}
	descriptionBounds, visible := tester.Find(description)
	if !visible || descriptionBounds.W < 149 || !enabled {
		t.Fatalf("expanding help squeezed the description or changed the switch: %+v, enabled=%t", descriptionBounds, enabled)
	}
}

func TestSettingsCardUsesItsContentRowSpacing(t *testing.T) {
	for _, gap := range []float32{0, 6} {
		kit := Kit{widgets: map[string]theme.WidgetStyle{
			"settingsGroupContent": {Opacity: 1, RowGap: gap},
		}}
		tester := ui.NewTester(func(c *ui.Context) {
			kit.SettingsSectionCard(c, "General settings", nil, func() {
				first, second := true, false
				kit.SettingsToggleField(c, &first, "Start hidden", "", "", false)
				kit.SettingsToggleField(c, &second, "Open near pointer", "", "", false)
			})
		}, 330, 200)
		first, foundFirst := tester.Find("Start hidden")
		second, foundSecond := tester.Find("Open near pointer")
		want := kit.style("settingsToggleRow").Height + gap
		if distance := second.Y - first.Y; !foundFirst || !foundSecond || distance < want-0.5 || distance > want+0.5 {
			t.Errorf("row gap %.0f: labels at %.1f and %.1f, want %.1f apart", gap, first.Y, second.Y, want)
		}
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
