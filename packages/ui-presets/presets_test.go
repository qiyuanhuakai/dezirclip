package presets

import (
	"testing"

	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

func TestBuiltinThemeCatalogs(t *testing.T) {
	for _, name := range []string{"retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"} {
		for _, mode := range []string{"light", "dark"} {
			t.Run(name+"/"+mode, func(t *testing.T) {
				preset := Select(name, mode)
				if appearance := (theme.Appearance{Mode: mode, Opacity: 1, Tokens: preset.Tokens()}); !appearance.Valid() {
					t.Fatalf("invalid palette: %+v", appearance)
				}
				for _, widget := range []string{"select", "filter", "searchInput", "help", "disclosure", "progress", "toast", "confirm", "tag"} {
					style := preset.widgets[widget]
					if style.Width <= 0 || style.Height <= 0 || style.Opacity <= 0 {
						t.Fatalf("missing computed geometry for %s: %+v", widget, style)
					}
				}
			})
		}
	}
}

func TestProgressToastsFollowThemeShadowPolicy(t *testing.T) {
	for name := range palettes {
		wantShadow := name == "retro" || name == "liquid-glass"
		for _, mode := range []string{"light", "dark"} {
			skin := Select(name, mode)
			for _, state := range []string{"progress", "progressDone", "progressError"} {
				hasShadow := len(skin.widgets[state].Shadows) > 0
				if hasShadow != wantShadow {
					t.Errorf("%s/%s %s: can draw shadow %v, want %v", name, mode, state, hasShadow, wantShadow)
				}
			}
		}
	}
}
