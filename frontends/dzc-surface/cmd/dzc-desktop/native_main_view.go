package main

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

func (n *nativeMain) view(c *ui.Context) {
	kit := presets.Select(n.theme, n.mode).Kit()
	kit.Apply(c)
	n.list.List.Selected = &n.history.selected
	n.list.List.Key = func(index int) any { return n.history.rows[index].Id }
	n.list.List.Label = func(index int) string { return n.history.rows[index].Preview }
	ui.Column(c).Fill().Gap(6).Background(c.Theme().Background).Children(func() {
		queryComposing := false
		actions := []uikit.HeaderAction{
			{Icon: "pin", Label: "Keep on top", Active: n.pinned, Run: n.togglePin, Disabled: n.mutating},
			{Icon: "hide", Label: "Hide", Run: func() { n.shell.hideMain(n.window) }},
		}
		header := uikit.HeaderOptions{Title: "DezirClip", Actions: actions}
		if n.showSettings {
			header.Title = "Settings"
			header.Back = &uikit.HeaderAction{Icon: "back", Label: "Back", Run: func() { n.showSettings = false }}
		} else {
			header.Actions = append(actions[:1:1], uikit.HeaderAction{
				Icon: "settings", Label: "Settings", Run: func() { n.showSettings = true },
			}, actions[1])
			header.Query, header.Placeholder, header.Filter = &n.query, "Search clipboard…", &n.kind
			header.Filters = []uikit.HeaderFilter{
				{ID: "text", Label: "Text"}, {ID: "image", Label: "Images"}, {ID: "files", Label: "Files"},
			}
			header.FocusQuery = &n.focusSearch
			header.OnQueryEdit = func(composing bool) { queryComposing = composing }
		}
		kit.Header(c, header)
		if n.showSettings {
			n.settingsView(c, kit)
		} else {
			n.editQuery(queryComposing)
			n.historyView(c, kit)
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
}

func (n *nativeMain) historyView(c *ui.Context, kit uikit.Kit) {
	if n.private {
		ui.Text(c, "Private session").TextColor(c.Theme().TextMuted).Padding(4, 8)
	}
	generation := n.history.generation
	list := kit.HistoryList(c, &n.list, uikit.HistoryListOptions{
		Count: len(n.history.rows), Label: "Clipboard history", EmptyLabel: "No clipboard entries",
		LoadingLabel: "Loading…", MoreLabel: "Load more…", Loading: n.reading, HasMore: n.history.more && !n.failed,
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
			kit.HistoryCard(cell.Context, uikit.HistoryCardOptions{
				Source: row.SourceApp, Time: time.UnixMilli(row.Timestamp).Format("15:04"), Summary: row.Preview,
				Pinned: row.IsPinned, Selected: cell.Selected(),
				Activate: func() { n.rowAction(row, generation, "copy") },
				Actions: []uikit.HeaderAction{
					{Icon: "pin", Label: pinLabel, Disabled: n.mutating, Run: func() { n.rowAction(row, generation, "pin") }},
					{Icon: "hide", Label: "Delete entry", Disabled: n.mutating, Run: func() { n.rowAction(row, generation, "delete") }},
				},
			})
		},
	})
	if list.Submitted() && n.history.selected >= 0 && n.history.selected < len(n.history.rows) {
		n.rowAction(n.history.rows[n.history.selected], generation, "copy")
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
	})
}
