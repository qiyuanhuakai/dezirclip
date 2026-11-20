package uikit

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
)

// SettingsToggle preserves checkbox accessibility while drawing a switch.
func (k Kit) SettingsToggle(c *ui.Context, on *bool, label string) ui.Element {
	style, palette := k.style("select"), c.Theme()
	row := ui.CheckboxBase(c, on).Label(label).FillWidth().MinHeight(34).Gap(10).AlignItems(ui.Center).FocusRing(true)
	row.Children(func() {
		text(c, label, style).Grow(1).Wrap().PassThrough()
		track := ui.Box(c).Size(36, 20).Shrink(0).PassThrough()
		fill, offset := palette.Surface, float32(3)
		if *on {
			fill, offset = palette.Accent, 19
		} else if row.Hovered() {
			fill = palette.Selection
		}
		duration := time.Duration(k.palette().TransitionMs) * time.Millisecond
		if duration > 0 {
			fill = track.AnimateColor("switch-track", fill, ui.Animation{Duration: duration})
			offset = track.Animate("switch-position", offset, duration)
		}
		track.Radius(10).Border(1, palette.Border).Background(fill).Children(func() {
			ui.Box(c).Absolute().Left(offset).Top(3).Size(14, 14).Radius(7).
				Background(palette.AccentText).Border(1, palette.Border).PassThrough()
		})
	})
	return row
}

func (k Kit) HotkeyBinding(c *ui.Context, label, value, placeholder string, recording, disabled bool) ui.Element {
	style := k.style("select")
	if recording {
		style = k.style("selectFocus")
	}
	field := ui.ButtonBase(c).Label(label).Height(style.Height).Gap(4).AlignItems(ui.Center).FocusRing(true).Disabled(disabled)
	paint(field, k.interactive(c, field, style, recording))
	field.Children(func() {
		if value == "" {
			text(c, placeholder, style).PassThrough()
			return
		}
		cap := k.style("selectSelected")
		cap.Padding = [4]float32{1, 5, 1, 5}
		for _, key := range strings.Split(value, "+") {
			paint(ui.Box(c).MinWidth(16).Center().PassThrough(), cap).
				Children(func() { text(c, key, cap).SingleLine().PassThrough() })
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

func (k Kit) Combo(c *ui.Context, value *string, label string, choices []string, disabled bool) ui.Element {
	parts := ui.ComboboxBase(c, value)
	field := k.styleField(c, parts.Input.Label(label).Disabled(disabled))
	if !disabled && parts.Open() {
		parts.Popup(func(panel ui.Element) {
			paint(panel, k.style("selectMenu"))
			for _, choice := range choices {
				if parts.Filtering() && !strings.Contains(strings.ToLower(choice), strings.ToLower(*value)) {
					continue
				}
				item := parts.Item(choice).Label(choice)
				style := k.style("selectOption")
				if choice == *value || item.Highlighted() {
					style = k.style("selectSelected")
				}
				paint(item.Height(style.Height), k.interactive(c, item, style, choice == *value))
				item.Children(func() { text(c, choice, style) })
			}
		})
	}
	return field
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
		track := ui.Rect{X: bounds.X + 8, Y: bounds.Y + (bounds.H-4)/2, W: width, H: 4}
		p.Fill(track, palette.Border, 2)
		p.Fill(ui.Rect{X: track.X, Y: track.Y, W: width * fraction, H: 4}, palette.Accent, 2)
		thumb := ui.Rect{X: track.X + width*fraction - 6, Y: track.Y - 4, W: 12, H: 12}
		p.Fill(thumb, palette.Surface, 6)
		p.Stroke(thumb, palette.Accent, 6, 1)
	})
	return slider
}
