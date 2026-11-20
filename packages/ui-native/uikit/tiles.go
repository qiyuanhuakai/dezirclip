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
	title, group, grid := k.style("emojiGroupTitle"), k.style("emojiGroup"), k.style("emojiGrid")
	gap := grid.ColumnGap
	columns := characterColumnCount(width, gap)
	ui.Column(c).FillWidth().Gap(group.RowGap).Children(func() {
		paint(text(c, label, title).FillWidth(), title)
		tileRows(c, len(entries), columns, width, gap, func(index int, cellWidth float32) {
			k.characterTile(c, entries[index], cellWidth, disabled, onSelect)
		})
	})
}

func characterColumnCount(width, gap float32) int {
	return max(1, int((width+gap)/(34+gap)))
}

func (k Kit) characterTile(c *ui.Context, entry CharacterChoice, width float32, disabled bool, onSelect func(string)) {
	tile := k.style("emojiTile")
	button := ui.ButtonBase(c).Label(entry.Value).Size(width, tile.Height).Center().FocusRing(true).Disabled(disabled)
	if entry.Name != "" {
		button.Description(entry.Name).Tooltip(entry.Name)
	}
	style := k.interactive(c, button, tile, false)
	paint(button, style).Children(func() { text(c, entry.Value, style).SingleLine().PassThrough() })
	if button.Clicked() && onSelect != nil {
		onSelect(entry.Value)
	}
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

func (k Kit) EmojiAddTile(c *ui.Context, label string, width float32, disabled bool) ui.Element {
	style := k.style("emojiAddTile")
	button := ui.ButtonBase(c).Label(label).Tooltip(label).Size(width, width).Disabled(disabled).Center()
	if button.Hovered() || button.FocusVisible() {
		style = k.style("emojiAddTileHover")
	}
	paint(button, k.animatePaint(button, style)).Children(func() {
		ui.Column(c).Gap(6).AlignItems(ui.Center).Children(func() {
			ui.Icon(c, headerIcons["add"]).Size(18, 18).TextColor(styleColor(style.Text)).PassThrough()
			text(c, label, style).TextAlign(ui.Center).Wrap().PassThrough()
		})
	})
	return button
}

type EmojiFavoriteOptions struct {
	Name, RemoveLabel string
	Width             float32
	Disabled          bool
	Preview           func()
	OnChoose          func()
	OnRemove          func()
}

func (k Kit) EmojiFavoriteTile(c *ui.Context, options EmojiFavoriteOptions) {
	tile := ui.Box(c).Size(options.Width, options.Width).PassThrough()
	removeVisible := ui.Local(tile, "favorite-tile-remove", func() bool { return false })
	tile.Children(func() {
		style := k.style("emojiFavoriteTile")
		image := ui.ButtonBase(c).Fill().Center().Label(options.Name).Tooltip(options.Name).Disabled(options.Disabled)
		paint(image, style).Children(options.Preview)
		if image.Clicked() && options.OnChoose != nil {
			options.OnChoose()
		}
		if image.RightClicked() || image.Shortcut(ui.Shift, ui.KeyF10) {
			*removeVisible = !*removeVisible
		}
		hovered := tile.Hovered()
		if options.OnRemove == nil || !(*removeVisible || hovered) {
			return
		}
		remove := k.style("emojiFavoriteRemove")
		button := ui.ButtonBase(c).Absolute().Right(-6).Top(-6).Size(20, 20).Center().
			Label(options.RemoveLabel).Tooltip(options.RemoveLabel).Disabled(options.Disabled)
		paint(button, k.interactive(c, button, remove, false)).Children(func() {
			ui.Icon(c, closeIcon).Size(12, 12).TextColor(ui.RGB(255, 255, 255)).PassThrough()
		})
		if button.Clicked() {
			*removeVisible = false
			options.OnRemove()
		}
	})
}
