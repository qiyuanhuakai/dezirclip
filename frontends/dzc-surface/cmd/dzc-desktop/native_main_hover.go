package main

import (
	"context"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type nativeHoverPreview struct {
	row     int64
	bounds  ui.Rect
	timer   *time.Timer
	ticket  uint64
	preview uint64
}

func (h *nativeHoverPreview) reset() {
	if h.timer != nil {
		h.timer.Stop()
	}
	*h = nativeHoverPreview{ticket: h.ticket + 1}
}

func (h *nativeHoverPreview) owns(p *Preview) bool {
	return p != nil && p.entryID != 0 && h.preview == p.generation
}

func (n *nativeMain) hoverPreview(row protocol.DesktopEntry, generation uint64, bounds ui.Rect) {
	if n.session == nil || n.window == nil || n.preview == nil || n.keyboard || n.failed || n.mutating || n.hasHistoryDialog() {
		return
	}
	h := &n.hover
	if h.row == row.Id && h.bounds == bounds {
		return
	}
	active := h.owns(n.preview)
	if active && row.Id != 0 && n.preview.entryID != row.Id {
		n.closePreview()
		active = false
	}
	previous := h.preview
	h.reset()
	h.row, h.bounds = row.Id, bounds
	if active {
		h.preview = previous
	}
	if row.Id == 0 {
		if active {
			n.waitForHoverExit(n.session, h.ticket)
		}
		return
	}
	if bounds.W <= 0 || bounds.H <= 0 || n.preview.entryID != 0 {
		return
	}
	session, ticket := n.session, h.ticket
	h.timer = time.AfterFunc(300*time.Millisecond, func() {
		n.update(session, func() {
			if h.ticket != ticket || n.mutating || n.preview.entryID != 0 ||
				!mainWindowFocused(n.window) || n.history.generation != generation {
				return
			}
			n.showPreview(row, generation, bounds)
			if n.preview.entryID == row.Id {
				h.preview = n.preview.generation
			}
		})
	})
}

func (n *nativeMain) waitForHoverExit(session context.Context, ticket uint64) {
	h := &n.hover
	h.timer = time.AfterFunc(180*time.Millisecond, func() {
		n.update(session, func() {
			if h.ticket != ticket || !h.owns(n.preview) {
				return
			}
			if w := n.preview.window; w != nil {
				bounds, pointer, ok := previewPointer(w)
				if ok && pointer.X >= bounds.X && pointer.X < bounds.X+bounds.Width &&
					pointer.Y >= bounds.Y && pointer.Y < bounds.Y+bounds.Height {
					n.waitForHoverExit(session, ticket)
					return
				}
			}
			n.closePreview()
			h.reset()
		})
	})
}
