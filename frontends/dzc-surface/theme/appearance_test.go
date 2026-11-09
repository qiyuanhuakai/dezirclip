package theme

import (
	"math"
	"testing"
)

func TestAppearanceAcceptsFallbackAndRejectsUnboundedInput(t *testing.T) {
	for _, mode := range []string{"light", "dark"} {
		appearance := Appearance{Mode: mode, Opacity: .65, Locale: "zh-CN", Tokens: Select(mode)}
		if !appearance.Valid() { t.Fatal("default native appearance rejected") }
		for _, mutate := range []func(*Appearance){
			func(a *Appearance) { a.Opacity = math.NaN() },
			func(a *Appearance) { a.Tokens.RowHeight = 100000 },
			func(a *Appearance) { a.Tokens.FontFamilies = nil },
			func(a *Appearance) { a.Tokens.Text = "var(--text-primary)" },
			func(a *Appearance) { a.Tokens.Padding = -1 },
		} {
			invalid := appearance
			mutate(&invalid)
			if invalid.Valid() { t.Fatal("invalid native appearance accepted") }
		}
	}
}
