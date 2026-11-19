package presets

import "testing"

func TestImportedPaletteHonorsModeAndScope(t *testing.T) {
	source := `
		:root.theme-custom, body.theme-custom {
			--dc-surface-window: #edf3ed;
			--Input-RGB: 249 252 247;
			--dc-surface-input: rgb(var(--Input-RGB));
			--dc-text-primary: var(--forest-text, #123);
			--dc-accent: #28633c !important;
			--dc-border: #fff;
			--dc-radius: 12px;
		}
		:root.theme-custom.dark-mode, body.theme-custom.dark-mode {
			--dc-surface-window: #17251c;
			--forest-text: #edf6eb;
		}
		body.dark-mode.theme-custom {
			--dc-border: rgb(73, 99, 79);
			--dc-accent: #ffffff;
		}
		/* A descendant's variable is local to the widget. */
		.theme-custom .history-item { --dc-surface-window: #f00; }
		@media (min-width: 1px) { .theme-custom { --dc-surface-window: #f00; } }
	`
	mica := Select("mica", "light")
	light := Imported("mica", "light", source)
	dark := Imported("mica", "dark", source)
	if got := light.Tokens(); got.Background != "#edf3ed" || got.Panel != "#f9fcf7" ||
		got.Text != "#112233" || got.Accent != "#28633c" || got.Radius != 12 || light.SurfaceTint() != "#edf3ed" {
		t.Fatalf("light theme colors = %+v, tint = %s", got, light.SurfaceTint())
	}
	if got := dark.Tokens(); got.Background != "#17251c" || got.Border != "#49634f" ||
		got.Text != "#edf6eb" || got.Accent != "#28633c" || dark.SurfaceTint() != "#17251c" {
		t.Fatalf("dark theme colors = %+v, tint = %s", got, dark.SurfaceTint())
	}
	if len(light.widgets) != 0 || len(mica.widgets) == 0 || mica.Tokens().Background != "#fcfcfc" {
		t.Fatal("import did not isolate the semantic styles from the built-in catalog")
	}
}

func TestImportedPaletteBoundsVariableExpansion(t *testing.T) {
	source := `.theme-custom { --loop: var(--loop, var(--loop)); --dc-accent: var(--loop); }`
	base := Select("retro", "dark")
	preset := Imported("retro", "dark", source)
	if preset.Tokens().Accent != base.Tokens().Accent || len(preset.widgets) != len(base.widgets) {
		t.Fatal("unsupported palette data displaced the base theme")
	}
}
