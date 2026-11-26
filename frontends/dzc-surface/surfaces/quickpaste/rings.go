package quickpaste

import (
	"slices"
	"strconv"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/model"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

var ringCategories = []string{"text", "image", "file", "url", "code", "video", "rich_text"}

func (p *Panel) placeDesktopRing() {
	cursor := mygo.Screen.CursorScreenPoint()
	display := mygo.Screen.DisplayNearestPoint(cursor)
	area := display.WorkArea
	if area.Width < 1 || area.Height < 1 {
		p.window.SetContentSize(440, 440)
		p.local.placeAtCursor(p.window)
		return
	}
	bounds, dock := ringBounds(cursor, area)
	p.ringDock = dock
	p.window.SetContentBounds(bounds)
}

func ringBounds(cursor mygo.Point, area mygo.Rectangle) (mygo.Rectangle, string) {
	width, height := min(440, area.Width), min(440, area.Height)
	dock := ""
	left, right, top := cursor.X-area.X, area.X+area.Width-cursor.X, cursor.Y-area.Y
	if nearest := min(left, right, top); nearest <= 48 {
		switch nearest {
		case top:
			dock, height = "top", min(224, height)
		case left:
			dock, width = "left", min(224, width)
		default:
			dock, width = "right", min(224, width)
		}
	}
	x := min(max(cursor.X-width/2, area.X), area.X+area.Width-width)
	y := min(max(cursor.Y-height/2, area.Y), area.Y+area.Height-height)
	return mygo.Rectangle{X: x, Y: y, Width: width, Height: height}, dock
}

func ringCategory(kind string) string {
	switch kind {
	case "files":
		return "file"
	case "link":
		return "url"
	default:
		return kind
	}
}

func (p *Panel) ringRows(rows *model.View) []int {
	kind := p.ringKind
	limit := p.ringSlots
	indices := make([]int, 0, limit)
	for i := 0; i < rows.Len(&p.model) && len(indices) < limit; i++ {
		row, current := rows.Row(&p.model, i)
		if current && ringCategory(row.Kind) == kind {
			indices = append(indices, i)
		}
	}
	return indices
}

func (p *Panel) moveRing(rows *model.View, delta int) {
	indices := p.ringRows(rows)
	if len(indices) > 0 {
		index := slices.Index(indices, p.model.Selected)
		p.model.Selected = indices[min(len(indices)-1, max(0, index+delta))]
	}
}

func (p *Panel) chooseRingKind(kind string) {
	if p.ringKind != kind {
		p.ringKind = kind
		p.model.Selected = -1
	}
}

func (p *Panel) compactRingLabel(kind string) string {
	if p.locale == "en" {
		switch kind {
		case "image":
			return "Img"
		case "video":
			return "Vid"
		case "rich_text":
			return "Rich"
		}
	}
	if kind == "rich_text" {
		return "富文"
	}
	return p.word("kind_" + kind)
}

func ringSlots(value *int64, dock string) int {
	fallback, limit := 8, 12
	if dock != "" {
		fallback, limit = 5, 8
	}
	if value != nil && *value >= 3 && *value <= int64(limit) {
		return int(*value)
	}
	return fallback
}

func (p *Panel) drawRings(c *ui.Context, kit uikit.Kit, tint ui.Color, rows *model.View, pointerMoved bool) {
	if p.ringKind == "" {
		for i := range rows.Len(&p.model) {
			row, ok := rows.Row(&p.model, i)
			if kind := ringCategory(row.Kind); ok && slices.Contains(ringCategories, kind) {
				p.ringKind = kind
				break
			}
		}
	}
	activeKind := p.ringKind
	if activeKind == "" {
		activeKind = ringCategories[0]
	}
	indices := p.ringRows(rows)
	if !slices.Contains(indices, p.model.Selected) {
		p.model.Selected = -1
		if len(indices) != 0 {
			p.model.Selected = indices[0]
		}
	}
	categories := make([]uikit.QuickPasteRingItem, 0, len(ringCategories))
	for _, kind := range ringCategories {
		categories = append(categories, uikit.QuickPasteRingItem{
			Key: kind, Label: p.word("kind_" + kind), CompactLabel: p.compactRingLabel(kind), Active: activeKind == kind,
			Select: func() {
				p.chooseRingKind(kind)
				c.Invalidate()
			},
		})
	}
	entries := make([]uikit.QuickPasteRingItem, 0, len(indices))
	clicked := -1
	for _, index := range indices {
		row, _ := rows.Row(&p.model, index)
		entries = append(entries, uikit.QuickPasteRingItem{
			Key: strconv.FormatInt(row.ID, 10), Label: row.Text, Detail: row.SourceApp, Active: p.model.Selected == index,
			Select: func() {
				if p.model.CanSelect() {
					clicked = index
				}
			},
			Hover: func() {
				if p.model.CanSelect() && pointerMoved && p.model.Selected != index {
					p.model.Selected = index
					c.Invalidate()
				}
			},
		})
	}
	kit.QuickPasteRings(c, p.ringDock, p.ringSlots, tint, categories, entries)
	if clicked >= 0 {
		p.model.Selected = clicked
		p.paste(rows)
	}
}

func (p *Panel) bindRingKeys(c *ui.Context, display string, rows *model.View) {
	for key, action := range map[ui.Key]func(){
		ui.KeyEscape: func() { p.dismiss("cancel") },
		ui.KeyEnter:  func() { p.paste(rows) },
		ui.KeyDown:   func() { p.moveRing(rows, 1) },
		ui.KeyUp:     func() { p.moveRing(rows, -1) },
		ui.KeyLeft:   func() { p.shiftRingKind(-1) },
		ui.KeyRight:  func() { p.shiftRingKind(1) },
	} {
		c.OnShortcut(0, key, func() {
			if p.model.Display == display {
				action()
				c.Invalidate()
			}
		})
	}
}

func (p *Panel) shiftRingKind(delta int) {
	count := len(ringCategories)
	index := max(0, slices.Index(ringCategories, p.ringKind))
	p.chooseRingKind(ringCategories[(index+delta+count)%count])
}
