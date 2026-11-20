package main

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
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
	n.fontSetting(c, "app.font_main", n.text("font_main", "Interface font"), n.text("native_reset_ui_font"), n.fontChoices.main)
	n.fontSetting(c, "app.font_mono", n.text("font_mono", "Monospace font"), n.text("native_reset_mono_font"), n.fontChoices.mono)
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

func (n *nativeMain) fontSetting(c *ui.Context, key, label, resetLabel string, choices []string) {
	saved := n.preferences[key]
	selected := saved
	ui.Text(c, label).FontWeight(600)
	ui.Row(c).Gap(6).Wrap().AlignItems(ui.Center).Children(func() {
		field := n.mainKit().Combo(c, &selected, label, choices, n.mutating || len(choices) == 0).Placeholder(n.text("font_default_option")).
			Grow(1).MinWidth(150).Disabled(n.mutating || len(choices) == 0)
		if field.Changed() && selected != saved {
			n.saveSetting(key, selected)
		}
		if saved != "" && n.mainKit().Button(c, uikit.ButtonOptions{Label: resetLabel, Disabled: n.mutating}).Clicked() {
			n.saveSetting(key, "")
		}
	})
}

func (n *nativeMain) rangeSetting(c *ui.Context, setting nativeRangeSetting, label string, saved int) int {
	type draft struct {
		value float64
		dirty bool
	}
	row := ui.Row(c).Gap(8).AlignItems(ui.Center)
	edit := ui.Local(row, setting.key, func() draft { return draft{value: float64(saved)} })
	if !edit.dirty && !n.mutating {
		edit.value = float64(saved)
	}
	row.Children(func() {
		ui.Text(c, label).MinWidth(100)
		slider := n.mainKit().Slider(c, &edit.value, float64(setting.minimum), float64(setting.maximum), label).
			Step(1).Label(label).Grow(1).Disabled(n.mutating)
		edit.dirty = edit.dirty || slider.Changed()
		value := int(edit.value)
		// Keep dragging local and save once the pointer is released.
		if edit.dirty && !slider.Pressed() && !n.mutating {
			edit.dirty = false
			if value != saved {
				n.saveSetting(setting.key, strconv.Itoa(value))
			}
		}
		ui.Text(c, strconv.Itoa(value)+setting.unit).MinWidth(36)
	})
	return int(edit.value)
}
