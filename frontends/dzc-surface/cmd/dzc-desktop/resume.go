package main

import (
	"context"
	"time"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

var prepareShow = mygo.NewEvent[uint64]("main-window-prepare-show")

// The request survives listener installation and is retired only after a fresh
// history page has committed against the same Host and privacy epoch.
func (s *DesktopShell) requestShow(w *mygo.Window) {
	s.idle.cancel()
	s.resumePending.Store(true)
	s.resumeStarted = time.Now()
	request := s.resumeSequence.Add(1)
	prepareShow.Emit(w, request)
}

func (s *DesktopShell) ResumeRequest() *uint64 {
	if !s.resumePending.Load() {
		return nil
	}
	request := s.resumeSequence.Load()
	return &request
}

func (s *DesktopShell) CompleteResume(ctx context.Context, request uint64, instance string, epoch uint64) (bool, error) {
	w := mygo.CallerWindow(ctx)
	if w == nil || !s.resumePending.Load() || request != s.resumeSequence.Load() {
		return false, nil
	}
	var state protocol.RuntimeState
	if err := s.dock.history.actions.DesktopCall(ctx, protocol.MethodHealth, struct{}{}, "RuntimeState", &state); err != nil {
		return false, err
	}
	if state.ServiceInstanceId != instance || state.PrivacyEpoch != epoch {
		return false, nil
	}
	done := make(chan bool, 1)
	mygo.RunOnMain(func() {
		if ctx.Err() != nil || w.IsDestroyed() || !s.resumePending.Load() || request != s.resumeSequence.Load() {
			done <- false
			return
		}
		s.resumePending.Store(false)
		if w.IsMinimized() {
			w.Restore()
		}
		restoreMainPosition(w)
		s.dock.hidden = false
		s.dock.shown = time.Now()
		w.Show()
		w.Focus()
		desktopTiming("React preparation and show", s.resumeStarted)
		done <- true
	})
	select {
	case accepted := <-done:
		return accepted, nil
	case <-ctx.Done():
		return false, ctx.Err()
	}
}
