package screenshot

import (
	"context"
	"errors"
	"log"
	"math"
	"runtime/debug"
	"slices"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type Selector struct {
	record        func(string) bool
	window        *mygo.Window
	stopping      bool
	owner         *client.Client
	session       *selection
	pendingRegion bool
	hotkeys       map[bool]string
}

// A selection owns one private IPC connection and one frozen monitor image.
type selection struct {
	ready      func(error)
	full       bool
	host       *client.Client
	state      protocol.RuntimeState
	frame      protocol.ScreenImage
	bitmap     *ui.Bitmap
	cancel     context.CancelFunc
	start, end ui.Point
	dragging   bool
	started    time.Time
}

func (s *Selector) SetShortcutRecorder(record func(string) bool) { s.record = record }

func (s *Selector) HasShortcut() bool { return len(s.hotkeys) != 0 }

func New(owner *client.Client) *Selector {
	s := &Selector{owner: owner, hotkeys: make(map[bool]string)}
	var caps protocol.CapabilityReport
	if err := owner.DesktopCall(context.Background(), protocol.MethodCapabilities, protocol.EmptyParams{}, "CapabilityReport", &caps); err != nil ||
		!slices.Contains(caps.Methods, "desktop.screen.finish") {
		return s
	}
	s.prepareWindow()
	s.Refresh()
	mygo.App.OnBeforeQuit(func(*mygo.QuitEvent) {
		s.stopping = true
		for _, key := range s.hotkeys {
			mygo.GlobalShortcut.Unregister(key)
		}
		s.clear()
	})
	return s
}

func (s *Selector) prepareWindow() {
	s.window = mygo.NewWindow(mygo.WindowOptions{Title: "DezirClip 截图", Hidden: true, Frameless: true, Width: 1, Height: 1,
		Content: ui.View(func(c *ui.Context) {
			if s.session != nil {
				s.draw(c, s.session)
			}
		})})
	s.window.OnClose(func(e *mygo.CloseEvent) {
		if !s.stopping {
			e.PreventDefault()
			s.clear()
		}
	})
	s.window.SetAlwaysOnTop(true)
}

func (s *Selector) preferences() (protocol.ScreenPreferences, error) {
	var pref protocol.ScreenPreferences
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	err := s.owner.DesktopCall(ctx, protocol.MethodDesktopScreenPreferences, protocol.EmptyParams{}, "ScreenPreferences", &pref)
	return pref, err
}

func (s *Selector) Refresh() error {
	if s.window == nil || s.stopping {
		return errors.New("ScreenshotUnavailable")
	}
	pref, err := s.preferences()
	if err != nil {
		return err
	}
	if !pref.Enabled {
		s.clear()
		s.window.Destroy()
	} else if s.window.IsDestroyed() {
		s.prepareWindow()
	}
	var registrationErrors error
	for full, key := range map[bool]string{false: pref.Region, true: pref.Full} {
		if !pref.Enabled {
			key = ""
		}
		if s.hotkeys[full] == key {
			continue
		}
		if key != "" {
			if err := mygo.GlobalShortcut.Register(key, func() {
				if s.record == nil || !s.record(key) {
					s.begin(full)
				}
			}); err != nil {
				log.Printf("Screenshot shortcut unavailable: %v", err)
				registrationErrors = errors.Join(registrationErrors, err)
				continue
			}
		}
		if old := s.hotkeys[full]; old != "" {
			mygo.GlobalShortcut.Unregister(old)
		}
		if key == "" {
			delete(s.hotkeys, full)
		} else {
			s.hotkeys[full] = key
		}
	}
	return registrationErrors
}

func (s *Selector) begin(full bool) {
	if s.session != nil {
		if s.session.full && !full {
			s.pendingRegion = true
		}
		return
	}
	s.start(full, nil, nil)
}

// Capture acknowledges a settings action when its selector is visible or its full frame has been copied.
func (s *Selector) Capture(full bool, epoch uint64, ready func(error)) {
	if s.window == nil || s.stopping || s.session != nil {
		ready(errors.New("ScreenshotUnavailable"))
		return
	}
	s.start(full, &epoch, ready)
}

func (shot *selection) notify(err error) {
	if shot.ready != nil {
		shot.ready(err)
		shot.ready = nil
	}
}

func (s *Selector) start(full bool, epoch *uint64, ready func(error)) {
	target, ok := cursorTarget()
	if !ok {
		if ready != nil {
			ready(errors.New("ScreenTargetUnavailable"))
		}
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 115*time.Second)
	shot := &selection{cancel: cancel, full: full, ready: ready, started: time.Now()}
	s.session = shot
	go func() {
		pref, err := s.preferences()
		if err != nil || !pref.Enabled {
			s.end(shot, errors.New("ScreenshotUnavailable"))
			return
		}
		host, err := s.owner.DesktopConnection(ctx)
		if err != nil {
			s.end(shot, err)
			return
		}
		state := host.Hello().State
		if epoch != nil && state.PrivacyEpoch != *epoch {
			host.Close()
			s.end(shot, errors.New("PrivacyEpochChanged"))
			return
		}
		var frame protocol.ScreenImage
		request := protocol.ScreenBeginParams{ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Cursor: &target.point}
		if err := host.Call(ctx, protocol.MethodDesktopScreenBegin, request, "ScreenImage", &frame); err != nil {
			host.Close()
			s.end(shot, err)
			return
		}
		if !target.matches(frame) {
			host.Close()
			s.end(shot, client.ErrProtocol)
			return
		}
		if full {
			err := finish(ctx, host, state, frame)
			host.Close()
			s.end(shot, err)
			return
		}
		raw, err := host.ReadScreenPNG(ctx, state, frame)
		var bitmap *ui.Bitmap
		if err == nil {
			bitmap, err = ui.DecodeBitmap(raw)
		}
		clear(raw)
		if err != nil {
			host.Close()
			s.end(shot, err)
			return
		}
		var current protocol.RuntimeState
		err = host.Call(ctx, protocol.MethodHealth, protocol.EmptyParams{}, "RuntimeState", &current)
		if err != nil || current.PrivacyEpoch != state.PrivacyEpoch || current.ServiceInstanceId != state.ServiceInstanceId {
			host.Close()
			s.end(shot, client.ErrProtocol)
			return
		}
		mygo.RunOnMain(func() {
			if s.session != shot || ctx.Err() != nil {
				host.Close()
				if s.session == shot {
					s.clear()
				}
				return
			}
			shot.host, shot.state, shot.frame, shot.bitmap = host, state, frame, bitmap
			if !target.place(s.window, frame) {
				s.clear()
				return
			}
			s.window.Show()
			s.window.Focus()
			s.window.Invalidate()
			shot.notify(nil)
			log.Printf("Screenshot selector ready in %d ms", time.Since(shot.started).Milliseconds())
			go s.watch(ctx, shot, state)
		})
	}()
}

func (s *Selector) watch(ctx context.Context, shot *selection, state protocol.RuntimeState) {
	events, err := s.owner.DesktopConnection(ctx)
	if err == nil {
		defer events.Close()
		cursor := state.EventSequence
		for ctx.Err() == nil {
			var update protocol.EventsResult
			wait := uint16(1000)
			err = events.Call(ctx, protocol.MethodEventsSubscribe, protocol.SubscribeParams{AfterSequence: cursor, WaitMs: &wait}, "EventsResult", &update)
			if err != nil || update.State.PrivacyEpoch != state.PrivacyEpoch || update.State.ServiceInstanceId != state.ServiceInstanceId {
				break
			}
			cursor = update.State.EventSequence
		}
	}
	s.end(shot, err)
}

func (s *Selector) draw(c *ui.Context, shot *selection) {
	if s.session != shot || shot.bitmap == nil {
		return
	}
	w, h := c.Size()
	if w <= 0 || h <= 0 {
		return
	}
	pane := ui.Column(c).Fill().Cursor(ui.CursorCrosshair)
	pane.Draw(func(p *ui.Painter, r ui.Rect) {
		p.Image(shot.bitmap, r, ui.FillBox)
		if shot.dragging {
			box := ui.Rect{X: min(shot.start.X, shot.end.X), Y: min(shot.start.Y, shot.end.Y),
				W: max(shot.start.X, shot.end.X) - min(shot.start.X, shot.end.X), H: max(shot.start.Y, shot.end.Y) - min(shot.start.Y, shot.end.Y)}
			shade := ui.RGBA(0, 0, 0, .28)
			p.Fill(ui.Rect{W: w, H: box.Y}, shade, 0)
			p.Fill(ui.Rect{Y: box.Y + box.H, W: w, H: h - box.Y - box.H}, shade, 0)
			p.Fill(ui.Rect{Y: box.Y, W: box.X, H: box.H}, shade, 0)
			p.Fill(ui.Rect{X: box.X + box.W, Y: box.Y, W: w - box.X - box.W, H: box.H}, shade, 0)
			p.Stroke(box, ui.Hex("#ffffff"), 0, 1)
			phase := float32(time.Since(shot.started).Milliseconds()%800) / 100
			for x := -phase; x < box.W; x += 8 {
				if x+4 <= 0 {
					continue
				}
				p.Line(box.X+max(0, x), box.Y, box.X+min(box.W, x+4), box.Y, 1, ui.Hex("#202124"))
				p.Line(box.X+max(0, x), box.Y+box.H, box.X+min(box.W, x+4), box.Y+box.H, 1, ui.Hex("#202124"))
			}
			for y := -phase; y < box.H; y += 8 {
				if y+4 <= 0 {
					continue
				}
				p.Line(box.X, box.Y+max(0, y), box.X, box.Y+min(box.H, y+4), 1, ui.Hex("#202124"))
				p.Line(box.X+box.W, box.Y+max(0, y), box.X+box.W, box.Y+min(box.H, y+4), 1, ui.Hex("#202124"))
			}
			p.AnimationFrame()
		}
	})
	c.OnShortcut(0, ui.KeyEscape, s.clear)
	pane.HandleInput(func(ev ui.InputEvent) bool {
		if s.session != shot || shot.bitmap == nil {
			return false
		}
		pos := ui.Point{X: min(w, max(0, ev.X)), Y: min(h, max(0, ev.Y))}
		switch ev.Kind {
		case ui.InputPointerDown:
			if ev.Button != 0 {
				return false
			}
			shot.start, shot.end, shot.dragging = pos, pos, true
		case ui.InputPointerMove:
			if !shot.dragging {
				return false
			}
			shot.end = pos
		case ui.InputPointerUp:
			if ev.Button != 0 || !shot.dragging {
				return false
			}
			shot.end, shot.dragging = pos, false
			s.crop(shot, w, h)
		default:
			return false
		}
		return true
	})
}

func (s *Selector) crop(shot *selection, w, h float32) {
	frame := shot.frame
	px := func(point, span float32, size uint32) int64 {
		return int64(math.Round(float64(point) / float64(span) * float64(size)))
	}
	x1, x2 := px(min(shot.start.X, shot.end.X), w, frame.Width), px(max(shot.start.X, shot.end.X), w, frame.Width)
	y1, y2 := px(min(shot.start.Y, shot.end.Y), h, frame.Height), px(max(shot.start.Y, shot.end.Y), h, frame.Height)
	log.Printf("Screenshot selection: %dx%d pixels, viewport %.0fx%.0f", x2-x1, y2-y1, w, h)
	if x1 == x2 || y1 == y2 {
		return
	}
	host, state := shot.host, shot.state
	shot.bitmap = nil
	s.window.Hide()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		params := protocol.ScreenCropParams{ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
			Token: frame.Token, X: frame.X + x1, Y: frame.Y + y1, Width: uint32(x2 - x1), Height: uint32(y2 - y1)}
		var cropped protocol.ScreenImage
		err := host.Call(ctx, protocol.MethodDesktopScreenCrop, params, "ScreenImage", &cropped)
		if err == nil {
			err = finish(ctx, host, state, cropped)
		}
		s.end(shot, err)
	}()
}

