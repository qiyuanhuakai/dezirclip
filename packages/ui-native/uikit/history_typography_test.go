package uikit

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

func TestHistoryCardClipsAnUnbreakablePreviewOnOneLine(t *testing.T) {
	for _, compact := range []bool{false, true} {
		tester := ui.NewTester(func(c *ui.Context) {
			kit := Kit{Mode: "light"}
			kit.Apply(c)
			ui.Column(c).Width(190).Children(func() {
				kit.HistoryCard(c, HistoryCardOptions{Summary: "Short text", Compact: compact}).Label("reference")
				kit.HistoryCard(c, HistoryCardOptions{Summary: strings.Repeat("Ab09", 66), Compact: compact}).Label("token")
			})
		}, 200, 180)
		plain, plainOK := tester.Find("reference")
		token, tokenOK := tester.Find("token")
		if !plainOK || !tokenOK || token.H != plain.H || token.W != plain.W {
			t.Fatalf("compact=%v: long token resized its card: reference %v, token %v", compact, plain, token)
		}
	}
}

func TestHistoryFontUsesItsThemeRole(t *testing.T) {
	palette := theme.Select("light")
	catalog := map[string]theme.WidgetStyle{
		"historyContent":        {Font: "Theme Text"},
		"historyContentCompact": {Font: "Theme Mono, monospace", UsesMonoFont: true},
	}
	base := NewKit("light", palette, catalog)
	interfaceOnly := base.WithInterfaceFont("Example UI")
	fonts := interfaceOnly.WithMonospaceFont("  Example Mono  ")
	tests := []struct {
		kit          Kit
		widget, font string
	}{
		{base, "historyContent", "Theme Text"},
		{interfaceOnly, "historyContentCompact", "Theme Mono, monospace"},
		{fonts, "historyContent", "Example UI"},
		{fonts, "historyContentCompact", "Example Mono"},
	}
	for _, test := range tests {
		if font := test.kit.style(test.widget).Font; font != test.font {
			t.Errorf("%s uses %q, want %q", test.widget, font, test.font)
		}
	}
}

func TestCompactHistoryTagsDoNotResizeOrBlockCard(t *testing.T) {
	clicks := 0
	tester := ui.NewTester(func(c *ui.Context) {
		kit := Kit{Mode: "light"}
		kit.Apply(c)
		kit.HistoryCard(c, HistoryCardOptions{Summary: "No tag", Compact: true, Width: 240})
		kit.HistoryCard(c, HistoryCardOptions{
			Summary: "Has tag", Compact: true, Width: 240,
			Tags:     []HistoryTag{{Name: "Work", Color: ui.RGB(36, 94, 214)}},
			Activate: func() { clicks++ },
		})
	}, 240, 180)
	plain, plainOK := tester.Find("No tag")
	tagged, taggedOK := tester.Find("Has tag")
	chip, chipOK := tester.Find("Work")
	if !plainOK || !taggedOK || !chipOK {
		t.Fatalf("missing compact cards or chip: %v %v %v", plainOK, taggedOK, chipOK)
	}
	if plain.H != tagged.H || chip.X < tagged.X+tagged.W/2 || chip.Y+chip.H > tagged.Y+tagged.H {
		t.Fatalf("compact label must overlay its card: plain %v, tagged %v, chip %v", plain, tagged, chip)
	}
	tester.ClickAt(chip.X+chip.W/2, chip.Y+chip.H/2)
	if clicks != 1 {
		t.Fatalf("click on the label must still activate the card, got %d", clicks)
	}
}

func TestCompactHistoryPinKeepsTextWidthAndHeight(t *testing.T) {
	clicks := 0
	const summary = "A clipboard preview keeps the same width when a card is pinned."
	tester := ui.NewTester(func(c *ui.Context) {
		kit := Kit{Mode: "light"}
		kit.Apply(c)
		ui.Column(c).Width(192).Children(func() {
			kit.HistoryCard(c, HistoryCardOptions{Summary: summary, Compact: true}).Label("plain")
			kit.HistoryCard(c, HistoryCardOptions{
				Summary: summary, Compact: true, Pinned: true, Activate: func() { clicks++ },
			}).Label("pinned")
		})
	}, 200, 156)
	plain, plainOK := tester.Find("plain")
	pinned, pinnedOK := tester.Find("pinned")
	if !plainOK || !pinnedOK || pinned.H != plain.H || pinned.W != plain.W {
		t.Fatalf("compact pin changed card geometry: plain %v, pinned %v", plain, pinned)
	}
	x, y := int(pinned.X)+2, int(pinned.Y+pinned.H/2)
	if pixel := tester.Image().RGBAAt(x, y); pixel.R > 30 || pixel.G < 100 || pixel.B < 230 {
		t.Fatalf("missing blue compact pin at the border: %v", pixel)
	}
	tester.ClickAt(float32(x)+0.5, float32(y)+0.5)
	if clicks != 1 {
		t.Fatal("the pin marker intercepted activation")
	}
}
