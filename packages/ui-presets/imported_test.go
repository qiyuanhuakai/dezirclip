package presets

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

const widgetTheme = `
	.theme-custom { --raised: #f9fcf7; --panel: #e1ebe1; --hover: #d2e2d3; }
	.theme-custom header { padding: 12px 0; background: linear-gradient(110deg, var(--raised), var(--panel)); }
	.theme-custom .header-title { font-size: 17px; font-weight: 650; letter-spacing: 0.4px; }
	.theme-custom .search-input { border-radius: 12px 12px 4px 12px; }
	.theme-custom .history-item {
		margin: 7px 10px;
		border: 1px solid #b1c8b5;
		border-inline-start: 3px solid #73967b;
		border-radius: 4px 12px 12px 4px;
		background: var(--raised);
		box-shadow: 0 2px 6px #0b241510;
	}
	.theme-custom .history-item:hover { background: var(--hover); }
	.theme-custom .history-item.selected { border-color: #28633c; background: #cde4cf; }
	.theme-custom.dark-mode .history-item { background: #293d30; }
`

func TestImportedWidgetStyles(t *testing.T) {
	light := Imported("mica", "light", widgetTheme)
	dark := Imported("mica", "dark", widgetTheme)
	header := light.widgets["header"]
	if header.Padding != [4]float32{12, 0, 12, 0} || header.Gradient == nil || header.Gradient.Angle != 110 ||
		header.Gradient.From != [4]uint8{249, 252, 247, 255} || header.Gradient.To != [4]uint8{225, 235, 225, 255} {
		t.Fatalf("unexpected header paint: %+v", header)
	}
	title := light.widgets["headerTitle"]
	if title.Size != 17 || title.Weight != 650 || title.LetterSpacing != .4 {
		t.Fatalf("unexpected title text style: %+v", title)
	}
	card := light.widgets["historyCard"]
	if card.Margin != [4]float32{7, 10, 7, 10} || card.CornerRadii == nil || *card.CornerRadii != [4]float32{4, 12, 12, 4} ||
		card.BorderWidths == nil || *card.BorderWidths != [4]float32{1, 1, 1, 3} || len(card.Shadows) != 1 ||
		card.Shadows[0].Color != [4]uint8{11, 36, 21, 16} {
		t.Fatalf("unexpected card paint: %+v", card)
	}
	if dark.widgets["historyCard"].Fill != [4]uint8{41, 61, 48, 255} ||
		light.widgets["historyCardCompactSelected"].Fill != [4]uint8{205, 228, 207, 255} ||
		Select("mica", "light").widgets["historyCard"].CornerRadii != nil {
		t.Fatal("mode or selection colors were lost, or the built-in catalog was modified")
	}
}

func TestImportedCardPaintsHoverAndCompactSelection(t *testing.T) {
	kit := Imported("mica", "light", widgetTheme).Kit()
	options := uikit.HistoryCardOptions{Summary: "Forest entry"}
	tester := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		ui.Column(c).Fill().Children(func() {
			kit.Header(c, uikit.HeaderOptions{Title: "Forest"})
			kit.HistoryCard(c, options)
		})
	}, 360, 240)
	card, ok := tester.Find("Forest entry")
	if !ok {
		t.Fatal("card not laid out")
	}
	x, y := card.X+card.W-18, card.Y+card.H/2
	tester.Move(x, y)
	pixel := tester.Image().RGBAAt(int(x), int(y))
	if pixel.R != 210 || pixel.G != 226 || pixel.B != 211 {
		t.Fatalf("hover pixel = %v", pixel)
	}
	options.Compact, options.Selected = true, true
	tester.Frame()
	card, ok = tester.Find("Forest entry")
	if !ok {
		t.Fatal("compact card not laid out")
	}
	pixel = tester.Image().RGBAAt(int(card.X+card.W-18), int(card.Y+card.H/2))
	if pixel.R != 205 || pixel.G != 228 || pixel.B != 207 {
		t.Fatalf("compact selected pixel = %v", pixel)
	}
}

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
