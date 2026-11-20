package main

import (
	"context"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativePinSource struct {
	id         int64
	generation uint64
}

func (n *nativeMain) canReorderVisiblePins() bool {
	sort := n.history.filter.sort
	if n.session == nil || sort != "" && sort != "pinned" {
		return false
	}
	count := 0
	for _, row := range n.history.rows {
		if !row.IsPinned {
			break
		}
		if row.Id > 0 {
			count++
			if count == 2 {
				return true
			}
		}
	}
	return false
}

func (n *nativeMain) pinReorderGrip(row protocol.DesktopEntry, index int, generation uint64, enabled bool, onDrag func()) *uikit.HistoryReorder {
	if !enabled || row.Id <= 0 || !row.IsPinned {
		return nil
	}
	source := nativePinSource{row.Id, generation}
	focus := n.pinFocus.id == row.Id && n.pinFocus.generation != generation && !n.mutating
	if focus {
		n.pinFocus = nativePinSource{}
	}
	return &uikit.HistoryReorder{
		Label: n.text("native_pin_reorder"),
		Value: source, Disabled: n.mutating || n.failed, Focus: focus,
		OnDrag: onDrag,
		OnMove: func(earlier bool) {
			step := 1
			if earlier {
				step = -1
			}
			for next := index + step; next >= 0 && next < len(n.history.rows); next += step {
				anchor := n.history.rows[next]
				if !anchor.IsPinned {
					break
				}
				if anchor.Id > 0 {
					n.movePinnedRow(source, anchor.Id, earlier, true)
					break
				}
			}
		},
	}
}

func (n *nativeMain) pinDropTarget(c *ui.Context, card ui.Element, row protocol.DesktopEntry, generation uint64) bool {
	_, y, _ := card.PointerPosition()
	before := y < card.Bounds().H/2
	valid := func(source nativePinSource) bool {
		return !n.mutating && !n.failed && source.id != row.Id && source.generation == generation
	}
	source, over := ui.DragOver[nativePinSource](card)
	if over && valid(source) {
		accent := c.Theme().Accent
		card.DrawOver(func(p *ui.Painter, r ui.Rect) {
			top := r.Y
			if !before {
				top += r.H - 2
			}
			p.Fill(ui.Rect{X: r.X + 5, Y: top, W: max(0, r.W-10), H: 2}, accent, 1)
		})
	}
	if source, dropped := ui.Drop[nativePinSource](card); dropped && valid(source) {
		n.movePinnedRow(source, row.Id, before, false)
	}
	return over
}

func (n *nativeMain) movePinnedRow(source nativePinSource, anchor int64, before, keyboard bool) {
	if n.session == nil || n.history.state.ServiceInstanceId == "" || n.mutating || n.failed ||
		source.generation != n.history.generation || source.id == anchor {
		return
	}
	if keyboard {
		n.pinFocus = source
	}
	n.mutating, n.notice = true, ""
	n.hover.reset()
	n.closePreview()
	session, state := n.session, n.history.state
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		err := n.library.movePin(ctx, state, protocol.LibraryPinMove{Id: source.id, AnchorId: anchor, Before: before})
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.pinFocus = nativePinSource{}
				n.notice = n.text("native_pin_reorder_failed")
			}
		})
	}()
}
