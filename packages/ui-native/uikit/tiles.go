package uikit

import (
	"slices"

	"github.com/egoist/mygo/ui"
)

type CharacterTab struct {
	ID, Label string
}

type CharacterChoice struct {
	Value   string            `json:"value"`
	Name    string            `json:"name"`
	Preview *CharacterPreview `json:"-"`
}

// CharacterPreview changes the tile artwork while Value remains its text action.
type CharacterPreview struct {
	Bitmap func() *ui.Bitmap
	Font   string
	Code   string
}

// CharacterPanel reserves the same outer padding and section gaps as the Web picker.
func (k Kit) CharacterPanel(c *ui.Context, viewportWidth float32, draw func(contentWidth float32)) {
	inset, bottom := float32(12), float32(12)
	if viewportWidth <= 520 {
		inset, bottom = 8, 8
	}
	ui.Column(c).FillWidth().Grow(1).MinHeight(0).Padding(8, inset, bottom).Gap(8).Children(func() {
		draw(max(1, viewportWidth-2*inset))
	})
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
	button := ui.ButtonBase(c).Label(label).Height(style.Height).Grow(1).Center().FocusRing(true).
		Cursor(actionCursor(disabled)).Selected(selected).Disabled(disabled)
	style = k.animatePaint(button, style)
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
	columns := characterChoiceColumns(width, gap, entries)
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

func characterChoiceColumns(width, gap float32, entries []CharacterChoice) int {
	if slices.ContainsFunc(entries, func(entry CharacterChoice) bool {
		return entry.Preview != nil && entry.Preview.Code != ""
	}) {
		return max(1, int((width+gap)/(88+gap)))
	}
	return characterColumnCount(width, gap)
}

func (k Kit) characterTile(c *ui.Context, entry CharacterChoice, width float32, disabled bool, onSelect func(string)) {
	tile := k.style("emojiTile")
	button := ui.ButtonBase(c).Label(entry.Value).Size(width, tile.Height).Center().FocusRing(true).
		Cursor(actionCursor(disabled)).Disabled(disabled)
	description := entry.Name
	preview := entry.Preview
	if preview != nil && preview.Code != "" {
		description += " · " + preview.Code
	}
	if description != "" {
		button.Description(description).Tooltip(description)
	}
	style := k.buttonInteraction(button, "emojiTile", tile)
	k.paintTranslated(button, style).Children(func() {
		if preview != nil {
			if preview.Bitmap != nil {
				if bitmap := preview.Bitmap(); bitmap != nil {
					ui.Image(c, bitmap).Size(24, 24).PassThrough()
					return
				}
			}
			if preview.Code != "" {
				style.Size = 9
				ui.Column(c).FillWidth().Padding(0, 2).Gap(1).Center().PassThrough().Children(func() {
					text(c, preview.Code, style).FillWidth().SingleLine().TextAlign(ui.Center).PassThrough()
					text(c, entry.Name, style).FillWidth().SingleLine().TextAlign(ui.Center).PassThrough()
				})
				return
			}
			if preview.Font != "" {
				style.Font = preview.Font
				style.Weight = 400
			}
		}
		text(c, entry.Value, style).SingleLine().PassThrough()
	})
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
	tile := ui.ButtonBase(c).Label(label).Size(width, height).FocusRing(true).Disabled(disabled).Cursor(actionCursor(disabled))
	return paint(tile, k.interactive(c, tile, style, false))
}

func (k Kit) EmojiAddTile(c *ui.Context, label string, width float32, disabled bool) ui.Element {
	var button ui.Element
	k.emojiFavoriteFrame(c, width, func(_ ui.Element, cell float32) {
		style := k.style("emojiAddTile")
		button = ui.ButtonBase(c).Label(label).Tooltip(label).Size(cell, cell).Disabled(disabled).Center().Cursor(actionCursor(disabled))
		if !disabled && (button.Hovered() || button.FocusVisible()) {
			style = k.style("emojiAddTileHover")
		}
		paint(button, k.animatePaint(button, style)).Children(func() {
			ui.Column(c).Gap(style.RowGap).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, headerIcons["add"]).Size(18, 18).TextColor(styleColor(style.Text)).PassThrough()
				text(c, label, style).TextAlign(ui.Center).Wrap().PassThrough()
			})
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
	k.emojiFavoriteFrame(c, options.Width, func(tile ui.Element, cell float32) {
		removeVisible := ui.Local(tile, "favorite-tile-remove", func() bool { return false })
		style := k.style("emojiFavoriteTile")
		image := ui.ButtonBase(c).Size(cell, cell).Center().Label(options.Name).Tooltip(options.Name).
			Disabled(options.Disabled).Cursor(actionCursor(options.Disabled))
		paint(image, style).Children(options.Preview)
		if image.Clicked() && options.OnChoose != nil {
			options.OnChoose()
		}
		if image.RightClicked() || image.Shortcut(ui.Shift, ui.KeyF10) {
			*removeVisible = !*removeVisible
		}
		hovered := tile.Hovered() || tile.FocusWithin()
		if options.OnRemove == nil || !(*removeVisible || hovered) {
			return
		}
		remove := k.style("emojiFavoriteRemove")
		button := ui.ButtonBase(c).Absolute().Right(-6).Top(-6).Size(20, 20).Center().
			Label(options.RemoveLabel).Tooltip(options.RemoveLabel).Disabled(options.Disabled).Cursor(actionCursor(options.Disabled))
		paint(button, k.interactive(c, button, remove, false)).Children(func() {
			ui.Icon(c, closeIcon).Size(12, 12).TextColor(ui.RGB(255, 255, 255)).PassThrough()
		})
		if button.Clicked() {
			*removeVisible = false
			options.OnRemove()
		}
	})
}
