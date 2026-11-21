package uikit

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

// ApplyAppearance sets the theme used by the native components in this frame.
func ApplyAppearance(c *ui.Context, palette theme.Tokens, dark bool) {
	appearance := ui.LightTheme()
	if dark {
		appearance = ui.DarkTheme()
	}
	theme.Apply(appearance, palette)
	appearance.Focus = appearance.Accent
	appearance.SurfaceHover = appearance.Selection
	appearance.SurfacePressed = appearance.Selection
	c.SetTheme(appearance)
}

type ButtonOptions struct {
	Label    string
	Primary  bool
	Selected bool
	Disabled bool
}

func Button(c *ui.Context, options ButtonOptions) ui.Element {
	return Kit{}.Button(c, options)
}

func (k Kit) Button(c *ui.Context, options ButtonOptions) ui.Element {
	palette := c.Theme()
	control := ui.ButtonBase(c).Label(options.Label).Height(32).PaddingX(12).
		Radius(palette.Radius).FocusRing(true).Disabled(options.Disabled)
	fill, text := palette.Surface, palette.Text
	switch {
	case options.Primary:
		fill, text = palette.Accent, palette.AccentText
		if control.Pressed() {
			fill = palette.AccentPressed
		} else if control.Hovered() {
			fill = palette.AccentHover
		}
	case options.Selected:
		fill = palette.Selection
	case control.Pressed():
		fill = palette.SurfacePressed
	case control.Hovered():
		fill = palette.SurfaceHover
	}
	if options.Disabled {
		text = palette.TextMuted
	}
	style := k.style("confirmCancel")
	style.Fill, style.Text = rgba(fill), rgba(text)
	style = k.interactive(c, control, style, options.Selected)
	paint(control, style)
	control.Children(func() { styledText(ui.Text(c, options.Label).PassThrough(), style) })
	return control
}

type SelectOption struct {
	Value, Label string
	// FontFamily previews a font in a searchable select without styling the query.
	FontFamily string
	Disabled   bool
}

func (k Kit) Select(c *ui.Context, selected *string, label string, options []SelectOption, disabled bool) ui.Element {
	name := label
	for _, option := range options {
		if option.Value == *selected {
			name = option.Label
			break
		}
	}
	if disabled {
		// The platform dims disabled widgets by default. Keep the disabled
		// control's semantics and paint the explicitly styled face next to it.
		style := k.style("selectDisabled")
		var trigger ui.Element
		paint(ui.Box(c).Size(style.Width, style.Height), style).Children(func() {
			trigger = ui.SelectBase(c, selected).Trigger.Label(label).Disabled(true).Fill()
			face := ui.Row(c).Absolute().Top(0).Right(0).Bottom(0).Left(0).AlignItems(ui.Center).PassThrough()
			face.Children(func() { k.selectFace(c, name, style) })
		})
		return trigger
	}
	parts := ui.SelectBase(c, selected)
	trigger := parts.Trigger.Label(label)
	style := k.style("select")
	switch {
	case trigger.Focused() || parts.Open():
		style = k.style("selectFocus")
	case trigger.Hovered():
		style = k.style("selectHover")
	}
	style = k.animatePaint(trigger, style)
	paint(trigger.Width(style.Width).MaxWidthPercent(100).Height(style.Height).FocusRing(false), style)
	trigger.Children(func() { k.selectFace(c, name, style) })
	if parts.Open() {
		parts.Popup(func(panel ui.Element) {
			menu := k.style("selectMenu")
			list := k.style("selectMenuList")
			paint(panel.Width(menu.Width).Translate(0, menu.MarginTop), menu)
			ui.Column(c).Padding(list.Padding[:]...).Children(func() {
				for _, option := range options {
					if option.Disabled {
						style := k.style("selectOptionDisabled")
						paint(ui.Row(c).Height(style.Height).AlignItems(ui.Center), style).
							Children(func() { text(c, option.Label, style) })
						continue
					}
					style := k.style("selectOption")
					item := parts.Item(option.Value).Label(option.Label)
					if option.Value == *selected {
						style = k.style("selectSelected")
					} else if item.Hovered() {
						style.Fill = k.style("selectSelected").Fill
					}
					paint(item.Height(style.Height), k.interactive(c, item, style, option.Value == *selected))
					item.Children(func() { text(c, option.Label, style) })
				}
			})
		})
	}
	return trigger
}

func (k Kit) selectFace(c *ui.Context, name string, style theme.WidgetStyle) {
	ui.Row(c).Grow(1).PaddingX(10).AlignItems(ui.Center).Children(func() { text(c, name, style).Grow(1) })
	indicator := k.style("selectIndicator")
	paint(ui.Row(c).Size(indicator.Width, indicator.Height).Center(), indicator).Children(func() {
		ui.Icon(c, chevronDown).Size(20, 20).TextColor(styleColor(indicator.Text))
	})
}

