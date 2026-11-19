// Package presets provides the GPL theme catalogs sampled from the Web UI.
package presets

import (
	_ "embed"
	"encoding/json"
	"fmt"

	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

//go:embed builtin-presets.json
var paletteJSON []byte

//go:embed builtin-widget-styles.json
var widgetJSON []byte

var palettes = decode[theme.Tokens](paletteJSON)
var widgets = decode[map[string]theme.WidgetStyle](widgetJSON)

func decode[T any](data []byte) map[string]map[string]T {
	var result map[string]map[string]T
	if err := json.Unmarshal(data, &result); err != nil {
		panic(fmt.Errorf("invalid theme catalog: %w", err))
	}
	return result
}

type Preset struct {
	mode    string
	colors  theme.Tokens
	widgets map[string]theme.WidgetStyle
}

// Select returns a built-in theme. The default theme and mode are mica and light.
func Select(name, mode string) Preset {
	if mode != "dark" {
		mode = "light"
	}
	if palettes[name] == nil {
		name = "mica"
	}
	return Preset{mode: mode, colors: palettes[name][mode], widgets: widgets[name][mode]}
}

// Kit applies this catalog to the independent native components.
func (p Preset) Kit() uikit.Kit {
	return uikit.NewKit(p.mode, p.colors, p.widgets)
}

// ShowcasePadding reproduces the Web fixture's viewport in the native gallery.
func (p Preset) ShowcasePadding() [4]float32 { return p.widgets["gallery"].Padding }
