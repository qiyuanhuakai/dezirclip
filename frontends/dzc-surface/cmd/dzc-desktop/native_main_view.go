package main

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func (n *nativeMain) view(c *ui.Context) {
	preset := n.preset()
	kit := preset.Kit().WithInterfaceFont(nativeInterfaceFont(n.preferences))
	kit.Apply(c)
	var surfaceOpacity *int
	tint := preset.SurfaceTint()
	if tint != "" {
		opacity := opacityPreference(n.preferences["app.surface_opacity"])
		surfaceOpacity = &opacity
		palette := *c.Theme()
		// Paint the shell only once, below the controls.
		palette.Background = ui.Color{}
		c.SetTheme(&palette)
	}
	n.list.List.Selected = &n.history.selected
	n.list.List.Key = func(index int) any {
		if index >= 0 && index < len(n.history.rows) {
			return n.history.rows[index].Id
		}
		return nil
	}
	n.list.List.Label = func(index int) string {
		if index >= 0 && index < len(n.history.rows) {
			return nativeCardSummary(n.history.rows[index])
		}
		return ""
	}
	main := ui.Column(c).Fill().Gap(6).Background(c.Theme().Background)
	if n.preferences["app.show_app_border"] != "false" {
		main.Border(1, c.Theme().Border)
	}
	queryFocused := false
	main.Children(func() {
		queryComposing := false
		actions := []uikit.HeaderAction{
			{Icon: "pin", Label: "Keep on top", Active: n.pinned, Run: n.togglePin, Disabled: n.mutating},
			{Icon: "hide", Label: "Hide", Run: func() { n.shell.hideMain(n.window) }},
		}
		header := uikit.HeaderOptions{Title: "DezirClip", Actions: actions}
		if n.showSettings {
			header.Title = "Settings"
			header.Back = &uikit.HeaderAction{Icon: "back", Label: "Back", Run: func() { n.showSettings = false }}
		} else if n.showEmoji {
			header.Title = "Emoji"
			header.Back = &uikit.HeaderAction{Icon: "back", Label: "Back", Run: n.closeEmoji, Disabled: n.mutating}
		} else if n.tagLibrary != nil {
			header.Title = "Tags"
			header.Back = &uikit.HeaderAction{Icon: "back", Label: "Back", Run: n.closeTagLibrary, Disabled: n.mutating}
			header.Query, header.Placeholder = &n.tagLibrary.query, "Search tagged entries…"
		} else {
			header.Actions = append(actions[:1:1], uikit.HeaderAction{
				Icon: "settings", Label: "Settings", Run: n.openSettings,
			}, actions[1])
			if n.preferences["app.tag_manager_enabled"] != "false" {
				header.Actions = append([]uikit.HeaderAction{{
					Icon: "tag", Label: "Tags", Run: n.openTagLibrary, Disabled: n.mutating,
				}}, header.Actions...)
			}
			if n.preferences["app.emoji_panel_enabled"] == "true" {
				header.Actions = append([]uikit.HeaderAction{{
					Icon: "emoji", Label: "Emoji", Run: n.openEmoji, Disabled: n.mutating,
				}}, header.Actions...)
			}
			if n.focusSearch {
				n.temporarySearch = true
			}
			if n.preferences["app.show_search_box"] != "false" || n.temporarySearch || n.query != "" {
				header.Query, header.Placeholder, header.Filter = &n.query, "Search clipboard…", &n.kind
				header.Filters = []uikit.HeaderFilter{
					{ID: "text", Label: "Text"}, {ID: "image", Label: "Images"}, {ID: "files", Label: "Files"},
				}
			}
		}
		if header.Query != nil {
			header.FocusQuery = &n.focusSearch
			header.OnQueryEdit = func(composing bool) { queryComposing = composing }
			header.OnQueryKey = n.searchKey
			header.QueryFocus = func(focused bool) { queryFocused = focused }
		}
		kit.Header(c, header)
		if n.showSettings {
			n.settingsView(c, kit, surfaceOpacity)
		} else if n.showEmoji {
			n.emojiView(c)
		} else {
			if n.tagLibrary != nil {
				n.tagLibraryControls(c, kit, n.tagLibrary)
			}
			n.editQuery(queryComposing)
			if n.tagLibrary == nil || n.tagLibrary.selected != "" {
				n.historyView(c, kit)
			}
		}
		if n.notice != "" {
			ui.Text(c, n.notice).Padding(4, 8).TextColor(c.Theme().TextMuted)
		}
		if n.failed {
			ui.Row(c).Padding(8).AlignItems(ui.Center).Gap(8).Children(func() {
				ui.Text(c, "Clipboard service unavailable").Grow(1).TextColor(c.Theme().Danger)
				if uikit.Button(c, uikit.ButtonOptions{Label: "Retry"}).Clicked() {
					n.prepare(n.resume)
				}
			})
		}
	})
	if surfaceOpacity != nil {
		main.Background(ui.Hex(tint).Alpha(float32(*surfaceOpacity) / 100))
	}
	n.historyShortcuts(c, queryFocused)
	if n.tags != nil {
		n.tagEditorView(c, kit, n.tags)
	}
	if n.tagLibrary != nil && n.tagLibrary.dialog != nil {
		n.tagLibraryDialog(c, kit, n.tagLibrary)
	}
}

