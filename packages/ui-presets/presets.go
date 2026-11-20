// Package presets provides the GPL theme catalogs sampled from the Web UI.
package presets

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

//go:embed builtin-presets.json
var paletteJSON []byte

//go:embed builtin-widget-styles.json
var widgetJSON []byte

type palette struct {
	theme.Tokens
	SurfaceTint string
}

var palettes = decode[palette](paletteJSON)
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
	colors  palette
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
	return uikit.NewKit(p.mode, p.colors.Tokens, p.widgets)
}

// Tokens returns the semantic palette for associated native windows.
func (p Preset) Tokens() theme.Tokens {
	tokens := p.colors.Tokens
	tokens.FontFamilies = slices.Clone(tokens.FontFamilies)
	return tokens
}

// SurfaceTint returns the outer window color for presets with adjustable shell opacity.
func (p Preset) SurfaceTint() string { return p.colors.SurfaceTint }

// WindowBackground resolves the shell tint at the given setting. Solid themes keep their background.
func (p Preset) WindowBackground(percent int) ui.Color {
	if tint := p.SurfaceTint(); tint != "" {
		return ui.Hex(tint).Alpha(float32(min(100, max(0, percent))) / 100)
	}
	return ui.Hex(p.colors.Background)
}

// ShowcasePadding reproduces the Web fixture's viewport in the native gallery.
func (p Preset) ShowcasePadding() [4]float32 { return p.widgets["gallery"].Padding }
