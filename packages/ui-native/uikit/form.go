package uikit

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
)

// SettingsToggle preserves checkbox accessibility while drawing a switch.
func (k Kit) SettingsToggle(c *ui.Context, on *bool, label string) ui.Element {
	rowStyle := k.style("settingsToggleRow")
	switchStyle := k.style("settingsSwitch")
	suffix := ""
	if *on {
		suffix = "On"
	}
	trackStyle, thumbStyle := k.style("settingsSwitchTrack"+suffix), k.style("settingsSwitchThumb"+suffix)
	row := paint(ui.CheckboxBase(c, on).Label(label).FillWidth().MinHeight(rowStyle.Height).
		Gap(10).AlignItems(ui.Center).FocusRing(true), rowStyle)
	row.Children(func() {
		k.SettingsLabel(c, label).Grow(1).Wrap().PassThrough()
		track := ui.Box(c).Size(switchStyle.Width, switchStyle.Height).Shrink(0).PassThrough()
		paint(track, k.animatePaint(track, trackStyle)).Children(func() {
			if thumbStyle.Width > 0 && thumbStyle.Height > 0 {
				left := thumbStyle.Left
				if duration := time.Duration(k.palette().TransitionMs) * time.Millisecond; duration > 0 {
					left = track.Animate("switch-position", left, duration)
				}
				paint(ui.Box(c).Absolute().Left(left).Top(thumbStyle.Top).
					Size(thumbStyle.Width, thumbStyle.Height).PassThrough(), thumbStyle)
			}
			for _, name := range []string{"settingsSwitchLeft", "settingsSwitchRight"} {
				face := k.style(name + suffix)
				if face.Width <= 0 || face.Height <= 0 {
					continue
				}
				element := ui.Box(c).Absolute().Size(face.Width, face.Height).Left(face.Left).
					Top(face.Top).Center().PassThrough()
				paint(element, k.animatePaint(element, face)).Children(func() {
					text(c, face.Caption, face).PassThrough()
				})
			}
		})
	})
	return row
}

// SettingsLabel paints the visible label of a settings row.
func (k Kit) SettingsLabel(c *ui.Context, label string) ui.Element {
	return text(c, label, k.style("settingsLabel"))
}

func (k Kit) HotkeyRow(c *ui.Context, label string, controls func()) {
	style := k.style("hotkeyRow")
	ui.Column(c).FillWidth().Padding(style.Margin[:]...).Children(func() {
		row := ui.Row(c).FillWidth().Wrap().Gap(max(6, style.ColumnGap)).AlignItems(ui.Center)
		paint(row, style).Children(func() {
			k.SettingsLabel(c, label).Grow(1).MinWidth(100).Wrap()
			controls()
		})
	})
}

func (k Kit) HotkeyBinding(c *ui.Context, label, value, placeholder string, recording, disabled bool) ui.Element {
	field := ui.ButtonBase(c).Label(label).FocusRing(true).Disabled(disabled)
	return k.hotkeyCaps(c, field, value, placeholder, recording)
}

// HotkeyList displays registered shortcuts as read-only keycaps, with removal
// available only while the list contains more than one binding.
func (k Kit) HotkeyList(c *ui.Context, values []string, placeholder, remove string, disabled bool, onRemove func(string)) {
	ui.Row(c).FillWidth().Wrap().GapX(16).GapY(8).Children(func() {
		if len(values) == 0 {
			k.hotkeyCaps(c, ui.Row(c), "", placeholder, false)
		}
		for _, value := range values {
			ui.Row(c).AlignItems(ui.Center).Gap(8).Children(func() {
				k.hotkeyCaps(c, ui.Row(c).Wrap(), value, "", false)
				if len(values) > 1 && onRemove != nil &&
					k.HotkeyRemoveButton(c, remove+" "+value, disabled).Clicked() {
					onRemove(value)
				}
			})
		}
	})
}

func (k Kit) HotkeyRemoveButton(c *ui.Context, label string, disabled bool) ui.Element {
	style := k.style("hotkeyRemove")
	button := ui.ButtonBase(c).Size(style.Width, style.Height).Label(label).Tooltip(label).Center().Disabled(disabled)
	paint(button, k.interactive(c, button, style, false)).Children(func() {
		ui.Icon(c, shortcutRemoveIcon).Size(14, 14).TextColor(styleColor(style.Text)).PassThrough()
	})
	return button
}

