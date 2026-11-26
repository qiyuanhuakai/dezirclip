package main

import (
	"context"
	"slices"
	"strings"
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
	matches              []characterGroup
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

func (n *nativeMain) emojiView(c *ui.Context) {
	tab := n.preferences["app.emoji_panel_tab"]
	if tab != "favorites" && tab != "symbols" {
		tab = "emoji"
	}
	kit := n.mainKit()
	width, _ := c.Size()
	kit.CharacterPanel(c, width, func(contentWidth float32) {
		kit.CharacterTabs(c, []uikit.CharacterTab{
			{ID: "emoji", Label: n.text("emoji_tab")},
			{ID: "symbols", Label: n.text("emoji_symbols_tab")},
			{ID: "favorites", Label: n.text("emoji_favorites")},
		}, tab, n.mutating, func(selection string) {
			n.saveSetting("app.emoji_panel_tab", selection)
		})
		if tab == "favorites" {
			n.emojiFavoritesView(c, contentWidth)
			return
		}
		n.drawEmojiResults(c, kit, tab, contentWidth)
	})
}

func (n *nativeMain) drawEmojiResults(c *ui.Context, kit uikit.Kit, tab string, contentWidth float32) {
	panel := n.emojiPanel()
	kit.CharacterSearch(c, &panel.query, n.text("emoji_search"), func(composing bool) {
		if !composing && (panel.tab != tab || panel.searched != panel.query) {
			panel.matches = slices.Clone(searchCharacters(tab, panel.query))
			for i := range panel.matches {
				group := &panel.matches[i]
				group.choices = previewCharacterChoices(group.choices, tab, strings.TrimSpace(panel.query) != "")
			}
			panel.tab, panel.searched = tab, panel.query
			panel.characters = ui.ListState{}
		}
	})
	groups := make([]uikit.CharacterGroup, 0, len(panel.matches))
	for _, group := range panel.matches {
		groups = append(groups, uikit.CharacterGroup{Label: n.emojiGroupName(group.ID), Choices: group.choices})
	}
	label := n.text("emoji_tab")
	if tab == "symbols" {
		label = n.text("emoji_symbols_tab")
	}
	kit.CharacterList(c, &panel.characters, contentWidth, groups, label,
		n.text("emoji_search_empty"), n.mutating, func(value string) { n.sendEmoji("text", value) })
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
