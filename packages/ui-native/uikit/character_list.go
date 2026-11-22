package uikit

import (
	"slices"

	"github.com/egoist/mygo/ui"
)

type CharacterGroup struct {
	Label   string
	Choices []CharacterChoice
}

type characterListRow struct {
	title   string
	choices []CharacterChoice
	top     float32
	cell    float32
}

// CharacterList renders visible glyph rows across all groups in one scrollable
// viewport. Reset state when replacing the results of a search.
func (k Kit) CharacterList(c *ui.Context, state *ui.ListState, width float32, groups []CharacterGroup, label, empty string, disabled bool, onSelect func(string)) {
	content := k.style("emojiContent")
	title, group, grid := k.style("emojiGroupTitle"), k.style("emojiGroup"), k.style("emojiGrid")
	width = max(1, width-content.Padding[1]-content.Padding[3])
	gap := grid.ColumnGap
	var rows []characterListRow
	for _, section := range groups {
		if len(section.Choices) == 0 {
			continue
		}
		top := float32(0)
		if len(rows) > 0 {
			top = content.RowGap
		}
		rows = append(rows, characterListRow{title: section.Label, top: top})
		columns := characterChoiceColumns(width, gap, section.Choices)
		cell := (width - float32(columns-1)*gap) / float32(columns)
		for choices := range slices.Chunk(section.Choices, columns) {
			top := grid.ColumnGap
			if len(rows[len(rows)-1].choices) == 0 {
				top = 0
			}
			rows = append(rows, characterListRow{choices: choices, top: top, cell: cell})
		}
	}
	if len(rows) == 0 {
		k.SettingsLabel(c, empty).AlignSelf(ui.Center)
		return
	}
	ui.List(c, state, len(rows)).Grow(1).MinHeight(0).FillWidth().Padding(content.Padding[:]...).Label(label).
		Rows(func(row ui.ListRow) {
			ctx, item := row.Context, rows[row.Index]
			if len(item.choices) == 0 {
				ui.Column(ctx).FillWidth().Padding(item.top, 0, group.RowGap).Children(func() {
					paint(text(ctx, item.title, title).FillWidth(), title)
				})
				return
			}
			ui.Row(ctx).FillWidth().Padding(item.top, 0, 0).Gap(gap).Children(func() {
				for _, entry := range item.choices {
					k.characterTile(ctx, entry, item.cell, disabled, onSelect)
				}
			})
		})
}
