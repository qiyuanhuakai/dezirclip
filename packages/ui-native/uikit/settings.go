package uikit

import "github.com/egoist/mygo/ui"

type SettingsSection struct {
	ID    string
	Label string
}

func (k Kit) SettingsDataPath(c *ui.Context, label, path string, actions func()) {
	block, header := k.style("settingsDataBlock"), k.style("settingsDataHeader")
	paint(ui.Column(c).FillWidth().Gap(block.RowGap+header.MarginBottom), block).Children(func() {
		paint(ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(max(8, header.ColumnGap)).Wrap(), header).Children(func() {
			text(c, label, k.style("settingsDataLabel")).Grow(1).Wrap()
			if actions != nil {
				actions()
			}
		})
		field := ui.Text(c, path).FillWidth().MinWidth(0).SingleLine()
		widget := "settingsDataPath"
		if field.Hovered() {
			widget = "settingsDataPathHover"
		}
		style := k.style(widget)
		if field.Hovered() {
			width, _ := c.MeasureText(0, ui.Span{Text: path, Font: style.Font, Size: style.Size, Weight: style.Weight,
				LetterSpacing: style.LetterSpacing})
			available := field.Bounds().W - style.Padding[1] - style.Padding[3] - horizontalBorders(style)
			if available > 0 && width > available {
				field.Tooltip(path)
			}
		}
		paint(styledText(field, style), k.animatePaint(field, style))
	})
}

func (k Kit) SettingsDataActions(c *ui.Context, content func()) {
	style := k.style("settingsDataActions")
	paint(ui.Row(c).FillWidth().Gap(style.ColumnGap).Wrap().AlignItems(ui.Center), style).
		Margin(style.Margin[:]...).Children(content)
}

func (k Kit) SettingsDataAction(c *ui.Context, label, action string, disabled bool) ui.Element {
	style := k.style("settingsDataAction")
	button := ui.ButtonBase(c).Label(label).Tooltip(label).Disabled(disabled).
		Height(style.Height).AlignItems(ui.Center).Gap(6)
	style = k.interactive(c, button, style, false)
	paint(button, style).Children(func() {
		var symbol *ui.SVG
		switch action {
		case "export":
			symbol = settingsExportIcon
		case "import":
			symbol = settingsImportIcon
		}
		if symbol != nil {
			ui.Icon(c, symbol).Size(14, 14).TextColor(styleColor(style.Text)).PassThrough()
		}
		text(c, label, style).SingleLine().PassThrough()
	})
	return button
}

var settingsExportIcon, settingsImportIcon = icon("Download"), icon("Upload")

func (k Kit) SettingsRemoveButton(c *ui.Context, label string, iconOnly, disabled bool) ui.Element {
	widget := "settingsRemove"
	if iconOnly {
		widget = "settingsRemoveIcon"
	}
	style := k.style(widget)
	button := ui.ButtonBase(c).Label(label).Tooltip(label).Height(style.Height).Disabled(disabled).
		AlignItems(ui.Center).Gap(5)
	if iconOnly {
		button.Width(style.Width).Center()
	}
	paint(button, k.interactive(c, button, style, false)).Children(func() {
		ui.Icon(c, headerIcons["clear"]).Size(14, 14).TextColor(styleColor(style.Text)).PassThrough()
		if !iconOnly {
			text(c, label, style).SingleLine().PassThrough()
		}
	})
	return button
}

// SettingsSectionCard paints an accordion card, or a fixed section when open is nil.
func (k Kit) SettingsSectionCard(c *ui.Context, title string, open *bool, content func()) {
	k.settingsSectionCard(c, title, open, 1, content)
}

// SettingsSectionGrid paints an open settings card with responsive field columns.
// Fields can span the card by setting ColumnSpan(-1) on their outer element.
func (k Kit) SettingsSectionGrid(c *ui.Context, title string, viewportWidth float32, content func()) {
	columns := 1
	switch {
	case viewportWidth >= 1000:
		columns = 3
	case viewportWidth >= 700:
		columns = 2
	}
	k.settingsSectionCard(c, title, nil, columns, content)
}

func (k Kit) settingsSectionCard(c *ui.Context, title string, open *bool, columns int, content func()) {
	expanded := open == nil || *open
	suffix := ""
	if expanded {
		suffix = "Open"
	}
	surface, header := k.style("settingsGroup"+suffix), k.style("settingsGroupHeader"+suffix)
	paint(ui.Column(c).FillWidth(), surface).Children(func() {
		var row ui.Element
		if open == nil {
			row = ui.Row(c)
		} else {
			row = ui.ButtonBase(c).Label(title).Expanded(expanded).FocusRing(true)
			header = k.interactive(c, row, header, expanded)
		}
		paint(row.FillWidth().Height(header.Height).AlignItems(ui.Center).Gap(8), header).Children(func() {
			text(c, title, header).Grow(1).Wrap().Translate(0, header.TranslateY).PassThrough()
			if open != nil {
				arrow := chevronRight
				if expanded {
					arrow = chevronDown
				}
				glyph := k.style("settingsGroupChevron")
				ui.Icon(c, arrow).Size(16, 16).Shrink(0).TextColor(styleColor(glyph.Text)).Opacity(glyph.Opacity).PassThrough()
			}
		})
		if open != nil && row.Clicked() {
			*open = !*open
		}
		if expanded {
			body := ui.Column(c)
			if columns > 1 {
				body.Grid().Columns(columns).AlignItems(ui.Start).GapX(20)
			}
			paint(body.FillWidth().GapY(8), k.style("settingsGroupContent")).Children(content)
		}
	})
}

