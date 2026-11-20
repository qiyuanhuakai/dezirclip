package main

import (
	"context"
	"strconv"
	"time"

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
	n.rangeSetting(c, nativeItemSize, n.text("clipboard_item_font_size", "Item font size"), n.text("clipboard_item_font_size_hint"), nativeItemSize.value(n.preferences))
	n.rangeSetting(c, nativeTagSize, n.text("clipboard_tag_font_size", "Tag font size"), n.text("clipboard_tag_font_size_hint"), nativeTagSize.value(n.preferences))

	if n.fontChoices == nil {
		if n.fontsFailed {
			ui.Row(c).ColumnSpan(-1).Gap(8).Wrap().Children(func() {
				ui.Text(c, n.text("common_error")).TextColor(c.Theme().TextMuted)
				if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_retry")}).Clicked() {
					n.findFonts()
				}
			})
		} else {
			n.findFonts()
			ui.Text(c, n.text("native_loading_fonts")).TextColor(c.Theme().TextMuted)
		}
		return
	}
	n.fontSetting(c, "app.font_main", n.text("font_main", "Interface font"), n.text("font_main_hint"), n.fontChoices.main)
	n.fontSetting(c, "app.font_mono", n.text("font_mono", "Monospace font"), n.text("font_mono_hint"), n.fontChoices.mono)
}

func (n *nativeMain) findFonts() {
	window := n.window
	if window == nil || n.library == nil || n.readingFonts {
		return
	}
	n.readingFonts, n.fontsFailed = true, false
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
		defer cancel()
		fonts, err := n.library.SystemFonts(ctx)
		var choices nativeFontChoices
		for _, font := range fonts {
			if font.IsMono {
				choices.mono = append(choices.mono, font.Family)
			} else {
				choices.main = append(choices.main, font.Family)
			}
		}
		// The available families belong to the renderer, independent of the clipboard session.
		window.Update(func() {
			n.readingFonts, n.fontsFailed = false, err != nil
			if err == nil {
				n.fontChoices = &choices
			}
		})
	}()
}

func (n *nativeMain) fontSetting(c *ui.Context, key, label, hint string, choices []string) {
	kit := n.mainKit()
	system := n.text("font_default_option")
	options := make([]uikit.SelectOption, 1, len(choices)+2)
	options[0] = uikit.SelectOption{Label: system}
	selected := n.preferences[key]
	found := selected == ""
	for _, family := range choices {
		options = append(options, uikit.SelectOption{Value: family, Label: family})
		found = found || family == selected
	}
	if !found {
		options = append(options, uikit.SelectOption{Value: selected, Label: selected})
	}
	kit.SettingsField(c.Key(key), uikit.SettingsFieldOptions{Stacked: true}).Children(func() {
		kit.SettingsFieldLabel(c, label, n.settingsHelpLabel(label), hint)
		_, changed := kit.SearchSelect(c, uikit.SearchSelectOptions{
			Label: label, Placeholder: system, NoMatches: n.text("no_matching_fonts"),
			Value: &selected, Choices: options, Disabled: n.mutating || n.session == nil || n.history.state.ServiceInstanceId == "",
		})
		if changed {
			n.saveSetting(key, selected)
		}
	})
}

func (n *nativeMain) rangeSetting(c *ui.Context, setting nativeRangeSetting, label, hint string, saved int) int {
	type draft struct {
		value float64
		dirty bool
	}
	kit := n.mainKit()
	row := kit.SettingsField(c.Key(setting.key), uikit.SettingsFieldOptions{Stacked: true, Last: setting == nativeBackgroundOpacity})
	edit := ui.Local(row, setting.key, func() draft { return draft{value: float64(saved)} })
	if !edit.dirty && !n.mutating {
		edit.value = float64(saved)
	}
	row.Children(func() {
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			kit.SettingsFieldLabel(c, label, n.settingsHelpLabel(label), hint).Grow(1)
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
				if setting == nativeSurfaceOpacity {
					n.saveSurfaceOpacity(value)
				} else {
					n.saveSetting(setting.key, strconv.Itoa(value))
				}
			}
		}
	})
	return int(edit.value)
}

func (n *nativeMain) saveSurfaceOpacity(value int) {
	const preference = "app.surface_opacity"
	saved := strconv.Itoa(value)
	n.writeSettingWithEffect(func(ctx context.Context, epoch uint64) error {
		return n.library.SetSetting(ctx, preference, saved, epoch)
	}, func() {
		n.preferences[preference] = saved
		n.surfaceOpacityDraft = nil
	})
}
