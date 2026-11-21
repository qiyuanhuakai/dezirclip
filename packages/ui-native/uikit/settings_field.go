package uikit

import "github.com/egoist/mygo/ui"

type SettingsFieldOptions struct {
	Stacked bool
	Last    bool
	Detail  bool
}

// SettingsField lays out a setting and its control with the theme's row padding and divider.
func (k Kit) SettingsField(c *ui.Context, options SettingsFieldOptions) ui.Element {
	style := k.style("settingsToggleRow")
	if options.Last {
		style.Padding[2], style.BorderWidth, style.BorderWidths = 0, 0, nil
	}
	var field ui.Element
	if options.Stacked {
		field = ui.Column(c).Gap(6)
	} else {
		field = ui.Row(c).GapX(12).GapY(8).Wrap().AlignItems(ui.Center).Justify(ui.SpaceBetween)
	}
	field = paint(field.AlignSelf(ui.Stretch).MinWidth(0).MinHeight(style.Height), style)
	if options.Detail {
		field.MarginStart(18)
	}
	return field
}

// SettingsFieldLabel shows a setting name and an optional expandable description.
func (k Kit) SettingsFieldLabel(c *ui.Context, label, helpLabel, description string) ui.Element {
	return k.settingsFieldLabel(c, label, helpLabel, description, nil)
}

func (k Kit) settingsFieldLabel(c *ui.Context, label, helpLabel, description string, action func()) ui.Element {
	if description == "" && action == nil {
		return k.SettingsLabel(c, label).MinWidth(0).Wrap()
	}
	column := ui.Column(c).MinWidth(0).Gap(4)
	open := ui.Local(column, "settings-description", func() bool { return false })
	column.Children(func() {
		ui.Row(c).Wrap().MinWidth(0).GapX(6).GapY(4).AlignItems(ui.Center).Children(func() {
			k.SettingsLabel(c, label).MinWidth(0).Wrap()
			k.settingsHelpButton(c, helpLabel, open)
		})
		if *open {
			widget := "helpText"
			if _, themed := k.widgets["settingsFieldHelpText"]; themed {
				widget = "settingsFieldHelpText"
			}
			if description != "" {
				text(c, description, k.style(widget)).FillWidth().MinWidth(0).Wrap()
			}
			if action != nil {
				action()
			}
		}
	})
	return column
}

// SettingsNumberField selects the current value on focus and reports when editing is complete.
func (k Kit) SettingsNumberField(c *ui.Context, value *string, label string, readOnly bool) (ui.Element, bool) {
	input := k.NumberField(c, value, label).ReadOnly(readOnly)
	wasFocused := ui.Local(input, "settings-number-focus", func() bool { return false })
	focused := input.Focused()
	// Let the pointer release finish placing the caret before selecting the value.
	if focused && !*wasFocused && input.Pressed() {
		return input, false
	}
	if focused && !*wasFocused && !readOnly {
		input.SetTextSelection(0, len(*value))
	}
	committed := !readOnly && (input.Submitted() || *wasFocused && !focused)
	*wasFocused = focused
	return input, committed
}
