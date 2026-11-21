package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

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
