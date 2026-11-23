package uikit

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

type SettingsSection struct {
	ID    string
	Label string
}

const SettingsStackMaxWidth = 420

// SettingsSectionStack centers the narrow layout and applies its inter-card gap.
func SettingsSectionStack(c *ui.Context, content func()) {
	ui.Column(c).FillWidth().AlignItems(ui.Center).Children(func() {
		ui.Column(c).FillWidth().MaxWidth(SettingsStackMaxWidth).Gap(4).Children(content)
	})
}

// SettingsSectionContentWidth accounts for the card frame and its content padding.
func (k Kit) SettingsSectionContentWidth(cardWidth float32) float32 {
	frame := k.style("settingsGroupOpen")
	content := k.style("settingsGroupContent")
	return cardWidth - horizontalBorders(frame) - frame.Padding[1] - frame.Padding[3] - content.Padding[1] - content.Padding[3]
}

func (k Kit) SettingsRailWidth() float32 {
	return k.style("settingsRail").Width
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
	button := ui.ButtonBase(c).Label(label).Disabled(disabled).
		Height(style.Height).AlignItems(ui.Center).Gap(style.ColumnGap).FocusRing(true).Cursor(actionCursor(disabled))
	if !disabled && (button.Hovered() || button.FocusVisible()) {
		style = k.style("settingsDataActionHover")
	}
	style = k.animatePaint(button, style)
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
		AlignItems(ui.Center).Gap(5).Cursor(actionCursor(disabled))
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
	card := ui.Column(c).FillWidth().Margin(surface.MarginTop, 0, surface.MarginBottom, 0)
	duration := time.Duration(k.palette().TransitionMs) * time.Millisecond
	var opening bool
	if open != nil && duration > 0 {
		wasExpanded := ui.Local(card, "settings-section-expanded", func() bool { return expanded })
		opening = expanded && !*wasExpanded
		*wasExpanded = expanded
		card.Transition(ui.ElementTransition{Position: true, Size: true, Duration: duration})
	}
	paint(card, surface).Children(func() {
		var row ui.Element
		if open == nil {
			row = ui.Row(c)
		} else {
			row = ui.ButtonBase(c).Label(title).Expanded(expanded).FocusRing(true).Cursor(ui.CursorPointer)
		}
		paint(row.FillWidth().Height(header.Height).AlignItems(ui.Center).Gap(8), k.animatePaint(row, header)).Children(func() {
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
			body := ui.Column(c.Key("settings-section-content"))
			if open != nil && duration > 0 {
				body.Transition(ui.ElementTransition{Size: true, Duration: duration})
			}
			if columns > 1 {
				body.Grid().Columns(columns).AlignItems(ui.Start).GapX(20)
			}
			contentStyle := k.style("settingsGroupContent")
			paint(body.FillWidth().GapY(contentStyle.RowGap), contentStyle)
			if entrance := contentStyle.Enter; open != nil && duration > 0 && entrance != nil {
				const key = "settings-content-entrance"
				if opening {
					card.Jump(key, 0)
				}
				animation := ui.Animation{Duration: time.Duration(entrance.DurationMs) * time.Millisecond}
				if ease := entrance.Ease; ease != nil {
					animation.Ease = ui.CubicBezier(ease[0], ease[1], ease[2], ease[3])
				}
				remaining := 1 - card.AnimateTo(key, 1, animation)
				body.TransformOrigin(0.5, 0).Translate(0, entrance.Y*remaining).Scale(1, 1+(entrance.ScaleY-1)*remaining).
					Opacity(contentStyle.Opacity * (1 + (entrance.Opacity-1)*remaining))
			}
			body.Children(content)
		}
	})
}

func (k Kit) SettingsChoices(c *ui.Context, label, active string, options []SelectOption, segmented, disabled bool, selectValue func(string)) {
	container, widget := "settingsChoiceCards", "settingsChoiceCard"
	if segmented {
		container, widget = "settingsSegments", "settingsSegment"
	}
	style := k.style(container)
	row := ui.Row(c).AlignItems(ui.Stretch).Role(ui.RoleGroup).Label(label).
		Gap(style.ColumnGap).MinWidth(0).MaxWidthPercent(100)
	if !segmented {
		row.FillWidth().Wrap()
	}
	paint(row, style).Children(func() {
		for _, option := range options {
			k.settingsChoiceButton(c, widget, active, option, !segmented, disabled, selectValue)
		}
	})
}

func (k Kit) settingsChoiceButton(c *ui.Context, widget, active string, option SelectOption, wrapCard, disabled bool, onSelect func(string)) {
	selected := option.Value == active
	button := ui.ButtonBase(c.Key(option.Value)).Label(option.Label).Selected(selected).
		Disabled(disabled || option.Disabled).Center().FocusRing(true).MinWidth(0).MaxWidthPercent(100).Grow(1).Cursor(actionCursor(disabled || option.Disabled))
	if widget == "settingsSegment" {
		normal, selected := k.style(widget), k.style(widget+"Active")
		width := func(style theme.WidgetStyle) float32 {
			return settingsTextWidth(c, option.Label, style) + style.Padding[1] + style.Padding[3] + horizontalBorders(style)
		}
		button.Width(max(64, width(normal), width(selected))).Shrink(1)
	}
	if selected {
		widget += "Active"
	} else if button.Hovered() && !disabled && !option.Disabled {
		widget += "Hover"
	}
	style := k.animatePaint(button, k.style(widget))
	if wrapCard {
		// Use the unwrapped label to choose a row, then allow text to wrap within a full-width card.
		button.Basis(settingsTextWidth(c, option.Label, style) + style.Padding[1] + style.Padding[3] + horizontalBorders(style)).Shrink(0)
	}
	paint(button.MinHeight(style.Height), style).Children(func() {
		text(c, option.Label, style).MinWidth(0).MaxWidthPercent(100).Wrap().TextAlign(ui.Center).PassThrough()
	})
	if !selected && onSelect != nil && button.Clicked() {
		onSelect(option.Value)
	}
}

func settingsTextWidth(c *ui.Context, label string, style theme.WidgetStyle) float32 {
	if style.Uppercase {
		label = strings.ToUpper(label)
	}
	width, _ := c.MeasureText(0, ui.Span{Text: label, Font: style.Font, Size: style.Size,
		Weight: style.Weight, LetterSpacing: style.LetterSpacing})
	return width
}

// SettingsRail renders the wide settings navigation with application-owned selection.
func (k Kit) SettingsRail(c *ui.Context, label, active string, sections []SettingsSection, onSelect func(string)) {
	rail := k.style("settingsRail")
	paint(ui.Column(c).Width(rail.Width).Gap(4).Label(label), rail).Children(func() {
		for _, section := range sections {
			widget := "settingsRailItem"
			if section.ID == active {
				widget = "settingsRailActive"
			}
			style := k.style(widget)
			button := ui.ButtonBase(c).FillWidth().MinHeight(style.Height).Label(section.Label).
				Selected(section.ID == active).Cursor(ui.CursorPointer)
			style = k.buttonInteraction(button, widget, style)
			k.paintTranslated(button, style)
			button.Children(func() {
				ui.Row(c).FillWidth().Children(func() { text(c, section.Label, style).MinWidth(0).Wrap().PassThrough() })
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
	const widget = "settingsFooterCard"
	style := k.style(widget)
	button := ui.ButtonBase(c).Height(style.Height).Label(label).Disabled(disabled).Cursor(actionCursor(disabled))
	style = k.buttonInteraction(button, widget, style)
	k.paintTranslated(button, style)
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
	const widget = "settingsFooterLink"
	style := k.style(widget)
	button := ui.ButtonBase(c).Height(style.Height).Label(label).Cursor(ui.CursorPointer)
	style = k.buttonInteraction(button, widget, style)
	style.Opacity = button.Animate("footer-link-opacity", style.Opacity, 120*time.Millisecond)
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
