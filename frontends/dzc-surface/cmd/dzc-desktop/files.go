package main

import (
	"context"
	"errors"
	"sync/atomic"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

var fileDragPending atomic.Bool

func (l *Library) DropFiles(ctx context.Context, params protocol.FileDropParams) error {
	if mygo.CallerWindow(ctx) == nil {
		return errors.New("WindowUnavailable")
	}
	return l.dropFiles(ctx, params)
}

func (l *Library) dropFiles(ctx context.Context, params protocol.FileDropParams) error {
	if fileDragPending.Load() {
		return nil
	}
	var result protocol.OperationResult
	if err := l.owner.DesktopCall(ctx, protocol.MethodDesktopFilesDrop, params, "OperationResult", &result); err != nil {
		return err
	}
	if !result.Performed {
		return errors.New("FileDropUnconfirmed")
	}
	return nil
}

func (l *Library) DragFiles(ctx context.Context, params protocol.FileDragParams) error {
	return l.dragFiles(ctx, mygo.CallerWindow(ctx), params)
}

func (l *Library) dragFiles(ctx context.Context, window *mygo.Window, params protocol.FileDragParams) error {
	if window == nil {
		return errors.New("WindowUnavailable")
	}
	if !fileDragPending.CompareAndSwap(false, true) {
		return errors.New("FileDragBusy")
	}
	if !window.IsVisible() {
		fileDragPending.Store(false)
		return errors.New("WindowUnavailable")
	}
	var files protocol.FileDragResult
	if err := l.owner.DesktopCall(ctx, protocol.MethodDesktopFilesDrag, params, "FileDragResult", &files); err != nil {
		fileDragPending.Store(false)
		return err
	}
	done := make(chan error, 1)
	// Keep self-drops blocked until the system finishes the gesture, even
	// when the initiating renderer context is cancelled meanwhile.
	window.Update(func() {
		finish := func(err error) {
			fileDragPending.Store(false)
			done <- err
		}
		if err := ctx.Err(); err != nil {
			finish(err)
			return
		}
		if window.IsDestroyed() || !window.IsVisible() {
			finish(errors.New("WindowUnavailable"))
			return
		}
		startFileDrag(window, files.Paths, finish)
	})
	select {
	case err := <-done:
		return err
	case <-ctx.Done():
		return ctx.Err()
	}
}
