package main

import (
	"slices"
	"strconv"

	"github.com/egoist/mygo/ui"
)

type nativeFontChoices struct {
	main, mono []string
}

type nativeRangeSetting struct {
	key, unit                  string
	minimum, maximum, fallback int
}

var (
	nativeItemSize       = nativeRangeSetting{"app.clipboard_item_font_size", " px", 11, 18, 13}
	nativeTagSize        = nativeRangeSetting{"app.clipboard_tag_font_size", " px", 8, 14, 10}
	nativeSurfaceOpacity = nativeRangeSetting{key: "app.surface_opacity", unit: "%", maximum: 100}
	nativePreviewOpacity = nativeRangeSetting{key: "app.preview_opacity", unit: "%", maximum: 100}
)

func (s nativeRangeSetting) value(preferences map[string]string) int {
	value, err := strconv.Atoi(preferences[s.key])
	if err != nil || value < s.minimum || value > s.maximum {
		return s.fallback
	}
	return value
}

func (n *nativeMain) typographySettings(c *ui.Context) {
	n.rangeSetting(c, nativeItemSize, n.text("clipboard_item_font_size", "Item font size"), nativeItemSize.value(n.preferences))
	n.rangeSetting(c, nativeTagSize, n.text("clipboard_tag_font_size", "Tag font size"), nativeTagSize.value(n.preferences))

	if n.fontChoices == nil {
		n.findFonts()
		ui.Text(c, n.text("native_loading_fonts")).TextColor(c.Theme().TextMuted)
		return
	}
	n.fontSetting(c, "app.font_main", n.text("font_main", "Interface font"), n.fontChoices.main)
	n.fontSetting(c, "app.font_mono", n.text("font_mono", "Monospace font"), n.fontChoices.mono)
}

func (n *nativeMain) findFonts() {
	window := n.window
	if window == nil || n.readingFonts {
		return
	}
	n.readingFonts = true
	go func() {
		var choices nativeFontChoices
		for _, family := range ui.FontFamilies() {
			if family.Monospaced {
				choices.mono = append(choices.mono, family.Name)
			} else {
				choices.main = append(choices.main, family.Name)
			}
		}
		// The available families belong to the renderer, independent of the clipboard session.
		window.Update(func() {
			n.fontChoices, n.readingFonts = &choices, false
		})
	}()
}

func (n *nativeMain) fontSetting(c *ui.Context, key, label string, choices []string) {
	kit := n.mainKit()
	kit.SettingsLabel(c, label)
	system := n.text("font_default_option")
	options := append([]string{system}, choices...)
	selected := n.preferences[key]
	if selected == "" {
		selected = system
	} else if !slices.Contains(options, selected) {
		options = append(options, selected)
	}
	field, changed := kit.Combo(c, &selected, label, system, options, n.mutating)
	field.FillWidth()
	if changed {
		if selected == system {
			selected = ""
		}
		n.saveSetting(key, selected)
	}
}

func (n *nativeMain) rangeSetting(c *ui.Context, setting nativeRangeSetting, label string, saved int) int {
	type draft struct {
		value float64
		dirty bool
	}
	row := ui.Column(c).FillWidth().Padding(6, 0)
	edit := ui.Local(row, setting.key, func() draft { return draft{value: float64(saved)} })
	if !edit.dirty && !n.mutating {
		edit.value = float64(saved)
	}
	row.Children(func() {
		kit := n.mainKit()
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			kit.SettingsLabel(c, label).Grow(1).Wrap()
			kit.SettingsLabel(c, strconv.Itoa(int(edit.value))+setting.unit).FontSize(11).TextColor(c.Theme().TextMuted)
		})
		slider := kit.Slider(c, &edit.value, float64(setting.minimum), float64(setting.maximum), label).
			Step(1).FillWidth().Disabled(n.mutating)
		edit.dirty = edit.dirty || slider.Changed()
		value := int(edit.value)
		// Keep dragging local and save once the pointer is released.
		if edit.dirty && !slider.Pressed() && !n.mutating {
			edit.dirty = false
			if value != saved {
				n.saveSetting(setting.key, strconv.Itoa(value))
			}
		}
	})
	return int(edit.value)
}
