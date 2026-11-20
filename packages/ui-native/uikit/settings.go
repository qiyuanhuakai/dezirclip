package uikit

import "github.com/egoist/mygo/ui"

type SettingsSection struct {
	ID    string
	Label string
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
