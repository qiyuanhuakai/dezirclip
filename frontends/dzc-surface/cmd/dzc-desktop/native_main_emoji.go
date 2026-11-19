package main

import (
	"context"
	_ "embed"
	"encoding/json"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
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
	ui.Row(c).Padding(0, 8).Gap(5).Children(func() {
		if uikit.Button(c, uikit.ButtonOptions{Label: "Emoji", Selected: !favorites, Disabled: n.mutating}).Clicked() && favorites {
			n.saveSetting("app.emoji_panel_tab", "emoji")
		}
		if uikit.Button(c, uikit.ButtonOptions{Label: "Favorites", Selected: favorites, Disabled: n.mutating}).Clicked() && !favorites {
			n.saveSetting("app.emoji_panel_tab", "favorites")
		}
	})
	if favorites {
		n.emojiFavoritesView(c)
		return
	}
	width, _ := c.Size()
	columns := max(1, int((width-16)/43))
	ui.Scroll(c).Grow(1).Children(func() {
		ui.Column(c).FillWidth().Padding(8).Gap(8).Children(func() {
			for _, group := range emojiCatalog {
				ui.Text(c, group.Name).FontWeight(600)
				ui.Grid(c).Columns(columns).Gap(4).Children(func() {
					for _, emoji := range group.Emojis {
						button := n.emojiButton(c, emoji, 35)
						button.Children(func() { ui.Text(c, emoji).FontSize(20).SingleLine() })
						if button.Clicked() {
							n.sendEmoji("text", emoji)
						}
					}
				})
			}
		})
	})
}

func (n *nativeMain) emojiButton(c *ui.Context, label string, height float32) ui.Element {
	button := ui.ButtonBase(c).Label(label).Height(height).FillWidth().Center().
		Radius(c.Theme().Radius).FocusRing(true).Disabled(n.mutating)
	if button.Hovered() {
		button.Background(c.Theme().SurfaceHover)
	}
	return button
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
				n.notice = "Could not send emoji."
			case !paste:
				n.notice = "Copied to clipboard"
			}
		})
	}()
}
