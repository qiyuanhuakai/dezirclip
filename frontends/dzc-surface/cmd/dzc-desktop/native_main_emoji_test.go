package main

import "testing"

func TestCharacterCatalogSearch(t *testing.T) {
	for _, example := range []struct{ tab, query, value string }{
		{"emoji", "笑脸", "😀"},
		{"emoji", "愛心", "❤️"},
		{"emoji", ":grinning:", "😀"},
		{"emoji", "❤", "❤️"},
		{"symbols", "U+2A7D", "⩽"},
		{"symbols", "slanted equal", "⩽"},
		{"symbols", "左右雙箭頭", "⇔"},
	} {
		t.Run(example.tab+"/"+example.query, func(t *testing.T) {
			for _, group := range searchCharacters(example.tab, example.query) {
				for _, entry := range group.Entries {
					if entry.Value == example.value {
						return
					}
				}
			}
			t.Fatalf("search did not include %q", example.value)
		})
	}
	if len(searchCharacters("symbols", "😀")) != 0 {
		t.Fatal("emoji presentation leaked into the standalone symbol results")
	}
}
