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

func (k Kit) HotkeyBinding(c *ui.Context, label, value, placeholder string, recording, disabled bool) ui.Element {
	group := k.style("hotkeyGroup")
	cap := k.style("hotkeyCap")
	if recording {
		cap = k.style("hotkeyCapRecording")
		value = ""
	}
	field := ui.ButtonBase(c).Label(label).Gap(group.ColumnGap).AlignItems(ui.Center).
		FocusRing(true).Disabled(disabled)
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
	return k.styleField(c, ui.TextInputBase(c, value).Label(label))
}

func (k Kit) TextArea(c *ui.Context, value *string, label string) ui.Element {
	return k.styleField(c, ui.TextAreaBase(c, value).Label(label))
}

func (k Kit) styleField(c *ui.Context, field ui.Element) ui.Element {
	style := k.style("select")
	if field.Focused() {
		style = k.style("selectFocus")
	} else if field.Hovered() {
		style = k.style("selectHover")
	}
	style = k.animatePaint(field, style)
	return styledText(paint(field.Height(style.Height), style), style).FocusRing(true)
}

func (k Kit) Combo(c *ui.Context, value *string, label, placeholder string, choices []string, disabled bool) (ui.Element, bool) {
	type draft struct{ text, saved string }
	box := ui.Box(c).Height(k.style("select").Height)
	edit := ui.Local(box, "deziro-combobox", func() draft { return draft{text: *value, saved: *value} })
	if edit.saved != *value {
		edit.text, edit.saved = *value, *value
	}
	changed := false
	box.Children(func() {
		parts := ui.ComboboxBase(c, &edit.text)
		field := k.styleField(c, parts.Input.Fill().Label(label).Placeholder(placeholder).Disabled(disabled))
		parts.Popup(func(panel ui.Element) {
			paint(panel, k.style("selectMenu"))
			query := strings.ToLower(strings.TrimSpace(edit.text))
			ui.Scroll(c).FillWidth().MaxHeight(215).Children(func() {
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
	palette := c.Theme()
	slider := ui.SliderBase(c, value, minimum, maximum).Height(24).PaddingX(8).Label(label).FocusRing(true)
	slider.Draw(func(p *ui.Painter, bounds ui.Rect) {
		width := max(0, bounds.W-16)
		fraction := float32(0)
		if maximum > minimum {
			fraction = float32(min(1, max(0, (*value-minimum)/(maximum-minimum))))
		}
		track := ui.Rect{X: bounds.X + 8, Y: bounds.Y + (bounds.H-8)/2, W: width, H: 8}
		p.Fill(track, palette.Surface, 4)
		p.Stroke(track, palette.Border, 4, 1)
		p.Fill(ui.Rect{X: track.X, Y: track.Y, W: width * fraction, H: track.H}, palette.Accent, 4)
		thumb := ui.Rect{X: track.X + width*fraction - 7, Y: track.Y - 3, W: 14, H: 14}
		p.Fill(thumb, palette.Accent, 7)
	})
	return slider
}
