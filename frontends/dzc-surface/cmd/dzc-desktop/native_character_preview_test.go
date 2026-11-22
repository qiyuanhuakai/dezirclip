package main

import (
	"testing"

	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func TestCharacterFlagPreviews(t *testing.T) {
	flags := flagPreviews()
	for _, group := range searchCharacters("emoji", "") {
		if group.ID != "flags" {
			continue
		}
		if len(flags) != len(group.Entries) {
			t.Fatalf("flag previews: got %d, want %d", len(flags), len(group.Entries))
		}
		for _, entry := range group.Entries {
			preview := flags[entry.Value]
			if preview == nil || preview.Bitmap() == nil {
				t.Fatalf("missing flag artwork for %q", entry.Value)
			}
		}
		return
	}
	t.Fatal("flag catalog is missing")
}

func TestCharacterSymbolPreviewKeepsOriginalText(t *testing.T) {
	entries := []uikit.CharacterChoice{
		{Value: "\U0001D11E", Name: "Musical Symbol G Clef"},
		{Value: "\U0001CC00", Name: "Up-pointing Go-kart"},
	}
	browse := previewCharacterChoices(entries, "symbols", false)
	if len(browse) != 1 || browse[0].Value != entries[0].Value || browse[0].Preview.Font == "" {
		t.Fatalf("unexpected browsable symbols: %+v", browse)
	}
	results := previewCharacterChoices(entries, "symbols", true)
	if len(results) != 2 || results[1].Value != entries[1].Value || results[1].Preview.Code != "U+1CC00" {
		t.Fatalf("search failed to preserve the original symbol: %+v", results)
	}
}