func SearchField(c *ui.Context, value *string, label, placeholder string) ui.Element {
	palette := c.Theme()
	return ui.TextInputBase(c, value).Label(label).Placeholder(placeholder).Height(36).Grow(1).
		Radius(palette.Radius).PaddingX(10).Background(palette.Surface).Border(1, palette.Border).FocusRing(true)
}

type FilterOption struct {
	ID    string
	Label string
	Count int
}

func (k Kit) Filters(c *ui.Context, options []FilterOption, active map[string]bool, allLabel, clearLabel string) {
	ui.Row(c).Gap(6).Wrap().Children(func() {
		if k.button(c, allLabel, "filterAction").Clicked() {
			for _, option := range options {
				active[option.ID] = true
			}
		}
		for _, option := range options {
			label := option.Label
			if option.Count >= 0 {
				label = fmt.Sprintf("%s %d", label, option.Count)
			}
			widget := "filter"
			if active[option.ID] {
				widget = "filterSelected"
			}
			if k.button(c, label, widget).Clicked() {
				active[option.ID] = !active[option.ID]
			}
		}
		if k.button(c, clearLabel, "filterAction").Clicked() {
			clear(active)
		}
	})
}

func (k Kit) MatchHighlight(c *ui.Context, text, query string) {
	if spans := k.matchSpans(text, query); spans != nil {
		styledText(ui.RichText(c, spans...), k.style("match"))
	} else {
		ui.Text(c, text)
	}
}

func (k Kit) matchSpans(text, query string) []ui.Span {
	needle := []rune(strings.ToLower(query))
	if len(needle) == 0 {
		return nil
	}
	source, lower := []rune(text), []rune(strings.ToLower(text))
	mark := k.style("mark")
	var spans []ui.Span
	last := 0
	for start := 0; start+len(needle) <= len(lower); start++ {
		if !slices.Equal(lower[start:start+len(needle)], needle) {
			continue
		}
		if start < last {
			previous := len(spans) - 1
			spans[previous].Text += string(source[last : start+len(needle)])
		} else {
			if start > last {
				spans = append(spans, ui.Span{Text: string(source[last:start])})
			}
			spans = append(spans, ui.Span{Text: string(source[start : start+len(needle)]),
				Background: styleColor(mark.Fill), Color: styleColor(mark.Text), Radius: mark.Radius})
		}
		last = start + len(needle)
	}
	if last < len(source) {
		spans = append(spans, ui.Span{Text: string(source[last:])})
	}
	return spans
}

func (k Kit) SearchHistory(c *ui.Context, entries []string, emptyLabel string, onSelect func(string)) {
	panel := k.style("history")
	paint(ui.Column(c).Width(panel.Width), panel).Margin(panel.Top, 0, 0, 0).Children(func() {
		if len(entries) == 0 {
			style := k.style("historyEmpty")
			paint(ui.Row(c).Center().Height(style.Height), style).Children(func() { text(c, emptyLabel, style) })
			return
		}
		style := k.style("historyItem")
		symbol := k.style("historyIcon")
		for _, entry := range entries {
			row := paint(ui.ButtonBase(c).Label(entry).Justify(ui.Start).Height(style.Height).Gap(style.ColumnGap), style)
			row.Children(func() {
				ui.Icon(c, historyClock).Size(symbol.Width, symbol.Height).TextColor(styleColor(symbol.Text)).Opacity(symbol.Opacity)
				text(c, entry, style)
			})
			if row.Clicked() {
				onSelect(entry)
			}
		}
	})
}

func (k Kit) SettingsDisclosure(c *ui.Context, title string, open *bool, content func()) {
	style := k.style("disclosure")
	ui.Column(c).Children(func() {
		arrow := chevronRight
		if *open {
			arrow = chevronDown
		}
		row := ui.ButtonBase(c).Label(title).FillWidth().Height(style.Height).Justify(ui.Start).Gap(8).FocusRing(true)
		paint(row, style)
		row.Children(func() {
			chevron := k.style("disclosureChevron")
			paint(ui.Row(c).Size(chevron.Width, chevron.Height).Center(), chevron).Children(func() {
				ui.Icon(c, arrow).Size(14, 14).TextColor(styleColor(chevron.Text))
			})
			text(c, title, style)
		})
		if row.Clicked() {
			*open = !*open
		}
		if *open {
			content()
		}
	})
}

func (k Kit) SettingHelp(c *ui.Context, label string, open *bool, help string) {
	ui.Column(c).Gap(4).Children(func() {
		k.settingsHelpButton(c, label, open)
		if *open {
			text(c, help, k.style("helpText")).LineHeight(1.5)
		}
	})
}

func (k Kit) settingsHelpButton(c *ui.Context, label string, open *bool) {
	style := k.style("help")
	control := ui.ButtonBase(c).Label(label).Expanded(*open).FocusRing(true).Shrink(0)
	if control.Hovered() {
		style = k.style("helpActive")
	}
	paint(control.Size(style.Width, style.Height).AlignSelf(ui.Start).Center(), style).Children(func() {
		ui.Icon(c, circleHelp).Size(15, 15).TextColor(styleColor(style.Text)).PassThrough()
	})
	if control.Clicked() {
		*open = !*open
	}
}