func (k Kit) SettingsChoices(c *ui.Context, label, active string, options []SelectOption, segmented, disabled bool, selectValue func(string)) {
	container, widget := "settingsChoiceCards", "settingsChoiceCard"
	if segmented {
		container, widget = "settingsSegments", "settingsSegment"
	}
	style := k.style(container)
	var row ui.Element
	if !segmented && len(options) > 4 {
		columns := 3
		if width, _ := c.Size(); width < 380 {
			columns = 2
		}
		row = ui.Grid(c).Columns(columns)
	} else {
		row = ui.Row(c).Wrap().AlignItems(ui.Center)
	}
	row.Role(ui.RoleGroup).Label(label).Gap(style.ColumnGap)
	if !segmented {
		row.FillWidth()
	}
	paint(row, style).Children(func() {
		for _, option := range options {
			selected := option.Value == active
			button := ui.ButtonBase(c).Label(option.Label).Selected(selected).Disabled(disabled || option.Disabled).Center().FocusRing(true)
			buttonStyle := k.style(widget)
			if selected {
				buttonStyle = k.style(widget + "Active")
			} else if button.Hovered() {
				buttonStyle = k.style(widget + "Hover")
			}
			button.Grow(1)
			if segmented {
				button.Width(0).MinWidth(64)
			}
			paint(button.Height(buttonStyle.Height), k.animatePaint(button, buttonStyle)).
				Children(func() { text(c, option.Label, buttonStyle).SingleLine().PassThrough() })
			if !selected && selectValue != nil && button.Clicked() {
				selectValue(option.Value)
			}
		}
	})
}

// SettingsRail renders the wide settings navigation with application-owned selection.
func (k Kit) SettingsRail(c *ui.Context, label, active string, sections []SettingsSection, onSelect func(string)) {
	rail := k.style("settingsRail")
	paint(ui.Column(c).Width(rail.Width).Gap(4).Label(label), rail).Children(func() {
		for _, section := range sections {
			style := k.style("settingsRailItem")
			if section.ID == active {
				style = k.style("settingsRailActive")
			}
			button := ui.ButtonBase(c).FillWidth().Height(style.Height).Label(section.Label)
			style = k.interactive(c, button, style, section.ID == active)
			paint(button, style)
			button.Children(func() {
				ui.Row(c).FillWidth().Children(func() { text(c, section.Label, style).SingleLine() })
			})
			if onSelect != nil && button.Clicked() {
				onSelect(section.ID)
			}
		}
	})
}

type SettingsFooterOptions struct {
	ProductName   string
	Feedback      string
	Reset         string
	Website       string
	Compact       bool
	ResetDisabled bool
	OnFeedback    func()
	OnReset       func()
	OnWebsite     func()
}

// SettingsFooter displays application information and settings actions.
func (k Kit) SettingsFooter(c *ui.Context, options SettingsFooterOptions) {
	actions := func(gap float32) {
		ui.Row(c).Gap(gap).Wrap().Justify(ui.Center).Children(func() {
			k.settingsFooterCard(c, options.Feedback, icon("MessageSquare"), false, options.OnFeedback)
			k.settingsFooterCard(c, options.Reset, icon("RotateCcw"), options.ResetDisabled, options.OnReset)
		})
	}
	links := func() {
		ui.Row(c).Gap(10).Wrap().Justify(ui.Center).Children(func() {
			k.settingsFooterLink(c, options.Website, nil, options.OnWebsite)
			k.settingsFooterLink(c, "GitHub", icon("Github"), options.OnWebsite)
		})
	}
	version := func() { text(c, options.ProductName, k.style("settingsFooterVersion")) }
	if options.Compact {
		ui.Row(c).FillWidth().Gap(12).Wrap().AlignItems(ui.Center).Justify(ui.SpaceBetween).Children(func() {
			actions(8)
			ui.Row(c).Gap(10).Wrap().AlignItems(ui.Center).Children(func() {
				version()
				links()
			})
		})
		return
	}
	ui.Column(c).FillWidth().Gap(16).AlignItems(ui.Center).Margin(16, 0, 32).Children(func() {
		actions(12)
		ui.Column(c).Gap(4).AlignItems(ui.Center).Children(func() {
			version()
			links()
		})
	})
}

func (k Kit) settingsFooterCard(c *ui.Context, label string, symbol *ui.SVG, disabled bool, run func()) {
	style := k.style("settingsFooterCard")
	button := ui.ButtonBase(c).Height(style.Height).Label(label).Disabled(disabled)
	style = k.interactive(c, button, style, false)
	paint(button, style)
	button.Children(func() {
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Icon(c, symbol).Size(16, 16).TextColor(styleColor(style.Text))
			text(c, label, style).SingleLine()
		})
	})
	if run != nil && button.Clicked() {
		run()
	}
}

func (k Kit) settingsFooterLink(c *ui.Context, label string, symbol *ui.SVG, run func()) {
	style := k.style("settingsFooterLink")
	button := ui.ButtonBase(c).Height(style.Height).Label(label)
	style = k.interactive(c, button, style, false)
	paint(button, style)
	button.Children(func() {
		ui.Row(c).Gap(4).AlignItems(ui.Center).Children(func() {
			if symbol != nil {
				ui.Icon(c, symbol).Size(12, 12).TextColor(styleColor(style.Text))
			}
			text(c, label, style).Underline().SingleLine()
		})
	})
	if run != nil && button.Clicked() {
		run()
	}
}
