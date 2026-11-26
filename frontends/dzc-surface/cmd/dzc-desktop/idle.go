package main

import (
	"context"
	"errors"
	"strconv"
	"time"

	"github.com/egoist/mygo"
)

const (
	minIdleSeconds     = 5
	maxIdleSeconds     = 3600
	defaultIdleSeconds = 60
)

// Only an explicit hide starts an idle period. Docking and native dialogs can
// hide the main window temporarily and need it to remain available.
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

func idleDestroyDelay(enabled, value string) time.Duration {
	if enabled != "true" {
		return 0
	}
	seconds, err := strconv.Atoi(value)
	if err != nil || seconds < minIdleSeconds || seconds > maxIdleSeconds {
		seconds = defaultIdleSeconds
	}
	return time.Duration(seconds) * time.Second
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
	var delay time.Duration
	var known bool
	if s.idleCloseDelay != nil {
		// Hiding ends the visible session, including any in-flight settings refresh.
		delay, known = s.idleCloseDelay()
	}
	w.Hide()
	if s.refreshMemory != nil {
		s.refreshMemory()
	}
	s.dock.syncPolling()
	schedule := func(delay time.Duration) {
		if delay > 0 && generation == s.idle.generation && !w.IsDestroyed() && !w.IsVisible() {
			s.scheduleIdleClose(w, generation, time.Until(started.Add(delay)))
		}
	}
	if known {
		schedule(delay)
		return
	}
	go func() {
		settings := (&Library{s.dock.history.actions}).windowPreferences(context.Background())
		if delay := idleDestroyDelay(settings.IdleDestroy, settings.IdleSeconds); delay > 0 {
			mygo.RunOnMain(func() { schedule(delay) })
		}
	}()
}

func (s *DesktopShell) scheduleIdleClose(w *mygo.Window, generation uint64, delay time.Duration) {
	s.idle.timer = time.AfterFunc(delay, func() {
		mygo.RunOnMain(func() {
			if generation != s.idle.generation || w.IsDestroyed() || w.IsVisible() {
				return
			}
			if s.keepMainAlive != nil && s.keepMainAlive() {
				s.scheduleIdleClose(w, generation, time.Second)
				return
			}
			s.diagnostics.record("idle-destroy", 0, nil)
			w.Close()
		})
	})
}
