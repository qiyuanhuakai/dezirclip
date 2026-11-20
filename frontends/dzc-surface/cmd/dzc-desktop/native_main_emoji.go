package main

import (
	"context"
	_ "embed"
	"encoding/json"
	"time"

	"github.com/egoist/mygo/ui"
)

//go:embed emoji-data.json
var emojiCatalogJSON []byte

var emojiCatalog = func() []emojiGroup {
	var catalog struct{ Groups []emojiGroup }
	if err := json.Unmarshal(emojiCatalogJSON, &catalog); err != nil {
		panic(err)
	}
	return catalog.Groups
}()

type emojiGroup struct {
	Name   string
	Emojis []string
}

type nativeEmojiPanel struct {
	paths          []string
	favorites      ui.ListState
	thumbnails     nativeVisibleCache[string, ui.ImageSource]
	read           uint64
	ready, loading bool
	loadError      bool
}

func (n *nativeMain) openEmoji() {
	n.resetPreviewSelection()
	n.showEmoji = true
}

func (n *nativeMain) closeEmoji() {
	if !n.mutating {
		n.showEmoji = false
	}
}

func (n *nativeMain) emojiView(c *ui.Context) {
	favorites := n.preferences["app.emoji_panel_tab"] == "favorites"
	kit := n.mainKit()
	ui.Column(c).FillWidth().Padding(0, 8).Children(func() {
		kit.EmojiTabs(c, n.text("emoji_tab"), n.text("emoji_favorites"), favorites, n.mutating, func(favorites bool) {
			tab := "emoji"
			if favorites {
				tab = "favorites"
			}
			n.saveSetting("app.emoji_panel_tab", tab)
		})
	})
	if favorites {
		n.emojiFavoritesView(c)
		return
	}
	ui.Scroll(c).FillWidth().MinHeight(0).Grow(1).Shrink(1).Children(func() {
		ui.Column(c).FillWidth().Padding(8).Gap(8).Children(func() {
			for _, group := range emojiCatalog {
				kit.EmojiGrid(c, n.emojiGroupName(group.Name), group.Emojis, n.mutating, func(emoji string) {
					n.sendEmoji("text", emoji)
				})
			}
		})
	})
}

func (n *nativeMain) emojiGroupName(name string) string {
	switch name {
	case "常用":
		return n.text("native_emoji_frequent")
	case "表情":
		return n.text("native_emoji_expressions")
	case "人物":
		return n.text("native_emoji_people")
	case "手势":
		return n.text("native_emoji_gestures")
	case "动物":
		return n.text("native_emoji_animals")
	case "美食":
		return n.text("native_emoji_food")
	case "活动":
		return n.text("native_emoji_activities")
	case "旅行":
		return n.text("native_emoji_travel")
	case "物品":
		return n.text("native_emoji_objects")
	case "符号":
		return n.text("native_emoji_symbols")
	default:
		return name
	}
}

func (n *nativeMain) sendEmoji(kind, value string) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating, n.notice = true, ""
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
			paste = false
			outcome, err = n.actions.SendEmoji(ctx, kind, value, epoch, false)
		}
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
