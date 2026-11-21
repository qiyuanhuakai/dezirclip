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

func (n *nativeMain) emojiFavoritesView(c *ui.Context, contentWidth float32) {
	panel := n.emojiPanel()
	if !panel.ready && !panel.loading && !panel.loadError && !n.mutating {
		n.loadEmojiFavorites(panel)
	}
	width, _ := c.Size()
	cell, gap := float32(74), float32(8)
	if width <= 520 {
		cell, gap = 66, 6
	}
	columns := max(1, int((contentWidth-8+gap)/(cell+gap)))
	count := len(panel.paths) + 1
	rows := (count + columns - 1) / columns
	kit := n.mainKit()
	ui.List(c, &panel.favorites, rows).Grow(1).FillWidth().Gap(gap).Label(n.text("emoji_favorites", "Favorite images")).Rows(func(row ui.ListRow) {
		start := row.Index * columns
		ui.Row(row.Context).FillWidth().Padding(4, 4).Gap(gap).Justify(ui.Center).Children(func() {
			for index := start; index < min(count, start+columns); index++ {
				if index == 0 {
					if kit.EmojiAddTile(row.Context, n.text("emoji_add_files"), cell, n.mutating || n.private).Clicked() {
						n.changeEmojiFavorites("import", nil, true)
					}
					continue
				}
				n.emojiFavoriteCard(row.Context, kit, panel, panel.paths[index-1], cell)
			}
			if empty := columns - min(columns, count-start); empty > 0 && len(panel.paths) > 0 {
				ui.Box(row.Context).Width(float32(empty)*(cell+gap) - gap)
			}
		})
	})
	message := n.text("emoji_fav_tip")
	if n.private {
		message = n.text("native_favorites_read_only")
	} else if panel.loading {
		message = n.text("common_loading", "Loading favorites…")
	} else if len(panel.paths) == 0 {
		message = n.text("emoji_fav_hint", "Choose images or drop them here to add favorites.")
	}
	ui.Text(c, message).FillWidth().Padding(8).FontSize(11).TextAlign(ui.Center).Wrap().TextColor(c.Theme().TextMuted)
	if panel.loadError && kit.Button(c, uikit.ButtonOptions{Label: n.text("common_retry", "Retry loading favorites")}).Clicked() {
		n.loadEmojiFavorites(panel)
	}
}

func (n *nativeMain) emojiFavoriteCard(c *ui.Context, kit uikit.Kit, panel *nativeEmojiPanel, path string, width float32) {
	name := filepath.Base(path)
	preview, loaded := panel.thumbnails.lookup(path)
	if !loaded {
		n.loadEmojiThumbnail(panel, path)
	}
	options := uikit.EmojiFavoriteOptions{
		Name: name, Width: width, Disabled: n.mutating, RemoveLabel: n.text("native_remove_favorite"),
		OnChoose: func() { n.sendEmoji("image", path) },
		Preview: func() {
			if preview != nil {
				ui.Image(c, preview).Fill().Fit(ui.Contain).PassThrough()
			} else {
				ui.Text(c, name).FontSize(11).MaxLines(2).PassThrough()
			}
		},
	}
	if !n.private {
		options.OnRemove = func() { n.changeEmojiFavorites("remove", []string{path}, false) }
	}
	kit.EmojiFavoriteTile(c.Key(path), options)
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
	language := n.language()
	go func() {
		var err error
		var order []string
		if choose {
			paths, err = mygo.Dialog.Open(mygo.OpenDialogOptions{
				Parent: window, Title: language.text("emoji_add_files", "Add favorite images"), Multiple: true,
				Filters: []mygo.FileFilter{{Name: language.text("native_images"), Extensions: []string{"png", "jpg", "jpeg", "webp", "gif"}}},
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
				n.notice = n.text("native_favorites_failed")
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
