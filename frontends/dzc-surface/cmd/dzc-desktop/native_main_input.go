package main

import (
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
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
	if n.tagLibrary == nil && n.tagSuggestionKey(key) {
		return true
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
	reveal := nativeConcealed(row)
	if reveal && !n.canUseProtectedRow(row) {
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
	n.preview.present(n.window, n.preview.generation+1, row.Id, n.history.state, reveal, placement, a)
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
	opacity := opacityPreference(n.preferences["app.preview_opacity"], n.preferences["app.surface_opacity"])
	language := string(n.language())
	preset := n.preset()
	appearance := theme.Appearance{Mode: n.mode, Locale: language, Opacity: float64(opacity) / 100, Tokens: preset.Tokens()}
	if tint := preset.SurfaceTint(); tint != "" {
		appearance.Tokens.Background = tint
	}
	if font := nativeInterfaceFont(n.preferences); font != "" {
		appearance.Tokens.FontFamilies = []string{font}
	}
	return previewAppearance{
		Language: language, Palette: &appearance, Opacity: opacity, Dark: n.mode == "dark",
		Title:        n.text("preview_toolbar_title"),
		Pin:          n.text("preview_toolbar_pin"),
		Close:        n.text("preview_toolbar_close"),
		Retry:        n.text("common_retry"),
		Concealed:    n.text("native_preview_hidden"),
		Failed:       n.text("native_preview_failed"),
		OpacityLabel: n.text("preview_toolbar_opacity"),
		ZoomIn:       n.text("preview_toolbar_zoom_in"),
		ZoomOut:      n.text("preview_toolbar_zoom_out"),
		Fit:          n.text("preview_toolbar_fit"),
		FitLabel:     n.text("preview_toolbar_fit_label"),
		Actual:       n.text("preview_toolbar_actual", "Actual size"),
		ImageZoom:    n.text("preview_toolbar_zoom"),
	}
}

func (n *nativeMain) historyShortcuts(c *ui.Context, queryFocused bool) {
	if n.showEmoji && !n.showSettings {
		c.OnShortcut(0, ui.KeyEscape, n.closeEmoji)
		return
	}
	if n.showSettings || n.hasHistoryDialog() {
		return
	}
	c.OnShortcut(ui.Ctrl, ui.KeyF, func() { n.focusSearch = true })
	if n.tagLibrary == nil {
		c.OnShortcut(ui.Ctrl, ui.KeyN, func() { n.addHistoryEntry("") })
	}
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
		contentDisabled := disabled || nativeConcealed(row) && n.protectedBody(row) == nil
		if editableHistoryKind(row.ContentType) && menu.Item(n.text("edit_item")).Disabled(contentDisabled).Chosen() {
			n.editHistoryEntry(row, generation)
		}
		if menu.Item(n.text("preview_toolbar_title")).Disabled(contentDisabled).Chosen() {
			n.hover.reset()
			n.showPreview(row, generation, card.Bounds())
		}
		if menu.Item(n.text("common_copy")).Disabled(contentDisabled).Chosen() {
			n.rowAction(row, generation, nativeCopy)
		}
		if menu.Item(n.text("common_paste")).Disabled(contentDisabled).Chosen() {
			n.rowAction(row, generation, nativePaste)
		}
		if menu.Item(n.text("native_paste_formatted")).Disabled(contentDisabled).Chosen() {
			n.rowAction(row, generation, nativeRichPaste)
		}
		if row.ContentType == "image" && menu.Item(n.text("native_copy_base64")).Disabled(contentDisabled).Chosen() {
			n.rowAction(row, generation, nativeBase64)
		}
		if row.ContentType == "image" && row.Id > 0 && !nativeConcealed(row) {
			status := n.ocrStatus(row, nil)
			label := n.text("native_ocr_recognize")
			if status == "processing" {
				label = n.text("ocr_processing")
			}
			if menu.Item(label).Disabled(!n.eligibleOCR(row, generation) || n.private ||
				n.ocrJob.status == "processing" || status == "processing").Chosen() {
				n.recognizeOCR(row, generation)
			}
			if status == "done" && menu.Item(n.text("native_ocr_copy")).Disabled(!n.eligibleOCR(row, generation)).Chosen() {
				n.copyOCR(row, generation)
			}
		}
		if nativeTextToolEntry(row.ContentType) && !nativeConcealed(row) &&
			menu.Item(n.text("native_qr_code")).Disabled(disabled).Chosen() {
			n.openQR(row, generation)
		}
		if nativeTextToolEntry(row.ContentType) && !nativeConcealed(row) &&
			menu.Item(n.text("native_transformations")).Disabled(disabled).Chosen() {
			n.openTransform(row, generation)
		}
		if row.Id > 0 && menu.Item(n.text("open")).Disabled(contentDisabled).Chosen() {
			n.rowAction(row, generation, nativeOpen)
		}
		if menu.Item(n.text("native_edit_tags")).Disabled(disabled).Chosen() {
			n.editTags(row, generation)
		}
		if row.Id != 0 && menu.Item(n.text("privacy_management")).Disabled(!n.canOpenHistoryDialog() || disabled).Chosen() {
			n.openPrivacyFeedback(row, generation)
		}
	})
}
