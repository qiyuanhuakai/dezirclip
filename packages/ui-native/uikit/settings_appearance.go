package uikit

import "github.com/egoist/mygo/ui"

type ThemeManagerOptions struct {
	Label, BuiltinLabel, ImportedLabel    string
	Active                                string
	Builtins, Imported                    []SelectOption
	ImportLabel, ExportLabel, RemoveLabel string
	LimitMessage                          string
	Disabled, ImportDisabled              bool
	OnChange                              func(string)
	OnImport, OnExport, OnRemove          func()
}

// SettingsThemeManager groups preset and imported themes under the same themed controls.
func (k Kit) SettingsThemeManager(c *ui.Context, width float32, options ThemeManagerOptions) {
	ui.Column(c).FillWidth().Gap(12).Label(options.Label).Children(func() {
		ui.Row(c).FillWidth().Wrap().AlignItems(ui.Center).Gap(8).Children(func() {
			labelWidth := settingsTextWidth(c, options.Label, k.style("settingsLabel"))
			k.SettingsLabel(c, options.Label).Basis(labelWidth).Grow(1).Shrink(0).MaxWidthPercent(100).MinWidth(0).Wrap()
			if k.appearanceAction(c, options.ImportLabel, options.Disabled || options.ImportDisabled, false).Clicked() && options.OnImport != nil {
				options.OnImport()
			}
		})
		if len(options.Builtins) > 0 {
			k.themeChoices(c, width, options.BuiltinLabel, options.Active, options.Builtins, options.Disabled, options.OnChange)
		}
		if len(options.Imported) > 0 {
			k.themeChoices(c, width, options.ImportedLabel, options.Active, options.Imported, options.Disabled, options.OnChange)
		}
		for _, option := range options.Imported {
			if options.Active == option.Value {
				ui.Row(c).FillWidth().Wrap().AlignItems(ui.Center).Gap(8).Label(option.Label).Children(func() {
					if k.appearanceAction(c, options.ExportLabel, options.Disabled, false).Clicked() && options.OnExport != nil {
						options.OnExport()
					}
					if k.SettingsRemoveButton(c, options.RemoveLabel, false, options.Disabled).Height(36).Clicked() && options.OnRemove != nil {
						options.OnRemove()
					}
				})
				break
			}
		}
		if options.LimitMessage != "" {
			ui.Text(c, options.LimitMessage).Wrap()
		}
	})
}

func (k Kit) themeChoices(c *ui.Context, width float32, label, active string, options []SelectOption, disabled bool, onChange func(string)) {
	const gap, minWidth = 8, 110
	columns := min(len(options), max(1, int(width+gap)/(minWidth+gap)))
	ui.Grid(c).FillWidth().Columns(columns).Gap(gap).Role(ui.RoleGroup).Label(label).Children(func() {
		for _, option := range options {
			k.settingsChoiceButton(c, "settingsChoiceCard", active, option, false, disabled, onChange)
		}
	})
}

type SettingsBackgroundOptions struct {
	Label, ChooseLabel, ClearLabel, Filename string
	Disabled                                 bool
	OnChoose, OnClear                        func()
}

func (k Kit) SettingsBackground(c *ui.Context, options SettingsBackgroundOptions, opacity func()) {
	ui.Column(c).FillWidth().Gap(6).Padding(6, 0, 0).Children(func() {
		ui.Row(c).FillWidth().Gap(8).Wrap().AlignItems(ui.Center).Children(func() {
			k.SettingsLabel(c, options.Label).Grow(1).MinWidth(0).Wrap()
			if k.appearanceAction(c, options.ChooseLabel, options.Disabled, true).Clicked() && options.OnChoose != nil {
				options.OnChoose()
			}
			if options.Filename != "" && k.SettingsRemoveButton(c, options.ClearLabel, true, options.Disabled).Clicked() && options.OnClear != nil {
				options.OnClear()
			}
		})
		if options.Filename != "" {
			ui.Text(c, options.Filename).Margin(4, 0, 0).FontSize(11).TextColor(c.Theme().TextMuted).Wrap()
			if opacity != nil {
				ui.Column(c).FillWidth().Margin(12, 0, 0).Children(opacity)
			}
		}
	})
}

func (k Kit) appearanceAction(c *ui.Context, label string, disabled, compact bool) ui.Element {
	if compact {
		return k.settingsAction(c, label, disabled, 28, &[4]float32{4, 8, 4, 8}, nil)
	}
	return k.settingsAction(c, label, disabled, 36, nil, nil)
}

func (k Kit) settingsAction(c *ui.Context, label string, disabled bool, height float32, padding *[4]float32, glyph *ui.SVG) ui.Element {
	button := ui.ButtonBase(c).Label(label).Disabled(disabled).FocusRing(true).Center().Cursor(actionCursor(disabled))
	widget := "updateAction"
	if disabled {
		widget += "Disabled"
	} else if button.Hovered() {
		widget += "Hover"
	}
	style := k.style(widget)
	if disabled {
		// ButtonBase already reduces the opacity of disabled controls.
		style.Opacity = 1
	}
	if height > 0 {
		style.Height = height
	}
	if padding != nil {
		style.Padding = *padding
	}
	paint(button.Height(style.Height).Gap(style.ColumnGap), k.animatePaint(button, style)).Children(func() {
		if glyph != nil {
			ui.Icon(c, glyph).Size(14, 14).TextColor(styleColor(style.Text)).Shrink(0).PassThrough()
		}
		if label != "" {
			text(c, label, style).SingleLine().PassThrough()
		}
	})
	return button
}
