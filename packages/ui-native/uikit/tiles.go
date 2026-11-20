package uikit

import "github.com/egoist/mygo/ui"

type CharacterTab struct {
	ID, Label string
}

type CharacterChoice struct {
	Value string `json:"value"`
	Name  string `json:"name"`
}

func (k Kit) EmojiTabs(c *ui.Context, emojiLabel, favoritesLabel string, favorites, disabled bool, selectFavorites func(bool)) {
	selected := "emoji"
	if favorites {
		selected = "favorites"
	}
	k.CharacterTabs(c, []CharacterTab{{"emoji", emojiLabel}, {"favorites", favoritesLabel}}, selected, disabled,
		func(tab string) { selectFavorites(tab == "favorites") })
}

// CharacterTabs draws the emoji, symbol and favorite tabs with their shared theme.
func (k Kit) CharacterTabs(c *ui.Context, tabs []CharacterTab, active string, disabled bool, onSelect func(string)) {
	style := k.style("emojiTabs")
	paint(ui.Row(c).FillWidth().Gap(style.ColumnGap), style).Children(func() {
		for _, tab := range tabs {
			if k.emojiTab(c, tab.Label, tab.ID == active, disabled).Clicked() && tab.ID != active {
				onSelect(tab.ID)
			}
		}
	})
}

func (k Kit) emojiTab(c *ui.Context, label string, selected, disabled bool) ui.Element {
	style := k.style("emojiTab")
	if selected {
		style = k.style("emojiTabActive")
	}
	button := ui.ButtonBase(c).Label(label).Height(style.Height).Grow(1).Center().FocusRing(true).Disabled(disabled)
	style = k.interactive(c, button, style, selected)
	paint(button, style).Children(func() { text(c, label, style).PassThrough() })
	return button
}

func (k Kit) EmojiGrid(c *ui.Context, label string, entries []string, disabled bool, selectEmoji func(string)) {
	width, _ := c.Size()
	choices := make([]CharacterChoice, len(entries))
	for i, value := range entries {
		choices[i].Value = value
	}
	k.CharacterGrid(c, label, choices, width-16, disabled, selectEmoji)
}

// CharacterContent applies the padding and vertical spacing shared by picker groups.
func (k Kit) CharacterContent(c *ui.Context, width float32, draw func(float32)) {
	style := k.style("emojiContent")
	ui.Column(c).FillWidth().Padding(style.Padding[:]...).Gap(style.RowGap).Children(func() {
		draw(max(1, width-style.Padding[1]-style.Padding[3]))
	})
}

// CharacterGrid fits tiles to the supplied content width and describes each glyph.
func (k Kit) CharacterGrid(c *ui.Context, label string, entries []CharacterChoice, width float32, disabled bool, onSelect func(string)) {
	title, group, grid, tile := k.style("emojiGroupTitle"), k.style("emojiGroup"), k.style("emojiGrid"), k.style("emojiTile")
	gap := grid.ColumnGap
	columns := max(1, int((width+gap)/(34+gap)))
	ui.Column(c).FillWidth().Gap(group.RowGap).Children(func() {
		paint(text(c, label, title).FillWidth(), title)
		tileRows(c, len(entries), columns, width, gap, func(index int, cellWidth float32) {
			entry := entries[index]
			button := ui.ButtonBase(c).Label(entry.Value).Size(cellWidth, tile.Height).Center().FocusRing(true).Disabled(disabled)
			if entry.Name != "" {
				button.Description(entry.Name).Tooltip(entry.Name)
			}
			style := k.interactive(c, button, tile, false)
			paint(button, style).Children(func() { text(c, entry.Value, style).SingleLine().PassThrough() })
			if button.Clicked() && onSelect != nil {
				onSelect(entry.Value)
			}
		})
	})
}

// TileGrid keeps empty slots at the end of the last row and sizes every row
// from the viewport instead of measuring track widths from its contents.
func (k Kit) TileGrid(c *ui.Context, count, columns int, gap float32, draw func(int, float32)) {
	width, _ := c.Size()
	tileRows(c, count, columns, width-16, gap, draw)
}

func tileRows(c *ui.Context, count, columns int, width, gap float32, draw func(int, float32)) {
	columns = max(1, columns)
	width = max(1, (width-float32(columns-1)*gap)/float32(columns))
	ui.Column(c).FillWidth().Gap(gap).Children(func() {
		for start := 0; start < count; start += columns {
			ui.Row(c).FillWidth().Gap(gap).Children(func() {
				for index := start; index < min(count, start+columns); index++ {
					draw(index, width)
				}
			})
		}
	})
}

func (k Kit) TileButton(c *ui.Context, label string, width, height float32, disabled bool) ui.Element {
	style := k.style("headerButton")
	style.Padding, style.Shadows = [4]float32{}, nil
	style.Fill, style.Border = [4]uint8{}, [4]uint8{}
	style.BorderWidth, style.BorderWidths, style.Outline = 0, nil, nil
	tile := ui.ButtonBase(c).Label(label).Size(width, height).FocusRing(true).Disabled(disabled)
	return paint(tile, k.interactive(c, tile, style, false))
}
