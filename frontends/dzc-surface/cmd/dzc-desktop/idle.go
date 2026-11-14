package main

import (
	"context"
	"errors"
	"strconv"
	"time"

	"github.com/egoist/mygo"
)

// Only an explicit hide starts an idle period. Docking and native dialogs can hide
// the WebView temporarily and need the same window to remain available.
type hiddenWebView struct {
	generation uint64
	timer      *time.Timer
}

func (idle *hiddenWebView) acceptWake(dispatch func(func())) uint64 {
	accepted := make(chan struct{})
	var generation uint64
	dispatch(func() { idle.cancel(); generation = idle.generation; close(accepted) })
	<-accepted
	return generation
}

func (idle *hiddenWebView) cancel() {
	idle.generation++
	if idle.timer != nil {
		idle.timer.Stop()
		idle.timer = nil
	}
}

func (s *DesktopShell) HideMain(ctx context.Context) error {
	w := mygo.CallerWindow(ctx)
	if w == nil {
		return errors.New("WindowUnavailable")
	}
	mygo.RunOnMain(func() { s.hideMain(w) })
	return nil
}

func (s *DesktopShell) hideMain(w *mygo.Window) {
	s.resumePending.Store(false)
	s.resumeSequence.Add(1)
	if w.IsDestroyed() {
		return
	}
	s.dock.hidden = false
	s.dock.history.targetInvalid.Store(true)
	s.idle.cancel()
	generation := s.idle.generation
	started := time.Now()
	w.Hide()
	go func() {
		settings := (&Library{s.dock.history.actions}).windowPreferences(context.Background())
		if settings.IdleDestroy != "true" {
			return
		}
		seconds, err := strconv.Atoi(settings.IdleSeconds)
		if err != nil || seconds < 5 || seconds > 3600 {
			seconds = 60
		}
		mygo.RunOnMain(func() {
			if generation != s.idle.generation || w.IsDestroyed() || w.IsVisible() {
				return
			}
			s.idle.timer = time.AfterFunc(time.Until(started.Add(time.Duration(seconds)*time.Second)), func() {
				mygo.RunOnMain(func() {
					if generation == s.idle.generation && !w.IsDestroyed() && !w.IsVisible() {
						s.diagnostics.record("idle-destroy", 0, nil)
						w.Close()
					}
				})
			})
		})
	}()
}
