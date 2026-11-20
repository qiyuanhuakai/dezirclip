package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

func (k Kit) EmojiTabs(c *ui.Context, emojiLabel, favoritesLabel string, favorites, disabled bool, selectFavorites func(bool)) {
	ui.Row(c).FillWidth().Gap(6).Children(func() {
		if k.emojiTab(c, emojiLabel, !favorites, disabled).Clicked() && favorites {
			selectFavorites(false)
		}
		if k.emojiTab(c, favoritesLabel, favorites, disabled).Clicked() && !favorites {
			selectFavorites(true)
		}
	})
}

func (k Kit) emojiTab(c *ui.Context, label string, selected, disabled bool) ui.Element {
	style := k.style("confirmCancel")
	if selected {
		style.Fill, style.Border = rgba(c.Theme().Accent), rgba(c.Theme().Accent)
		style.Text = rgba(c.Theme().AccentText)
	}
	style.Size, style.Weight, style.LetterSpacing = 11, 800, .8
	style.Padding = [4]float32{}
	button := ui.ButtonBase(c).Label(label).Height(30).Grow(1).Center().FocusRing(true).Disabled(disabled)
	style = k.interactive(c, button, style, selected)
	paint(button, style).Children(func() { text(c, strings.ToUpper(label), style).PassThrough() })
	return button
}

func (k Kit) EmojiGrid(c *ui.Context, label string, entries []string, disabled bool, selectEmoji func(string)) {
	title := k.style("headerTitle")
	title.Size, title.Weight, title.LetterSpacing = 10, 800, .8
	width, _ := c.Size()
	const gap = 6
	columns := max(1, int((width-16+gap)/(34+gap)))
	ui.Column(c).FillWidth().Gap(gap).Children(func() {
		text(c, strings.ToUpper(label), title).FillWidth().Padding(4).
			Border(1, c.Theme().Border).BorderWidth(0, 0, 1, 0)
		k.TileGrid(c, len(entries), columns, gap, func(index int, cellWidth float32) {
			emoji := entries[index]
			button := k.TileButton(c, emoji, cellWidth, 32, disabled).Center()
			button.Children(func() { ui.Text(c, emoji).FontSize(18).SingleLine().PassThrough() })
			if button.Clicked() && selectEmoji != nil {
				selectEmoji(emoji)
			}
		})
	})
}

// TileGrid keeps empty slots at the end of the last row and sizes every row
// from the viewport instead of measuring track widths from its contents.
func (k Kit) TileGrid(c *ui.Context, count, columns int, gap float32, draw func(int, float32)) {
	width, _ := c.Size()
	width = max(1, (width-16-float32(columns-1)*gap)/float32(columns))
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