func (n *nativeMain) historyView(c *ui.Context, kit uikit.Kit) {
	if n.private {
		ui.Text(c, "Private session").TextColor(c.Theme().TextMuted).Padding(4, 8)
	}
	generation := n.history.generation
	compact := n.preferences["app.compact_mode"] == "true"
	itemSize, tagSize := nativeItemSize.value(n.preferences), nativeTagSize.value(n.preferences)
	var hovered protocol.DesktopEntry
	var hoverBounds ui.Rect
	list := kit.HistoryList(c, &n.list, uikit.HistoryListOptions{
		Count: len(n.history.rows), Label: "Clipboard history", EmptyLabel: "No clipboard entries",
		LoadingLabel: "Loading…", MoreLabel: "Load more…", Loading: n.reading, HasMore: n.history.more && !n.failed,
		Compact:    compact,
		OnLoadMore: n.loadPage,
		Row: func(cell ui.ListRow) {
			if generation != n.history.generation || cell.Index >= len(n.history.rows) {
				return
			}
			row := n.history.rows[cell.Index]
			pinLabel := "Pin entry"
			if row.IsPinned {
				pinLabel = "Unpin entry"
			}
			card := kit.HistoryCard(cell.Context, uikit.HistoryCardOptions{
				Source: row.SourceApp, Time: time.UnixMilli(row.Timestamp).Format("15:04"), Summary: nativeCardSummary(row),
				Pinned: row.IsPinned, Selected: cell.Selected(), Compact: compact,
				Match:    n.history.filter.query,
				TextFont: n.preferences["app.font_mono"],
				TextSize: float32(itemSize),
				TextRows: 4,
				TagSize:  float32(tagSize),
				Tags:     n.cardTags(row.Tags),
				Body:     n.cardBody(cell.Context, row, compact),
				Activate: func() {
					n.resetPreviewSelection()
					n.rowAction(row, generation, nativePaste)
				},
				Actions: []uikit.HeaderAction{
					{Icon: "pin", Label: pinLabel, Disabled: n.mutating, Run: func() { n.rowAction(row, generation, nativePin) }},
					{Icon: "hide", Label: "Delete entry", Disabled: n.mutating, Run: func() { n.rowAction(row, generation, nativeDelete) }},
				},
			})
			n.entryMenu(card, row, generation)
			if card.Hovered() {
				hovered, hoverBounds = row, card.Bounds()
			}
			if cell.Selected() && n.keyboard {
				n.previewKeyboardRow(row, generation, card.Bounds())
			}
		},
	})
	list.TrackScroll(&n.historyScroll).HandleInput(n.historyWheel)
	n.hoverPreview(hovered, generation, hoverBounds)
	if n.focusList {
		list.Focus()
		n.focusList = false
	}
	if list.Submitted() && n.history.selected >= 0 && n.history.selected < len(n.history.rows) {
		n.selectedAction(nativePaste)
	}
	if first, _ := n.list.List.Visible(); first > 0 && n.preferences["app.show_scroll_top_button"] != "false" {
		ui.Row(c).Padding(0, 8, 5).Justify(ui.End).Children(func() {
			if uikit.Button(c, uikit.ButtonOptions{Label: "Scroll to top"}).Clicked() {
				n.list.List.ScrollTo(0, ui.Start)
			}
		})
	}
}
