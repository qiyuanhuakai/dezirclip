package main

import (
	"math"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

func (n *nativeMain) selectedEntry() (protocol.DesktopEntry, bool) {
	i := n.history.selected
	if n.session == nil || n.failed || i < 0 || i >= len(n.history.rows) {
		return protocol.DesktopEntry{}, false
	}
	return n.history.rows[i], true
}

func (n *nativeMain) navigateHistory(delta int) {
	if n.session == nil || n.failed || len(n.history.rows) == 0 {
		return
	}
	if n.hover.owns(n.preview) {
		n.closePreview()
	}
	n.hover.reset()
	if n.keyboard {
		n.history.selected = max(0, min(len(n.history.rows)-1, n.history.selected+delta))
	} else {
		n.history.selected = 0
		n.keyboard = true
	}
	n.list.List.ScrollTo(n.history.selected, ui.Center)
}

func (n *nativeMain) searchKey(key ui.TextKey) bool {
	if key.Mods != 0 || n.showSettings || n.hasHistoryDialog() {
		return false
	}
	switch key.Key {
	case ui.KeyEscape:
		if !n.closePreview() {
			n.focusList = true
			if n.tagLibrary == nil && n.query == "" && n.preferences["app.show_search_box"] == "false" {
				n.temporarySearch = false
			}
		}
		return true
	case ui.KeyUp, ui.KeyDown:
		if n.preferences["app.arrow_key_selection"] != "true" {
			return false
		}
		if key.Text() == n.historyQuery() {
			if key.Key == ui.KeyUp {
				n.navigateHistory(-1)
			} else {
				n.navigateHistory(1)
			}
		}
		return true
	case ui.KeyEnter:
		if key.Text() == n.historyQuery() && n.keyboard {
			n.selectedAction(nativePaste)
		}
		return true
	}
	return false
}

func (n *nativeMain) selectedAction(action nativeRowAction) {
	if row, ok := n.selectedEntry(); ok {
		n.rowAction(row, n.history.generation, action)
	}
}

func (n *nativeMain) historyWheel(event ui.InputEvent) bool {
	if event.Kind != ui.InputScroll || event.Mods != 0 || event.Momentum ||
		n.tagLibrary != nil || n.hasHistoryDialog() || n.preferences["app.show_search_box"] != "false" {
		return false
	}
	if event.DY < -5 && n.historyScroll.Y > 0 {
		n.towardsTop = event.Time
		n.searchWheel = 0
		return false
	}
	if event.DY < -5 && !n.temporarySearch && event.Time.Sub(n.towardsTop) > 250*time.Millisecond {
		n.searchWheel -= event.DY
		if n.searchWheel > 45 {
			n.temporarySearch = true
			return true
		}
	} else {
		n.searchWheel = 0
	}
	if event.DY > 10 && n.temporarySearch && strings.TrimSpace(n.query) == "" {
		n.temporarySearch = false
		n.focusList = true
		return true
	}
	return false
}

func (n *nativeMain) closePreview() bool {
	if n.preview == nil || n.preview.entryID == 0 {
		return false
	}
	n.preview.clear()
	return true
}

func (n *nativeMain) showPreview(row protocol.DesktopEntry, generation uint64, bounds ui.Rect) {
	if n.preview == nil || n.window == nil || n.session == nil || n.history.generation != generation || n.failed || n.showSettings {
		return
	}
	if n.preview.entryID == row.Id {
		return
	}
	var placement *previewPlacement
	if bounds.W > 0 && bounds.H > 0 {
		source := mygo.Rectangle{X: int(bounds.X), Y: int(bounds.Y), Width: int(bounds.W), Height: int(bounds.H)}
		placement = &previewPlacement{
			Point: mygo.Point{X: source.X + source.Width + 12, Y: source.Y}, Source: source,
		}
	}
	a := n.previewAppearance()
	n.preview.present(n.window, n.preview.generation+1, row.Id, n.history.state, false, placement, a)
}

func (n *nativeMain) previewKeyboardRow(row protocol.DesktopEntry, generation uint64, bounds ui.Rect) {
	if n.previewSelection == row.Id || n.preferences["app.preview_keyboard_enabled"] != "true" {
		return
	}

	// A closed preview stays closed until keyboard navigation selects another row.
	n.previewSelection = row.Id
	n.showPreview(row, generation, bounds)
}

func (n *nativeMain) previewAppearance() previewAppearance {
	opacity := 65
	for _, key := range []string{"app.preview_opacity", "app.surface_opacity"} {
		value, err := strconv.ParseFloat(strings.TrimSpace(n.preferences[key]), 64)
		if err == nil && !math.IsNaN(value) && !math.IsInf(value, 0) {
			opacity = int(math.Round(max(0, min(100, value))))
			break
		}
	}
	language := n.preferences["app.language"]
	appearance := theme.Appearance{Mode: n.mode, Locale: language, Opacity: float64(opacity) / 100,
		Tokens: presets.Select(n.theme, n.mode).Tokens()}
	return previewAppearance{
		Language: language, Palette: &appearance, Opacity: opacity, Dark: n.mode == "dark",
		Title: "Preview", Pin: "Pin this preview", Close: "Close preview", Retry: "Retry", Concealed: "Preview hidden",
		Failed: "Preview action failed", OpacityLabel: "Background opacity", ZoomIn: "Zoom in", ZoomOut: "Zoom out",
		Fit: "Fit to window", FitLabel: "Fit", Actual: "Actual size", ImageZoom: "Image zoom",
	}
}

func (n *nativeMain) historyShortcuts(c *ui.Context, queryFocused bool) {
	if n.showSettings || n.hasHistoryDialog() {
		return
	}
	c.OnShortcut(ui.Ctrl, ui.KeyF, func() { n.focusSearch = true })
	if queryFocused {
		return
	}
	if n.preferences["app.arrow_key_selection"] == "true" {
		c.OnShortcut(0, ui.KeyUp, func() { n.navigateHistory(-1) })
		c.OnShortcut(0, ui.KeyDown, func() { n.navigateHistory(1) })
		c.OnShortcut(0, ui.KeyEnter, func() {
			if n.keyboard {
				n.selectedAction(nativePaste)
			}
		})
	}
	c.OnShortcut(ui.Ctrl, ui.KeyC, func() { n.selectedAction(nativeCopy) })
	c.OnShortcut(0, ui.KeyEscape, func() {
		if n.closePreview() {
			return
		}
		if n.keyboard && n.history.selected > 0 {
			n.history.selected = 0
			n.list.List.ScrollTo(0, ui.Start)
		} else if n.window != nil {
			n.shell.hideMain(n.window)
		}
	})
}

func (n *nativeMain) entryMenu(card ui.Element, row protocol.DesktopEntry, generation uint64) {
	card.ContextMenu(func(menu *ui.Menu) {
		disabled := n.mutating || n.failed || n.history.generation != generation
		if menu.Item("Preview").Disabled(disabled).Chosen() {
			n.hover.reset()
			n.showPreview(row, generation, card.Bounds())
		}
		if menu.Item("Copy").Disabled(disabled).Chosen() {
			n.rowAction(row, generation, nativeCopy)
		}
		if menu.Item("Paste").Disabled(disabled).Chosen() {
			n.rowAction(row, generation, nativePaste)
		}
		if menu.Item("Paste with formatting").Disabled(disabled).Chosen() {
			n.rowAction(row, generation, nativeRichPaste)
		}
		if row.ContentType == "image" && menu.Item("Copy image as Base64").Disabled(disabled).Chosen() {
			n.rowAction(row, generation, nativeBase64)
		}
		if row.IsExternal && menu.Item("Open").Disabled(disabled).Chosen() {
			n.rowAction(row, generation, nativeOpen)
		}
		if menu.Item("Edit tags").Disabled(disabled).Chosen() {
			n.editTags(row, generation)
		}
	})
}
