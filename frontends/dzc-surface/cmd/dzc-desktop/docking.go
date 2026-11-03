package main

import (
	"context"
	"errors"
	"strconv"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo"
)

var dockPinned = mygo.NewEvent[bool]("edge-pinned")

type dockSample struct {
	window, monitor mygo.Rectangle
	cursor          mygo.Point
	pressed         bool
}

// The activation strip is limited to the part of this monitor occupied by the window.
func (s dockSample) strip() mygo.Rectangle {
	w, m := s.window, s.monitor
	left, top := max(w.X, m.X), max(w.Y, m.Y)
	right, bottom := min(w.X+w.Width, m.X+m.Width), min(w.Y+w.Height, m.Y+m.Height)
	if right <= left || bottom <= top {
		return mygo.Rectangle{}
	}
	const thickness = 5
	switch {
	case w.Y <= m.Y+thickness:
		return mygo.Rectangle{X: left, Y: m.Y, Width: right - left, Height: thickness}
	case w.X <= m.X+thickness:
		return mygo.Rectangle{X: m.X, Y: top, Width: thickness, Height: bottom - top}
	case w.X+w.Width >= m.X+m.Width-thickness:
		return mygo.Rectangle{X: m.X + m.Width - thickness, Y: top, Width: thickness, Height: bottom - top}
	}
	return mygo.Rectangle{}
}

func containsPoint(r mygo.Rectangle, p mygo.Point) bool {
	return p.X >= r.X && p.Y >= r.Y && p.X < r.X+r.Width && p.Y < r.Y+r.Height
}

// The main WebView owns its docking monitor. The native resident never polls the cursor.
type edgeDock struct {
	dialogs        atomic.Int32
	window         *mygo.Window
	preview        *Preview
	history        *History
	stop           chan struct{}
	hidden         bool
	strip, monitor mygo.Rectangle
	shown          time.Time
}

func (d *edgeDock) configure(enabled bool) {
	if !enabled {
		if d.stop != nil {
			close(d.stop)
			d.stop = nil
		}
		if d.hidden && !d.window.IsDestroyed() {
			d.reveal()
		}
		return
	}
	if d.stop != nil || !dockingAvailable() {
		return
	}
	d.shown = time.Now()
	stop := make(chan struct{})
	d.stop = stop
	go func() {
		timer := time.NewTicker(150 * time.Millisecond)
		defer timer.Stop()
		for {
			select {
			case <-stop:
				return
			case <-timer.C:
				mygo.RunOnMain(func() {
					if d.stop == stop && !d.window.IsDestroyed() {
						d.tick()
					}
				})
			}
		}
	}()
}

func (d *edgeDock) reveal() {
	d.hidden = false
	d.history.rememberTarget(context.Background())
	d.window.Show()
	d.window.Focus()
	d.shown = time.Now()
}

func (d *edgeDock) tick() {
	w := d.window
	if d.dialogs.Load() != 0 {
		return
	}
	if !d.hidden && (!w.IsVisible() || w.IsMinimized() || w.IsMaximized() || w.IsFullScreen() ||
		d.preview.window != nil || time.Since(d.shown) < 800*time.Millisecond) {
		return
	}
	sample, ok := readDockSample(w)
	if !ok || sample.pressed {
		return
	}
	if d.hidden {
		if sample.monitor != d.monitor || containsPoint(d.strip, sample.cursor) {
			d.reveal()
		}
		return
	}
	strip := sample.strip()
	if strip.Width == 0 || containsPoint(sample.window, sample.cursor) {
		return
	}
	if !w.IsAlwaysOnTop() {
		w.SetAlwaysOnTop(true)
		dockPinned.Emit(w, true)
	}
	d.strip, d.monitor, d.hidden = strip, sample.monitor, true
	w.Hide()
}

func (s *DesktopShell) SetEdgeDocking(ctx context.Context, enabled bool, epoch uint64) error {
	w := mygo.CallerWindow(ctx)
	if w == nil || (enabled && !dockingAvailable()) {
		return errors.New("EdgeDockingUnavailable")
	}
	if err := (&Library{s.dock.history.actions}).SetSetting(ctx, "app.edge_docking", strconv.FormatBool(enabled), epoch); err != nil {
		return err
	}
	mygo.RunOnMain(func() {
		if !w.IsDestroyed() {
			s.dock.configure(enabled)
		}
	})
	return nil
}