func (k Kit) ProgressToast(c *ui.Context, message string, progress float64, status string, dismiss func()) {
	style := k.style("progress")
	switch status {
	case "error":
		style = k.style("progressError")
		progress = 0
	case "done":
		style = k.style("progressDone")
		progress = 1
	}
	progress = min(1, max(0, progress))
	ui.Overlay(c, func() {
		card := paint(ui.Column(c).Absolute().Top(60).Right(16).Width(style.Width).Height(style.Height).Gap(6), style)
		card.Children(func() {
			ui.Row(c).AlignItems(ui.Center).Gap(8).Children(func() {
				text(c, message, k.style("progressMessage")).Grow(1)
				switch status {
				case "error":
					if k.button(c, "×", "progressClose").Label("Dismiss").Clicked() {
						dismiss()
					}
				case "done":
					text(c, "✓", k.style("progressStatus"))
				default:
					text(c, fmt.Sprintf("%.0f%%", progress*100), k.style("progressLabel"))
				}
			})
			track := k.style("progressTrack")
			ui.Box(c).Height(track.Height).Background(styleColor(track.Fill)).Radius(track.Radius).Draw(func(p *ui.Painter, r ui.Rect) {
				if progress > 0 {
					r.W *= float32(progress)
					p.Fill(r, styleColor(k.style("progressFill").Fill), track.Radius)
				}
			})
		})
	})
}

func (k Kit) Toast(c *ui.Context, message string) {
	style := k.style("toast")
	width, _ := c.Size()
	ui.Overlay(c, func() {
		paint(ui.Row(c).Absolute().Top(60).Left((width-style.Width)/2).Width(style.Width).Height(style.Height).Center(), style).
			Children(func() { text(c, message, style) })
	})
}

func (k Kit) Dialog(c *ui.Context, open *bool, title, closeLabel string, busy bool, body func()) {
	k.dialog(c, open, title, closeLabel, busy, "tag", body)
}

// TagNameField renders a labeled name editor in a tag dialog.
func (k Kit) TagNameField(c *ui.Context, value *string, label string, readOnly bool) ui.Element {
	field, labelStyle, style := k.style("tagField"), k.style("tagFieldLabel"), k.style("tagInput")
	var input ui.Element
	ui.Column(c).FillWidth().Gap(field.RowGap).Margin(0, 0, field.MarginBottom).Children(func() {
		text(c, label, labelStyle)
		input = ui.TextInputBase(c, value).Label(label).ReadOnly(readOnly).FillWidth().Height(style.Height)
		styledText(paintTextField(input, style), style)
	})
	return input
}

func (k Kit) dialog(c *ui.Context, open *bool, title, closeLabel string, busy bool, widget string, body func()) {
	style := k.style(widget)
	ui.DialogBase(c, open, func(backdrop, panel ui.Element) {
		backdrop.Background(styleColor(k.style(widget + "Backdrop").Fill))
		width, _ := c.Size()
		paint(panel.Label(title).Width(min(width-24, style.Width)).MinHeight(style.Height), style)
		if busy && panel.Dismissed() {
			*open = true
		}
		ui.Column(c).Gap(12).Children(func() {
			if closeLabel == "" {
				ui.Row(c).AlignItems(ui.Center).Children(func() {
					text(c, title, k.style(widget+"Title")).Grow(1)
				})
			} else {
				k.tagHeading(c, open, title, closeLabel, busy)
			}
			body()
		})
	})
}

func (k Kit) tagHeading(c *ui.Context, open *bool, title, label string, busy bool) {
	heading := k.style("tagHeading")
	ui.Row(c).FillWidth().MinHeight(heading.Height).AlignItems(ui.Start).Gap(heading.ColumnGap).Children(func() {
		text(c, title, k.style("tagTitle")).Grow(1)
		style, symbol := k.style("tagClose"), k.style("tagCloseIcon")
		button := paint(ui.ButtonBase(c).Label(label).Tooltip(label).Size(style.Width, style.Height).Center().Disabled(busy), style)
		button.Children(func() {
			ui.Icon(c, closeIcon).Size(symbol.Width, symbol.Height).TextColor(styleColor(symbol.Text)).PassThrough()
		})
		if button.Clicked() {
			*open = false
		}
	})
}

func (k Kit) ConfirmDialog(c *ui.Context, open *bool, title, message, cancelLabel, confirmLabel string, confirm func()) {
	k.dialog(c, open, title, "", false, "confirm", func() {
		text(c, message, k.style("confirmMessage"))
		ui.Box(c).Height(0)
		ui.Row(c).Gap(12).Children(func() {
			ui.Spacer(c)
			if k.button(c, cancelLabel, "confirmCancel").Clicked() {
				*open = false
			}
			if k.button(c, confirmLabel, "confirmPrimary").Clicked() {
				*open = false
				confirm()
			}
		})
	})
}