func finish(ctx context.Context, host *client.Client, state protocol.RuntimeState, frame protocol.ScreenImage) error {
	request := protocol.ScreenSessionParams{ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Token: frame.Token}
	var result protocol.ScreenFinishResult
	err := host.Call(ctx, protocol.MethodDesktopScreenFinish, request, "ScreenFinishResult", &result)
	if err == nil && !result.Copied {
		return client.ErrProtocol
	}
	return err
}

func (s *Selector) end(shot *selection, err error) {
	mygo.RunOnMain(func() {
		if s.session == shot {
			if err != nil {
				log.Printf("Screenshot ended: %v", err)
			}
			shot.notify(err)
			next := s.pendingRegion && err == nil && !s.stopping
			s.clear()
			if next {
				s.begin(false)
			}
		}
	})
}

func (s *Selector) clear() {
	s.pendingRegion = false
	shot := s.session
	if shot == nil {
		return
	}
	s.session = nil
	shot.notify(context.Canceled)
	shot.cancel()
	if shot.host != nil {
		shot.host.Close()
	}
	shot.bitmap = nil
	if !s.stopping && shot.host != nil {
		// Destroy the renderer that saw the frozen frame. A hidden renderer
		// may retain its last scene and its full-size GPU or software surface.
		s.window.Destroy()
		s.prepareWindow()
		go debug.FreeOSMemory()
	}
}
