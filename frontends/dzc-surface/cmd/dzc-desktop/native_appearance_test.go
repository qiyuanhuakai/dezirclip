package main

import (
	"encoding/json"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

func TestNativePopupAppearanceUsesSelectedThemeAndSeparateOpacity(t *testing.T) {
	for _, name := range []string{"retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"} {
		for _, mode := range []string{"light", "dark"} {
			t.Run(name+"/"+mode, func(t *testing.T) {
				settings := map[string]string{
					"app.theme": name, "app.color_mode": mode, "app.language": "en",
					"app.font_main": "Example UI", "app.clipboard_item_font_size": "18",
					"app.surface_opacity": "20", "app.preview_opacity": "0",
				}
				got := resolveNativeSurfaceAppearance(settings, mode == "light")
				preset := presets.Select(name, mode)
				want := preset.Tokens()
				tint := preset.SurfaceTint()
				if tint == "" {
					tint = want.Background
				}
				if got.base != name || !got.quickPaste.Valid() || !got.preview.Valid() {
					t.Fatalf("invalid popup styles: %+v", got)
				}
				if got.quickPaste.Mode != mode || got.quickPaste.Tokens.Background != want.Background ||
					got.quickPaste.Tokens.Accent != want.Accent || got.preview.Tokens.Background != tint {
					t.Fatalf("popup palette disagrees with the selected preset: %+v", got)
				}
				if got.quickPaste.Tokens.FontFamilies[0] != "Example UI" || got.preview.Tokens.FontFamilies[0] != "Example UI" ||
					got.quickPaste.Tokens.FontSize != 18 || got.preview.Tokens.FontSize != want.FontSize {
					t.Fatalf("popup typography settings were not applied: %+v", got)
				}
				if got.quickPaste.Opacity != .20 || got.preview.Opacity != 0 {
					t.Fatalf("independent opacity values were lost: %+v", got)
				}
				delete(settings, "app.preview_opacity")
				if inherited := resolveNativeSurfaceAppearance(settings, false).preview.Opacity; inherited != .20 {
					t.Fatalf("preview opacity inherited %v", inherited)
				}
			})
		}
	}
}

func TestNativePopupAppearanceUpdatesImportedThemeOnSystemModeChange(t *testing.T) {
	custom := protocol.DesktopThemePackage{Format: "dezirclip-theme", Version: 1, Id: "hours", Name: "Hours", Base: "liquid-glass",
		Css: ":root.theme-custom {--dc-surface-window:#e6dccd;--dc-accent:#395880} :root.theme-custom.dark-mode {--dc-surface-window:#242838;--dc-accent:#aabfdf}"}
	catalog, err := json.Marshal([]protocol.DesktopThemePackage{custom})
	if err != nil {
		t.Fatal(err)
	}
	settings := map[string]string{
		"app.theme": "custom:hours", "app.custom_themes": string(catalog), "app.color_mode": "system", "app.language": "en",
	}
	light := resolveNativeSurfaceAppearance(settings, false)
	dark := resolveNativeSurfaceAppearance(settings, true)
	if light.base != "liquid-glass" || light.quickPaste.Mode != "light" || light.quickPaste.Tokens.Accent != "#395880" ||
		dark.quickPaste.Mode != "dark" || dark.preview.Tokens.Background != "#242838" || dark.preview.Tokens.Accent != "#aabfdf" {
		t.Fatalf("imported appearance did not follow the system mode: light=%+v dark=%+v", light, dark)
	}
}
