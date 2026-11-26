package main

import (
	"context"
	"encoding/json"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
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

	var fonts nativeFontChoices
	if n.fontChoices != nil {
		fonts = *n.fontChoices
	} else {
		if n.fontsFailed {
			ui.Row(c).ColumnSpan(-1).Gap(8).Wrap().Children(func() {
				ui.Text(c, n.text("common_error")).TextColor(c.Theme().TextMuted)
				if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_retry")}).Clicked() {
					n.findFonts()
				}
			})
		} else {
			n.findFonts()
			ui.Text(c, n.text("native_loading_fonts")).TextColor(c.Theme().TextMuted).ColumnSpan(-1)
		}
	}
	n.fontSetting(c, "app.font_main", n.text("font_main", "Interface font"), n.text("font_main_hint"), fonts.main)
	n.fontSetting(c, "app.font_mono", n.text("font_mono", "Monospace font"), n.text("font_mono_hint"), fonts.mono)
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
		options = append(options, uikit.SelectOption{Value: family, Label: family, FontFamily: family})
		found = found || family == selected
	}
	if !found {
		options = append(options, uikit.SelectOption{Value: selected, Label: selected, FontFamily: selected})
	}
	kit.SettingsField(c.Key(key), uikit.SettingsFieldOptions{Stacked: true}).Children(func() {
		kit.SettingsFieldLabel(c, label, n.settingsHelpLabel(label), hint)
		_, changed := kit.SearchSelect(c, uikit.SearchSelectOptions{
			Label: label, Placeholder: system, NoMatches: n.text("no_matching_fonts"),
			Value: &selected, Choices: options, Disabled: n.mutating || n.session == nil || n.history.state.ServiceInstanceId == "",
		})
		if changed {
			n.saveFont(key, selected)
		}
	})
}

func (n *nativeMain) saveFont(key, selected string) {
	n.writeSettingWithEffect(func(ctx context.Context, epoch uint64) error {
		return n.library.SetSetting(ctx, key, selected, epoch)
	}, func() {
		n.preferences[key] = selected
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
				switch setting {
				case nativeSurfaceOpacity:
					n.saveOpacity(setting.key, value, &n.surfaceOpacityDraft)
				case nativeBackgroundOpacity:
					n.saveOpacity(setting.key, value, &n.background.opacityDraft)
				default:
					n.saveSetting(setting.key, strconv.Itoa(value))
				}
			}
		}
	})
	return int(edit.value)
}

// Read view settings before retiring the presentation. Preview opacity can
// be applied in place without rereading clipboard contents or rebuilding cards.
func (n *nativeMain) refreshViewSettings() {
	if n.session == nil || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.viewSettingsRevision++
	if n.readingViewSettings {
		return
	}
	n.readingViewSettings = true
	session, state, revision := n.session, n.history.state, n.viewSettingsRevision
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		raw, err := n.library.preferences(ctx, state)
		var settings map[string]string
		if err == nil {
			err = json.Unmarshal(raw, &settings)
		}
		n.update(session, func() {
			n.readingViewSettings = false
			if revision != n.viewSettingsRevision {
				n.refreshViewSettings()
				return
			}
			if err != nil || settings == nil || !sameSettingsExceptPreviewOpacity(n.preferences, settings) {
				n.prepare(n.resume)
				return
			}
			n.applyPreviewViewSettings(settings)
		})
	}()
}

func sameSettingsExceptPreviewOpacity(current, next map[string]string) bool {
	for key, value := range current {
		if key != nativePreviewOpacity.key && next[key] != value {
			return false
		}
	}
	for key, value := range next {
		if key != nativePreviewOpacity.key && current[key] != value {
			return false
		}
	}
	return true
}

func (n *nativeMain) applyPreviewViewSettings(settings map[string]string) {
	key := nativePreviewOpacity.key
	changed := n.preferences[key] != settings[key]
	n.preferences = settings
	if changed && n.preview != nil && n.preview.opacity != nil {
		control := n.preview.opacity
		control.applyPreference(opacityPreference(settings[key], settings["app.surface_opacity"]))
		if control.window != nil {
			control.window.Invalidate()
		}
	}
}

func (n *nativeMain) saveOpacity(preference string, value int, draft **int) {
	saved := strconv.Itoa(value)
	n.writeSettingWithResult(4*time.Second, func(ctx context.Context, epoch uint64) error {
		return n.library.SetSetting(ctx, preference, saved, epoch)
	}, func(err error) {
		if err == nil {
			n.preferences[preference] = saved
		}
		*draft = nil
	})
}
