package main

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"slices"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func (n *nativeMain) emojiPanel() *nativeEmojiPanel {
	if n.emoji == nil {
		n.emoji = &nativeEmojiPanel{}
	}
	return n.emoji
}

func (n *nativeMain) emojiFavoritesView(c *ui.Context) {
	panel := n.emojiPanel()
	if !panel.ready && !panel.loading && !panel.loadError && !n.mutating {
		n.loadEmojiFavorites(panel)
	}
	ui.Row(c).Padding(0, 8).Gap(6).Children(func() {
		if uikit.Button(c, uikit.ButtonOptions{Label: "Add images", Disabled: n.mutating || n.private}).Clicked() {
			n.changeEmojiFavorites("import", nil, true)
		}
		if panel.loadError && uikit.Button(c, uikit.ButtonOptions{Label: "Retry loading favorites"}).Clicked() {
			n.loadEmojiFavorites(panel)
		}
	})
	if len(panel.paths) == 0 {
		message := "Choose images or drop them here to add favorites."
		if n.private {
			message = "Image favorites are read-only during a private session."
		} else if panel.loading {
			message = "Loading favorites…"
		}
		ui.Text(c, message).Padding(8).Wrap().TextColor(c.Theme().TextMuted)
		return
	}
	width, _ := c.Size()
	columns := max(1, int((width-16)/80))
	rows := (len(panel.paths) + columns - 1) / columns
	ui.List(c, &panel.favorites, rows).Grow(1).FillWidth().Gap(5).Label("Favorite images").Rows(func(row ui.ListRow) {
		start := row.Index * columns
		if start >= len(panel.paths) {
			return
		}
		ui.Grid(row.Context).Columns(columns).Gap(5).Padding(0, 8).Children(func() {
			for _, path := range panel.paths[start:min(len(panel.paths), start+columns)] {
				n.emojiFavoriteCard(row.Context, panel, path)
			}
		})
	})
}

func (n *nativeMain) emojiFavoriteCard(c *ui.Context, panel *nativeEmojiPanel, path string) {
	name := filepath.Base(path)
	button := n.emojiButton(c, name, 75).Tooltip(name)
	preview, loaded := panel.thumbnails.lookup(path)
	if !loaded {
		n.loadEmojiThumbnail(panel, path)
	}
	button.Children(func() {
		if preview != nil {
			ui.Image(c, preview).Fill().Fit(ui.Contain)
		} else {
			ui.Text(c, name).FontSize(11).MaxLines(2)
		}
	})
	if button.Clicked() {
		n.sendEmoji("image", path)
	}
	button.ContextMenu(func(menu *ui.Menu) {
		if menu.Item("Remove favorite").Disabled(n.mutating || n.private).Chosen() {
			n.changeEmojiFavorites("remove", []string{path}, false)
		}
	})
}

func favoriteOrder(paths []string, saved string) []string {
	var preferred []string
	_ = json.Unmarshal([]byte(saved), &preferred)
	return orderedFavorites(paths, preferred)
}

func orderedFavorites(paths, preferred []string) []string {
	remaining := make(map[string]bool, len(paths))
	for _, path := range paths {
		remaining[path] = true
	}
	order := make([]string, 0, len(paths))
	for _, sequence := range [][]string{preferred, paths} {
		for _, path := range sequence {
			if remaining[path] {
				order = append(order, path)
				delete(remaining, path)
			}
		}
	}
	return order
}

func (n *nativeMain) loadEmojiFavorites(panel *nativeEmojiPanel) {
	if n.session == nil || n.history.state.ServiceInstanceId == "" {
		return
	}
	panel.loading, panel.loadError = true, false
	panel.read++
	session, epoch, request := n.session, n.history.state.PrivacyEpoch, panel.read
	order := n.preferences["app.emoji_favorites"]
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		result, err := n.library.EmojiFavorites(ctx, epoch, nil)
		n.update(session, func() {
			if n.emoji != panel || panel.read != request {
				return
			}
			panel.loading, panel.ready, panel.loadError = false, err == nil, err != nil
			if err == nil {
				panel.paths = favoriteOrder(result.Paths, order)
			}
		})
	}()
}

func (n *nativeMain) loadEmojiThumbnail(panel *nativeEmojiPanel, path string) {
	if n.session == nil || !panel.thumbnails.begin(path) {
		return
	}
	session, epoch := n.session, n.history.state.PrivacyEpoch
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		result, err := n.library.EmojiFavorites(ctx, epoch, &path)
		var preview ui.ImageSource
		if err == nil && result.Preview != nil {
			preview = previewSourceImage(*result.Preview)
		}
		n.update(session, func() {
			if n.emoji != panel {
				return
			}
			if !slices.Contains(panel.paths, path) {
				delete(panel.thumbnails.pending, path)
				return
			}
			panel.thumbnails.finish(path, preview)
		})
	}()
}

func (n *nativeMain) importEmojiPaths(paths []string) {
	if len(paths) == 0 || len(paths) > 256 {
		return
	}
	n.changeEmojiFavorites("import", slices.Clone(paths), false)
}

func (n *nativeMain) changeEmojiFavorites(action string, paths []string, choose bool) {
	if n.session == nil || n.mutating || n.private || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating, n.notice = true, ""
	session, epoch := n.session, n.history.state.PrivacyEpoch
	window := n.window
	panel := n.emojiPanel()
	preferences := n.preferences["app.emoji_favorites"]
	go func() {
		var err error
		var order []string
		if choose {
			paths, err = mygo.Dialog.Open(mygo.OpenDialogOptions{
				Parent: window, Title: "Add favorite images", Multiple: true,
				Filters: []mygo.FileFilter{{Name: "Images", Extensions: []string{"png", "jpg", "jpeg", "webp", "gif"}}},
			})
		}
		if err == nil && len(paths) > 0 {
			ctx, cancel := context.WithTimeout(session, 30*time.Second)
			defer cancel()
			order, err = n.saveEmojiFavorites(ctx, epoch, action, paths, preferences)
		}
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = "Some image favorites could not be updated."
			}
			if order != nil {
				panel.read++
				panel.loading, panel.ready, panel.loadError = false, true, false
				panel.paths = order
			} else if len(paths) > 0 {
				n.loadEmojiFavorites(panel)
			}
		})
	}()
}

func (n *nativeMain) saveEmojiFavorites(ctx context.Context, epoch uint64, action string, paths []string, saved string) ([]string, error) {
	var preferred []string
	_ = json.Unmarshal([]byte(saved), &preferred)
	var changeError error
	seen := make(map[string]bool, len(paths))
	for _, path := range paths {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if seen[path] {
			continue
		}
		seen[path] = true
		imported, err := n.library.MutateFavorite(ctx, epoch, action, path)
		if err != nil {
			changeError = errors.Join(changeError, err)
		} else if imported != "" {
			preferred = append(preferred, imported)
		}
	}
	result, err := n.library.EmojiFavorites(ctx, epoch, nil)
	if err != nil {
		return nil, errors.Join(changeError, err)
	}
	order := orderedFavorites(result.Paths, preferred)
	encoded, err := json.Marshal(order)
	if err == nil && string(encoded) != saved {
		err = n.library.SetSetting(ctx, "app.emoji_favorites", string(encoded), epoch)
	}
	return order, errors.Join(changeError, err)
}
