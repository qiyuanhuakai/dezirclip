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
		{"symbols", "約等", "≈"},
		{"symbols", "平方根", "√"},
		{"symbols", "向右的虛線箭頭", "⇢"},
		{"symbols", "向右的虛箭頭", "⇢"},
		{"emoji", "笑到打滕", "🤣"},
		{"symbols", "段落符號", "¶"},
		{"symbols", "圆周率", "π"},
		{"symbols", "slnted equal", "⩽"},
		{"symbols", "inifinity", "∞"},
		{"symbols", "ＩＮＦＩＮＩＴＹ", "∞"},
		{"symbols", ":", ":"},
		{"emoji", "笑到打滾", "🤣"},
		{"emoji", "puppy eyes", "🥺"},
		{"emoji", "buterfly", "🦋"},
		{"emoji", "点赞", "👍"},
		{"emoji", "比個心", "🫶"},
		{"emoji", "中国国旗", "🇨🇳"},
		{"emoji", "英國旗幟", "🇬🇧"},
		{"emoji", "EU flag", "🇪🇺"},
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

func TestCharacterSearchRanksPracticalNamesFirst(t *testing.T) {
	for _, example := range []struct{ tab, query, value string }{
		{"symbols", "打勾", "✓"},
		{"emoji", "笑到哭", "😂"},
	} {
		groups := searchCharacters(example.tab, example.query)
		if len(groups) == 0 || groups[0].Entries[0].Value != example.value {
			t.Fatalf("%q did not return %q first", example.query, example.value)
		}
	}
}