var shortcutRemoveIcon = icon("HotkeyRemove")

func (k Kit) hotkeyCaps(c *ui.Context, field ui.Element, value, placeholder string, recording bool) ui.Element {
	group := k.style("hotkeyGroup")
	cap := k.style("hotkeyCap")
	if recording {
		cap = k.style("hotkeyCapRecording")
		value = ""
	}
	field.Gap(group.ColumnGap).AlignItems(ui.Center)
	paint(field, group).Margin(group.Margin[:]...).Children(func() {
		for _, chord := range strings.Split(value, "+") {
			width := cap.Size * 2.2
			if chord == "" {
				chord, width = placeholder, cap.Size*8
			}
			keycap := ui.Box(c).Height(cap.Height).MinWidth(width).Center().PassThrough()
			paint(keycap, k.animatePaint(keycap, cap)).Margin(cap.Margin[:]...).
				Children(func() { text(c, chord, cap).SingleLine().PassThrough() })
		}
	})
	return field
}

// Checkbox draws the themed control on MyGo's accessible toggle primitive.
func (k Kit) Checkbox(c *ui.Context, checked *bool, label string) ui.Element {
	style := k.style("select")
	row := ui.CheckboxBase(c, checked).Label(label).FillWidth().MinHeight(32).
		AlignItems(ui.Center).Gap(8).FocusRing(true)
	row.Children(func() {
		fill, border := c.Theme().Surface, c.Theme().Border
		if *checked {
			fill, border = c.Theme().Accent, c.Theme().Accent
		} else if row.Hovered() || row.FocusVisible() {
			border = c.Theme().Accent
		}
		mark := ui.Box(c).Size(17, 17).Shrink(0)
		box := style
		box.Fill, box.Border = rgba(fill), rgba(border)
		box.Padding, box.Outline, box.Shadows = [4]float32{}, nil, nil
		box.BorderWidths = nil
		box.BorderWidth, box.Radius = 1, min(5, style.Radius)
		paint(mark, k.animatePaint(mark, box))
		if *checked {
			mark.Children(func() {
				ui.Text(c, "✓").Center().Fill().TextColor(c.Theme().AccentText).FontSize(12).PassThrough()
			})
		}
		text(c, label, style).Grow(1).Wrap().PassThrough()
	})
	return row
}

func (k Kit) TextField(c *ui.Context, value *string, label string) ui.Element {
	return k.styleField(c, ui.TextInputBase(c, value).Label(label), "textField")
}

func (k Kit) NumberField(c *ui.Context, value *string, label string) ui.Element {
	return k.styleField(c, ui.TextInputBase(c, value).Label(label), "numberField").TextAlign(ui.End)
}

func (k Kit) TextArea(c *ui.Context, value *string, label string) ui.Element {
	return k.styleField(c, ui.TextAreaBase(c, value).Label(label), "textField")
}

func (k Kit) styleField(c *ui.Context, field ui.Element, widget string) ui.Element {
	if field.Focused() {
		widget += "Focus"
	} else if widget == "select" && field.Hovered() {
		widget += "Hover"
	}
	style := k.animatePaint(field, k.style(widget))
	return styledText(paintTextField(field.Height(style.Height), style), style).FocusRing(true)
}

func (k Kit) Combo(c *ui.Context, value *string, label, placeholder string, choices []string, disabled bool) (ui.Element, bool) {
	return k.combo(c, value, label, placeholder, choices, disabled, k.style("select").Height)
}

