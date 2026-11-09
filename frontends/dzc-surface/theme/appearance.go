package theme

import (
	"github.com/egoist/mygo/ui"
	"regexp"
)

type Appearance struct {
	Mode    string
	Locale  string
	Opacity float64
	Tokens  Tokens
}

var appearanceColor = regexp.MustCompile(`^#[0-9a-fA-F]{6}$`)

func (a Appearance) Valid() bool {
	for _, value := range []string{a.Tokens.Background, a.Tokens.Panel, a.Tokens.Text, a.Tokens.Muted, a.Tokens.Accent, a.Tokens.Selected, a.Tokens.Border} {
		if !appearanceColor.MatchString(value) {
			return false
		}
	}
	for _, font := range a.Tokens.FontFamilies {
		if len(font) == 0 || len(font) > 256 {
			return false
		}
	}
	return (a.Mode == "light" || a.Mode == "dark") && a.Opacity >= 0 && a.Opacity <= 1 && len(a.Locale) <= 32 &&
		a.Tokens.FontSize >= 8 && a.Tokens.FontSize <= 32 && a.Tokens.Radius >= 0 && a.Tokens.Radius <= 32 &&
		a.Tokens.Padding >= 0 && a.Tokens.Padding <= 32 && a.Tokens.Gap >= 0 && a.Tokens.Gap <= 32 &&
		a.Tokens.RowHeight >= 24 && a.Tokens.RowHeight <= 128 && a.Tokens.SearchHeight >= 24 && a.Tokens.SearchHeight <= 64 &&
		a.Tokens.TransitionMs >= 0 && a.Tokens.TransitionMs <= 1000 &&
		a.Tokens.RegularWeight >= 100 && a.Tokens.RegularWeight <= 900 && a.Tokens.EmphasisWeight >= 100 && a.Tokens.EmphasisWeight <= 900 &&
		len(a.Tokens.FontFamilies) >= 1 && len(a.Tokens.FontFamilies) <= 8
}

func Apply(t *ui.Theme, tokens Tokens) {
	t.Background, t.Surface = ui.Hex(tokens.Background), ui.Hex(tokens.Panel)
	t.Text, t.TextMuted = ui.Hex(tokens.Text), ui.Hex(tokens.Muted)
	t.Accent, t.Selection, t.Border = ui.Hex(tokens.Accent), ui.Hex(tokens.Selected), ui.Hex(tokens.Border)
	t.Radius, t.FontSize = float32(tokens.Radius), float32(tokens.FontSize)
	if len(tokens.FontFamilies) > 0 {
		t.Font = tokens.FontFamilies[0]
	}
}
