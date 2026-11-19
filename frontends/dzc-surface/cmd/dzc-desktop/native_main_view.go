package main

import (
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

func (n *nativeMain) view(c *ui.Context) {
	kit := presets.Select(n.theme, n.mode).Kit()
	kit.Apply(c)
	n.list.List.Selected = &n.history.selected
	n.list.List.Key = func(index int) any {
		if index >= 0 && index < len(n.history.rows) {
			return n.history.rows[index].Id
		}
		return nil
	}
	n.list.List.Label = func(index int) string {
		if index >= 0 && index < len(n.history.rows) {
			return n.history.rows[index].Preview
		}
		return ""
	}
	ui.Column(c).Fill().Gap(6).Background(c.Theme().Background).Children(func() {
		queryComposing := false
		queryFocused := false
		actions := []uikit.HeaderAction{
			{Icon: "pin", Label: "Keep on top", Active: n.pinned, Run: n.togglePin, Disabled: n.mutating},
			{Icon: "hide", Label: "Hide", Run: func() { n.shell.hideMain(n.window) }},
		}
		header := uikit.HeaderOptions{Title: "DezirClip", Actions: actions}
		if n.showSettings {
			header.Title = "Settings"
			header.Back = &uikit.HeaderAction{Icon: "back", Label: "Back", Run: func() { n.showSettings = false }}
		} else if n.tagLibrary != nil {
			header.Title = "Tags"
			header.Back = &uikit.HeaderAction{Icon: "back", Label: "Back", Run: n.closeTagLibrary, Disabled: n.mutating}
			header.Query, header.Placeholder = &n.tagLibrary.query, "Search tagged entries…"
		} else {
			header.Actions = append(actions[:1:1], uikit.HeaderAction{
				Icon: "settings", Label: "Settings", Run: func() { n.resetPreviewSelection(); n.showSettings = true },
			}, actions[1])
			if n.preferences["app.tag_manager_enabled"] != "false" {
				header.Actions = append([]uikit.HeaderAction{{
					Icon: "tag", Label: "Tags", Run: n.openTagLibrary, Disabled: n.mutating,
				}}, header.Actions...)
			}
			header.Query, header.Placeholder, header.Filter = &n.query, "Search clipboard…", &n.kind
			header.Filters = []uikit.HeaderFilter{
				{ID: "text", Label: "Text"}, {ID: "image", Label: "Images"}, {ID: "files", Label: "Files"},
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
			n.settingsView(c, kit)
		} else {
			if n.tagLibrary != nil {
				n.tagLibraryControls(c, kit, n.tagLibrary)
			}
			n.editQuery(queryComposing)
			if n.tagLibrary == nil || n.tagLibrary.selected != "" {
				n.historyView(c, kit)
			}
			n.historyShortcuts(c, queryFocused)
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
				Source: row.SourceApp, Time: time.UnixMilli(row.Timestamp).Format("15:04"), Summary: row.Preview,
				Pinned: row.IsPinned, Selected: cell.Selected(), Compact: compact,
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
	n.hoverPreview(hovered, generation, hoverBounds)
	if n.focusList {
		list.Focus()
		n.focusList = false
	}
	if list.Submitted() && n.history.selected >= 0 && n.history.selected < len(n.history.rows) {
		n.selectedAction(nativePaste)
	}
}

func (n *nativeMain) settingsView(c *ui.Context, kit uikit.Kit) {
	ui.Column(c).Padding(12).Gap(8).Children(func() {
		ui.Text(c, "Theme").FontWeight(600)
		selected := n.theme
		if selected == "" {
			selected = "mica"
		}
		if kit.Select(c, &selected, "Theme", []uikit.SelectOption{
			{Value: "retro", Label: "Retro"}, {Value: "mica", Label: "Mica"}, {Value: "acrylic", Label: "Acrylic"},
			{Value: "liquid-glass", Label: "Liquid Glass"}, {Value: "macos", Label: "macOS"}, {Value: "scifi", Label: "Sci-Fi"},
		}, n.mutating).Changed() {
			n.saveSetting("app.theme", selected)
		}
		ui.Text(c, "Color mode").FontWeight(600)
		mode := n.preferences["app.color_mode"]
		if mode == "" {
			mode = "system"
		}
		if kit.Select(c, &mode, "Color mode", []uikit.SelectOption{
			{Value: "system", Label: "System"}, {Value: "light", Label: "Light"}, {Value: "dark", Label: "Dark"},
		}, n.mutating).Changed() {
			n.saveSetting("app.color_mode", mode)
		}
		for _, setting := range []struct{ key, label string }{
			{"app.compact_mode", "Compact history"},
			{"app.arrow_key_selection", "Select history with arrow keys"},
			{"app.preview_keyboard_enabled", "Preview keyboard selection"},
		} {
			enabled := n.preferences[setting.key] == "true"
			if ui.Checkbox(c, &enabled, setting.label).Disabled(n.mutating).Changed() {
				n.saveSetting(setting.key, strconv.FormatBool(enabled))
			}
		}
	})
}