func (k Kit) combo(c *ui.Context, value *string, label, placeholder string, choices []string, disabled bool, height float32) (ui.Element, bool) {
	type draft struct{ text, saved string }
	box := ui.Box(c).Height(height)
	edit := ui.Local(box, "deziro-combobox", func() draft { return draft{text: *value, saved: *value} })
	if edit.saved != *value {
		edit.text, edit.saved = *value, *value
	}
	changed := false
	box.Children(func() {
		parts := ui.ComboboxBase(c, &edit.text)
		indicator := k.style("selectIndicator")
		field := k.styleField(c, parts.Input.Fill().Label(label).Placeholder(placeholder).Disabled(disabled), "select").
			Height(height).PaddingEnd(indicator.Width + 4)
		paint(ui.Row(c).Absolute().Right(3).Top(0).Bottom(0).Width(indicator.Width).Center().PassThrough(), indicator).
			Children(func() {
				ui.Icon(c, chevronDown).Size(16, 16).TextColor(styleColor(indicator.Text)).PassThrough()
			})
		parts.Popup(func(panel ui.Element) {
			menu, list := k.style("selectMenu"), k.style("selectMenuList")
			paint(panel.Translate(0, menu.MarginTop), menu)
			query := strings.ToLower(strings.TrimSpace(edit.text))
			ui.Scroll(c).FillWidth().MaxHeight(215).Children(func() {
				ui.Column(c).FillWidth().Padding(list.Padding[:]...).Children(func() {
					for _, choice := range choices {
						if parts.Filtering() && !strings.Contains(strings.ToLower(choice), query) {
							continue
						}
						item := parts.Item(choice).Label(choice)
						style := k.style("selectOption")
						if item.Highlighted() || choice == *value {
							style = k.style("selectSelected")
						}
						style = k.interactive(c, item, style, item.Highlighted() || choice == *value)
						paint(item.FillWidth().Height(style.Height), style)
						item.Children(func() { text(c, choice, style).SingleLine().PassThrough() })
					}
				})
			})
		})
		if choice, ok := parts.Chosen(); ok && !disabled {
			changed = *value != choice
			*value, edit.text, edit.saved = choice, choice, choice
		} else if !field.Focused() && !parts.Open() {
			edit.text = *value
		}
	})
	return box, changed
}

func (k Kit) Slider(c *ui.Context, value *float64, minimum, maximum float64, label string) ui.Element {
	track, fill, thumb := k.style("settingsRangeTrack"), k.style("settingsRangeFill"), k.style("settingsRangeThumb")
	inset := thumb.Width / 2
	slider := ui.SliderBase(c, value, minimum, maximum).Height(max(20, thumb.Height)).
		PaddingX(inset).Label(label)
	focused := slider.FocusVisible()
	focusStyle := k.style("settingsRangeFocus")
	slider.Draw(func(p *ui.Painter, bounds ui.Rect) {
		fraction := float32(0)
		if maximum > minimum {
			fraction = float32(min(1, max(0, (*value-minimum)/(maximum-minimum))))
		}
		rail := ui.Rect{X: bounds.X, Y: bounds.Y + (bounds.H-track.Height)/2, W: bounds.W, H: track.Height}
		p.Fill(rail, styleColor(track.Fill), track.Radius)
		p.Clip(rail, track.Radius, func() {
			progress := ui.Rect{X: rail.X, Y: rail.Y, W: rail.W * fraction, H: rail.H}
			p.Fill(progress, styleColor(fill.Fill), 0)
		})
		p.Stroke(rail, styleColor(track.Border), track.Radius, track.BorderWidth)
		knob := ui.Rect{X: bounds.X + max(0, bounds.W-thumb.Width)*fraction,
			Y: bounds.Y + (bounds.H-thumb.Height)/2, W: thumb.Width, H: thumb.Height}
		for _, shadow := range thumb.Shadows {
			if !shadow.Inset {
				v := shadow.Values
				p.Shadow(knob, thumb.Radius, v[0], v[1], v[2], v[3], styleColor(shadow.Color))
			}
		}
		p.Fill(knob, styleColor(thumb.Fill), thumb.Radius)
		p.Stroke(knob, styleColor(thumb.Border), thumb.Radius, thumb.BorderWidth)
		if focused {
			if ring := focusStyle.Outline; ring != nil {
				offset := ring.Offset + ring.Width
				r := ui.Rect{X: rail.X - offset, Y: rail.Y - offset, W: rail.W + 2*offset, H: rail.H + 2*offset}
				p.Stroke(r, styleColor(ring.Color), min(track.Radius, rail.H/2)+offset, ring.Width)
			} else {
				p.Stroke(knob, p.Theme().Accent, thumb.Radius, 2)
			}
		}
	})
	return slider
}
