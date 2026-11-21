package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

type EmojiFavoritesOptions struct {
	ContentWidth                              float32
	Count                                     int
	Label, AddLabel, EmptyTitle, Hint, Status string
	DropLabel                                 string
	Disabled, Dragging                        bool
	OnAdd                                     func()
	OnDrop                                    func([]string)
	Item                                      func(c *ui.Context, index int, width float32)
}

// EmojiFavorites draws a scrollable favorite gallery with an add tile and local file drop feedback.
func (k Kit) EmojiFavorites(c *ui.Context, list *ui.ListState, options EmojiFavoritesOptions) {
	content, grid := k.style("emojiContent"), k.style("emojiFavoriteGrid")
	cellWidth := float32(74)
	gap := grid.ColumnGap
	if window, _ := c.Size(); window <= 520 {
		cellWidth, gap = 66, 6
	}
	available := options.ContentWidth - content.Padding[1] - content.Padding[3]
	cellWidth = min(cellWidth, max(1, available))
	total := options.Count + 1
	columns := min(total, max(1, int((available+gap)/(cellWidth+gap))))
	rows := (total + columns - 1) / columns
	zone := ui.Box(c).FillWidth().Grow(1).MinHeight(0).Clip()
	dragging := options.Dragging
	if options.OnDrop != nil && !options.Disabled {
		dragging = zone.FileDragOver() || dragging
		if paths := zone.DroppedFiles(); len(paths) > 0 {
			options.OnDrop(paths)
		}
	}
	rowCount := rows
	if !dragging {
		rowCount++
	}
	zone.Children(func() {
		ui.List(c, list, rowCount).Fill().Gap(gap).Padding(content.Padding[:]...).Label(options.Label).Rows(func(row ui.ListRow) {
			if row.Index == rows {
				margin := max(0, content.RowGap-gap) + grid.Padding[2]
				k.emojiFavoritesMessage(row.Context, options, margin)
				return
			}
			start := row.Index * columns
			end := min(total, start+columns)
			line := ui.Row(row.Context).FillWidth().Gap(gap).Justify(ui.Center)
			if row.Index == 0 {
				line.Margin(grid.Padding[0], 0, 0)
			}
			line.Children(func() {
				for index := start; index < end; index++ {
					if index == 0 {
						if k.EmojiAddTile(row.Context, options.AddLabel, cellWidth, options.Disabled).Clicked() && options.OnAdd != nil {
							options.OnAdd()
						}
					} else {
						options.Item(row.Context, index-1, cellWidth)
					}
				}
				if unused := columns - (end - start); unused > 0 {
					ui.Box(row.Context).Width(float32(unused)*(cellWidth+gap) - gap).PassThrough()
				}
			})
		})
		if dragging {
			style := k.style("emojiFavoriteDrop")
			box := ui.Box(c).Absolute().Left(4).Right(4).Top(4).Bottom(4).Center().PassThrough()
			paint(box, style).Children(func() {
				text(c, options.DropLabel, style).TextAlign(ui.Center).Wrap().PassThrough()
			})
		}
	})
}

func (k Kit) emojiFavoritesMessage(c *ui.Context, options EmojiFavoritesOptions, margin float32) {
	if options.Status == "" && options.Count == 0 && options.EmptyTitle != "" {
		panel, title, description := k.style("emojiFavoriteEmpty"), k.style("emojiFavoriteEmptyTitle"), k.style("emojiFavoriteEmptyHint")
		paint(ui.Column(c).FillWidth().Margin(margin, 0, 0).AlignItems(ui.Center).Gap(panel.RowGap), panel).Children(func() {
			paint(text(c, options.EmptyTitle, title).TextAlign(ui.Center).Wrap(), title)
			paint(text(c, options.Hint, description).MaxWidth(description.Width).TextAlign(ui.Center).Wrap(), description)
		})
		return
	}
	hint := options.Hint
	if options.Status != "" {
		hint = options.Status
	}
	style := k.style("emojiFavoriteTip")
	paint(text(c, hint, style).FillWidth().Margin(margin+style.MarginTop, 0, 0).TextAlign(ui.Center).Wrap(), style)
}

func (k Kit) emojiFavoriteFrame(c *ui.Context, width float32, draw func(ui.Element, float32)) {
	style := k.style("emojiFavoriteFrame")
	cell := max(1, width-style.Padding[1]-style.Padding[3]-2*style.BorderWidth)
	height := cell + style.Padding[0] + style.Padding[2] + 2*style.BorderWidth
	frame := ui.Box(c).Size(width, height)
	shift := float32(0)
	if frame.Hovered() || frame.FocusWithin() {
		style.Border = k.style("emojiFavoriteTile").Border
		shift = -2
	}
	frame.Translate(0, frame.Animate("favorite-lift", shift, 100*time.Millisecond))
	paint(frame, k.animatePaint(frame, style)).Children(func() { draw(frame, cell) })
}
