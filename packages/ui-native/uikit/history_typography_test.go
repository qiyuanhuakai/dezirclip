package uikit

import (
	"testing"

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
