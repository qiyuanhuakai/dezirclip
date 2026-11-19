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
	if !fileDragPending.CompareAndSwap(false, true) {
		return errors.New("FileDragBusy")
	}
	defer fileDragPending.Store(false)
	window := mygo.CallerWindow(ctx)
	if window == nil || !window.IsVisible() {
		return errors.New("WindowUnavailable")
	}
	var files protocol.FileDragResult
	if err := l.owner.DesktopCall(ctx, protocol.MethodDesktopFilesDrag, params, "FileDragResult", &files); err != nil {
		return err
	}
	done := make(chan error, 1)
	window.Update(func() { startFileDrag(window, files.Paths, func(err error) { done <- err }) })
	select {
	case err := <-done:
		return err
	case <-ctx.Done():
		return ctx.Err()
	}
}
