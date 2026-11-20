package main

import (
	"context"
	_ "embed"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

//go:embed emoji-data.json
var emojiCatalogJSON []byte

const characterPageSize = 160

type characterCatalog struct {
	Emoji   []characterGroup
	Symbols []characterGroup
}

type characterGroup struct {
	ID      string
	Entries []catalogCharacter
	choices []uikit.CharacterChoice
}

type catalogCharacter struct {
	uikit.CharacterChoice
	Keywords []string
	search   string
}

var characterNormalizer = strings.NewReplacer("\ufe0e", "", "\ufe0f", "")

func normalizeCharacterSearch(text string) string {
	return characterNormalizer.Replace(strings.ToLower(text))
}

var emojiCatalog = sync.OnceValue(func() characterCatalog {
	var catalog characterCatalog
	if err := json.Unmarshal(emojiCatalogJSON, &catalog); err != nil {
		panic(err)
	}
	for _, groups := range [][]characterGroup{catalog.Emoji, catalog.Symbols} {
		for groupIndex := range groups {
			group := &groups[groupIndex]
			group.choices = make([]uikit.CharacterChoice, len(group.Entries))
			for i := range group.Entries {
				entry := &group.Entries[i]
				words := append([]string{entry.Value, entry.Name}, entry.Keywords...)
				for _, value := range entry.Value {
					words = append(words, fmt.Sprintf("u+%04x", value))
				}
				entry.search = normalizeCharacterSearch(strings.Join(words, " "))
				group.choices[i] = entry.CharacterChoice
			}
		}
	}
	return catalog
})

func searchCharacters(tab, query string) []characterGroup {
	catalog := emojiCatalog()
	groups := catalog.Emoji
	if tab == "symbols" {
		groups = catalog.Symbols
	}
	terms := strings.Fields(strings.Trim(normalizeCharacterSearch(strings.TrimSpace(query)), ":"))
	if len(terms) == 0 {
		return groups
	}
	var result []characterGroup
	for _, group := range groups {
		filtered := characterGroup{ID: group.ID}
		for _, entry := range group.Entries {
			matches := true
			for _, term := range terms {
				if !strings.Contains(entry.search, term) {
					matches = false
					break
				}
			}
			if matches {
				filtered.Entries = append(filtered.Entries, entry)
				filtered.choices = append(filtered.choices, entry.CharacterChoice)
			}
		}
		if len(filtered.Entries) != 0 {
			result = append(result, filtered)
		}
	}
	return result
}

type nativeEmojiPanel struct {
	paths                []string
	favorites            ui.ListState
	thumbnails           nativeVisibleCache[string, ui.ImageSource]
	read                 uint64
	ready, loading       bool
	loadError            bool
	query, searched, tab string
	matches              []characterGroup
	limit                int
	scroll               ui.ScrollState
}

func (n *nativeMain) openEmoji() {
	n.resetPreviewSelection()
	panel := n.emojiPanel()
	panel.query, panel.searched, panel.tab = "", "", ""
	n.showEmoji = true
}

func (n *nativeMain) closeEmoji() {
	if !n.mutating {
		n.showEmoji = false
	}
}

func (n *nativeMain) emojiView(c *ui.Context) {
	tab := n.preferences["app.emoji_panel_tab"]
	if tab != "favorites" && tab != "symbols" {
		tab = "emoji"
	}
	kit := n.mainKit()
	ui.Column(c).FillWidth().Padding(0, 8).Children(func() {
		kit.CharacterTabs(c, []uikit.CharacterTab{
			{ID: "emoji", Label: n.text("emoji_tab")},
			{ID: "symbols", Label: n.text("emoji_symbols_tab")},
			{ID: "favorites", Label: n.text("emoji_favorites")},
		}, tab, n.mutating, func(selection string) {
			n.saveSetting("app.emoji_panel_tab", selection)
		})
	})
	if tab == "favorites" {
		n.emojiFavoritesView(c)
		return
	}
	panel := n.emojiPanel()
	ui.Column(c).FillWidth().Padding(0, 8).Children(func() {
		kit.ClipboardSearch(c, uikit.HeaderOptions{
			Query: &panel.query, Placeholder: n.text("emoji_search"),
			OnQueryEdit: func(composing bool) {
				if !composing && (panel.tab != tab || panel.searched != panel.query) {
					panel.matches = searchCharacters(tab, panel.query)
					panel.tab, panel.searched, panel.limit = tab, panel.query, characterPageSize
					panel.scroll = ui.ScrollState{}
				}
			},
		})
	})
	width, _ := c.Size()
	ui.Scroll(c).FillWidth().MinHeight(0).Grow(1).Shrink(1).TrackScroll(&panel.scroll).Children(func() {
		ui.Column(c).FillWidth().Padding(0, 8).Children(func() {
			kit.CharacterContent(c, width-16, func(contentWidth float32) {
				remaining, total := panel.limit, 0
				for _, group := range panel.matches {
					total += len(group.Entries)
					if remaining == 0 {
						continue
					}
					visible := min(remaining, len(group.Entries))
					kit.CharacterGrid(c, n.emojiGroupName(group.ID), group.choices[:visible], contentWidth, n.mutating, func(value string) {
						n.sendEmoji("text", value)
					})
					remaining -= visible
				}
				if total == 0 {
					kit.SettingsLabel(c, n.text("emoji_search_empty")).AlignSelf(ui.Center)
				} else if total > panel.limit && kit.Button(c, uikit.ButtonOptions{Label: n.text("emoji_more")}).
					AlignSelf(ui.Center).Clicked() {
					panel.limit += characterPageSize
				}
			})
		})
	})
}

func (n *nativeMain) emojiGroupName(name string) string {
	switch name {
	case "expressions":
		return n.text("emoji_group_expressions")
	case "people":
		return n.text("emoji_group_people")
	case "animals":
		return n.text("emoji_group_animals")
	case "food":
		return n.text("emoji_group_food")
	case "activities":
		return n.text("emoji_group_activities")
	case "travel":
		return n.text("emoji_group_travel")
	case "objects":
		return n.text("emoji_group_objects")
	case "emojiSymbols":
		return n.text("emoji_group_emojisymbols")
	case "flags":
		return n.text("emoji_group_flags")
	case "arrows":
		return n.text("emoji_group_arrows")
	case "math":
		return n.text("emoji_group_math")
	case "boxDrawing":
		return n.text("emoji_group_boxdrawing")
	case "currency":
		return n.text("emoji_group_currency")
	case "otherSymbols":
		return n.text("emoji_group_othersymbols")
	case "punctuation":
		return n.text("emoji_group_punctuation")
	default:
		return name
	}
}

func (n *nativeMain) sendEmoji(kind, value string) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating, n.notice = true, ""
	n.actions.diagnostics.record("emoji-send-requested", 0, nil)
	session, epoch := n.session, n.history.state.PrivacyEpoch
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		outcome := ""
		paste, err := n.canAutoPaste(ctx)
		if err == nil {
			outcome, err = n.actions.SendEmoji(ctx, kind, value, epoch, paste)
		}
		if err == nil && paste && outcome == "target_required" {
			n.actions.diagnostics.record("emoji-paste-target-unavailable", 0, nil)
			paste = false
			outcome, err = n.actions.SendEmoji(ctx, kind, value, epoch, false)
		}
		stage := "emoji-paste-completed"
		if err != nil || outcome != "completed" {
			stage = "emoji-send-failed"
		} else if !paste {
			stage = "emoji-copy-completed"
		}
		n.actions.diagnostics.record(stage, 0, err)
		n.update(session, func() {
			n.mutating = false
			switch {
			case err != nil || outcome != "completed":
				n.notice = n.text("native_emoji_send_failed")
			case !paste:
				n.notice = n.text("screenshot_captured")
			}
		})
	}()
}
