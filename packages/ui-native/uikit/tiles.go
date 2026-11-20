package uikit

import "github.com/egoist/mygo/ui"

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
