package main

import (
	"context"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

type nativeEmojiPanel struct {
	paths                []string
	favorites            ui.ListState
	thumbnails           nativeVisibleCache[string, ui.ImageSource]
	read                 uint64
	ready, loading       bool
	changing             bool
	loadError            bool
	query, searched, tab string
	searching            bool
	matches              []characterGroup
	groups               []uikit.CharacterGroup
	groupLanguage        nativeLanguage
	characters           ui.ListState
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

const nativeEmojiTabSetting = "app.emoji_panel_tab"

func (n *nativeMain) emojiTab() string {
	switch tab := n.preferences[nativeEmojiTabSetting]; tab {
	case "favorites", "symbols":
		return tab
	default:
		return "emoji"
	}
}

func (n *nativeMain) emojiView(c *ui.Context) {
	tab := n.emojiTab()
	kit := n.mainKit()
	width, _ := c.Size()
	kit.CharacterPanel(c, width, func(contentWidth float32) {
		kit.CharacterTabs(c, []uikit.CharacterTab{
			{ID: "emoji", Label: n.text("emoji_tab")},
			{ID: "symbols", Label: n.text("emoji_symbols_tab")},
			{ID: "favorites", Label: n.text("emoji_favorites")},
		}, tab, n.mutating, func(selection string) {
			n.saveSetting(nativeEmojiTabSetting, selection)
		})
		ui.Column(c.Key("character-tab-content")).FillWidth().Grow(1).Shrink(1).MinHeight(0).Clip().Children(func() {
			ui.Column(c.Key(tab)).Fill().Shrink(1).MinHeight(0).Transition(nativePanelEnter).Children(func() {
				if tab == "favorites" {
					n.emojiFavoritesView(c, contentWidth)
				} else {
					n.drawEmojiResults(c, kit, tab, contentWidth)
				}
			})
		})
	})
}

func (n *nativeMain) drawEmojiResults(c *ui.Context, kit uikit.Kit, tab string, contentWidth float32) {
	panel := n.emojiPanel()
	var composing bool
	kit.CharacterSearch(c, &panel.query, n.text("emoji_search"), func(editingComposition bool) {
		composing = editingComposition
	})
	if !composing && (panel.tab != tab || panel.searched != panel.query) {
		n.refreshEmojiResults(panel, tab)
	}
	if panel.tab != tab || !composing && panel.searched != panel.query {
		kit.SettingsLabel(c, n.text("common_loading", "Loading…")).AlignSelf(ui.Center)
		return
	}
	if language := n.language(); panel.groupLanguage != language || panel.groups == nil {
		panel.groupLanguage = language
		panel.groups = make([]uikit.CharacterGroup, 0, len(panel.matches))
		for _, group := range panel.matches {
			panel.groups = append(panel.groups, uikit.CharacterGroup{Label: n.emojiGroupName(group.ID), Choices: group.choices})
		}
	}
	label := n.text("emoji_tab")
	if tab == "symbols" {
		label = n.text("emoji_symbols_tab")
	}
	kit.CharacterList(c, &panel.characters, contentWidth, panel.groups, label,
		n.text("emoji_search_empty"), n.mutating, func(value string) { n.sendEmoji("text", value) })
}

var (
	browseEmoji   = sync.OnceValue(func() []characterGroup { return characterChoices("emoji", "") })
	browseSymbols = sync.OnceValue(func() []characterGroup { return characterChoices("symbols", "") })
)

func characterChoices(tab, query string) []characterGroup {
	groups := slices.Clone(searchCharacters(tab, query))
	searching := strings.TrimSpace(query) != ""
	for i := range groups {
		groups[i].choices = previewCharacterChoices(groups[i].choices, tab, searching)
	}
	return groups
}

func (n *nativeMain) refreshEmojiResults(panel *nativeEmojiPanel, tab string) {
	if panel.searching {
		return
	}
	query := panel.query
	find := func() []characterGroup {
		if strings.TrimSpace(query) == "" {
			if tab == "symbols" {
				return browseSymbols()
			}
			return browseEmoji()
		}
		return characterChoices(tab, query)
	}
	apply := func(matches []characterGroup) {
		panel.matches, panel.groups = matches, nil
		panel.tab, panel.searched = tab, query
		panel.characters = ui.ListState{}
	}
	session := n.session
	if session == nil {
		apply(find())
		return
	}
	panel.searching = true
	go func() {
		matches := find()
		n.update(session, func() {
			panel.searching = false
			if n.emoji == panel && panel.query == query && n.emojiTab() == tab {
				apply(matches)
			}
		})
	}()
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
