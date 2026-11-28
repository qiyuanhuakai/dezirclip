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
	paths          []string
	favorites      ui.ListState
	thumbnails     nativeVisibleCache[string, ui.ImageSource]
	read           uint64
	ready, loading bool
	changing       bool
	loadError      bool
	query, nextTab string
	visibleTab     string
	searching      bool
	results        map[string]*nativeCharacterResults
}

type nativeCharacterResults struct {
	query    string
	matches  []characterGroup
	groups   []uikit.CharacterGroup
	language nativeLanguage
	list     ui.ListState
}

func (n *nativeMain) openEmoji() {
	n.resetPreviewSelection()
	panel := n.emojiPanel()
	panel.query, panel.visibleTab = "", ""
	n.showEmoji = true
}

func (n *nativeMain) closeEmoji() {
	if !n.mutating {
		n.showEmoji = false
	}
}

const nativeEmojiTabSetting = "app.emoji_panel_tab"

func (n *nativeMain) emojiTab() string {
	if n.emoji != nil && n.emoji.nextTab != "" {
		return n.emoji.nextTab
	}
	switch tab := n.displayedSetting(nativeEmojiTabSetting, "emoji"); tab {
	case "favorites", "symbols":
		return tab
	default:
		return "emoji"
	}
}

func (n *nativeMain) saveEmojiTab(panel *nativeEmojiPanel) {
	if panel.nextTab == "" || n.preferenceUnavailable(nativeEmojiTabSetting) {
		return
	}
	tab := panel.nextTab
	panel.nextTab = ""
	n.savePreference(nativeEmojiTabSetting, tab, 4*time.Second, func(ctx context.Context, epoch uint64) error {
		return n.library.SetSetting(ctx, nativeEmojiTabSetting, tab, epoch)
	}, func(error) {
		if n.emoji == panel {
			n.saveEmojiTab(panel)
		}
	})
}

func (n *nativeMain) emojiView(c *ui.Context) {
	tab := n.emojiTab()
	kit := n.mainKit()
	width, _ := c.Size()
	kit.CharacterPanel(c.Key("character-panel"), width, func(contentWidth float32) {
		panel := n.emojiPanel()
		kit.CharacterTabs(c.Key("character-tabs"), []uikit.CharacterTab{
			{ID: "emoji", Label: n.text("emoji_tab")},
			{ID: "symbols", Label: n.text("emoji_symbols_tab")},
			{ID: "favorites", Label: n.text("emoji_favorites")},
		}, tab, n.preferenceUnavailable(""), func(selection string) {
			if selection != tab {
				panel.nextTab = selection
				n.saveEmojiTab(panel)
			}
		})
		if tab == "favorites" {
			n.ensureEmojiFavorites(panel)
		}
		var composing bool
		if tab != "favorites" || panel.loading && panel.visibleTab != "" && panel.visibleTab != "favorites" {
			ui.Column(c.Key("character-search")).FillWidth().Transition(nativeSettingReveal).Children(func() {
				kit.CharacterSearch(c, &panel.query, n.text("emoji_search"), func(editingComposition bool) {
					composing = editingComposition
				})
			})
		}
		query := strings.TrimSpace(panel.query)
		results := panel.results[tab]
		if tab != "favorites" && !composing && (results == nil || results.query != query) {
			n.refreshEmojiResults(panel, tab, query)
			results = panel.results[tab]
		}
		// Keep the current results mounted until the replacement exists. The
		// tab strip and search input have independent, stable identity.
		visible := tab
		if panel.visibleTab != "" && panel.visibleTab != tab {
			waiting := tab == "favorites" && panel.loading ||
				tab != "favorites" && !composing && (results == nil || results.query != query)
			if waiting && (panel.results[panel.visibleTab] != nil || panel.visibleTab == "favorites" && panel.ready) {
				visible = panel.visibleTab
				results = panel.results[visible]
			}
		}
		panel.visibleTab = visible
		ui.Column(c.Key("character-tab-content")).FillWidth().Grow(1).Shrink(1).MinHeight(0).Clip().Children(func() {
			ui.Column(c.Key(visible)).Fill().Shrink(1).MinHeight(0).Transition(nativePanelEnter).Children(func() {
				switch {
				case visible == "favorites":
					n.emojiFavoritesView(c, contentWidth, visible != tab)
				case results == nil:
					kit.SettingsLabel(c, n.text("common_loading", "Loading…")).AlignSelf(ui.Center)
				default:
					pending := visible != tab || !composing && results.query != query
					n.drawEmojiResults(c, kit, visible, contentWidth, results, pending)
				}
			})
		})
	})
}

func (n *nativeMain) drawEmojiResults(c *ui.Context, kit uikit.Kit, tab string, contentWidth float32, results *nativeCharacterResults, pending bool) {
	if language := n.language(); results.language != language || results.groups == nil {
		results.language = language
		results.groups = make([]uikit.CharacterGroup, 0, len(results.matches))
		for _, group := range results.matches {
			results.groups = append(results.groups, uikit.CharacterGroup{Label: n.emojiGroupName(group.ID), Choices: group.choices})
		}
	}
	label := n.text("emoji_tab")
	if tab == "symbols" {
		label = n.text("emoji_symbols_tab")
	}
	kit.CharacterList(c.Key("character-results"), &results.list, contentWidth, results.groups, label,
		n.text("emoji_search_empty"), n.mutating || pending, func(value string) { n.sendEmoji("text", value) })
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

func (n *nativeMain) refreshEmojiResults(panel *nativeEmojiPanel, tab, query string) {
	if panel.searching {
		return
	}
	find := func() []characterGroup {
		if query == "" {
			if tab == "symbols" {
				return browseSymbols()
			}
			return browseEmoji()
		}
		return characterChoices(tab, query)
	}
	apply := func(matches []characterGroup) {
		if panel.results == nil {
			panel.results = make(map[string]*nativeCharacterResults, 2)
		}
		panel.results[tab] = &nativeCharacterResults{query: query, matches: matches}
	}
	session := n.session
	if session == nil {
		apply(find())
		return
	}
	panel.searching = true
	go func() {
		matches := find()
		n.updateIfChanged(session, func() bool {
			panel.searching = false
			if n.emoji != panel || !n.showEmoji {
				return false
			}
			if strings.TrimSpace(panel.query) == query {
				apply(matches)
			}
			// If the query or tab changed while the worker was busy,
			// the next frame can start the latest request.
			return n.emojiTab() != "favorites"
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
