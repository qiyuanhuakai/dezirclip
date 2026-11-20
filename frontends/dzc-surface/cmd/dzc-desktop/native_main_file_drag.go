package main

import (
	"context"
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type nativeFileGesture struct {
	id     int64
	start  ui.Point
	cancel context.CancelFunc
	armed  bool
	spent  bool
}

func (g *nativeFileGesture) takeClick(id int64) bool {
	if id == g.id && g.spent {
		g.spent = false
		return true
	}
	return false
}

func nativeFileCard(row protocol.DesktopEntry) bool {
	switch row.ContentType {
	case "image", "file", "files", "video":
		return row.Id != 0
	default:
		return false
	}
}

func (n *nativeMain) fileCardInput(event ui.InputEvent, row protocol.DesktopEntry, generation uint64) bool {
	if event.Kind == ui.InputScroll {
		return n.historyWheel(event)
	}
	if event.Button != 0 {
		return false
	}
	g := &n.fileGesture
	switch event.Kind {
	case ui.InputPointerDown:
		if g.cancel != nil {
			g.cancel()
		}
		*g = nativeFileGesture{id: row.Id, start: ui.Point{X: event.X, Y: event.Y}, armed: true}
	case ui.InputPointerMove:
		if g.id != row.Id || !g.armed {
			return false
		}
		dx, dy := event.X-g.start.X, event.Y-g.start.Y
		if dx*dx+dy*dy >= 36 {
			g.armed, g.spent = false, true
			n.dragHistoryFiles(row, generation)
			return true
		}
	case ui.InputPointerUp:
		if g.id == row.Id {
			g.armed = false
			if g.cancel != nil {
				g.cancel()
			}
		}
	}
	// The card keeps its ordinary mouse and keyboard activation.
	return false
}

func (n *nativeMain) dragHistoryFiles(row protocol.DesktopEntry, generation uint64) {
	state := n.history.state
	reveal := nativeConcealed(row)
	if n.session == nil || state.ServiceInstanceId == "" || n.mutating || n.failed ||
		n.history.generation != generation || reveal && !n.canUseProtectedRow(row) {
		return
	}
	n.mutating, n.notice = true, ""
	n.hover.reset()
	n.closePreview()
	session, window := n.session, n.window
	drag, cancel := context.WithCancel(session)
	n.fileGesture.cancel = cancel
	n.shell.dock.dialogs.Add(1)
	go func() {
		defer cancel()
		err := n.library.dragFiles(drag, window, protocol.FileDragParams{
			Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: reveal,
		})
		n.shell.dock.dialogs.Add(-1)
		n.update(session, func() {
			n.mutating = false
			n.fileGesture.cancel = nil
			if err != nil && !errors.Is(err, context.Canceled) {
				n.notice = n.text("native_file_drag_failed")
			}
		})
	}()
}
