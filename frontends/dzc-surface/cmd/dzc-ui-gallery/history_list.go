package main

import (
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

func (g *gallery) renderHistoryList(c *ui.Context, kit uikit.Kit) {
	rows := []struct{ Source, Time, Summary string }{
		{"Editor", "12:30", "Copied from the active editor"},
		{"Browser", "12:28", "Release checklist and verification notes"},
		{"Terminal", "12:25", "Preview package ready for testing"},
	}
	g.historyList.List.Selected = &g.historySelected
	g.historyList.List.Label = func(index int) string { return rows[index].Summary }
	ui.Column(c).FillWidth().Height(320).Children(func() {
		kit.HistoryList(c, &g.historyList, uikit.HistoryListOptions{
			Count: len(rows), Label: "Clipboard history",
			Row: func(row ui.ListRow) {
				item := rows[row.Index]
				kit.HistoryCard(row.Context, uikit.HistoryCardOptions{
					Source: item.Source, Time: item.Time, Summary: item.Summary, Selected: row.Selected(),
					Activate: func() { g.historySelected = row.Index },
					Actions:  []uikit.HeaderAction{{Icon: "pin", Label: "Pin"}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}},
				})
			},
		})
	})
}
