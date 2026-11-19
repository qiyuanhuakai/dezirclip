package main

import (
	"context"
	"encoding/json"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

// All state below belongs to the main event thread. Reads stay on worker
// goroutines and carry a presentation context and a snapshot ticket.
type nativeMain struct {
	window  *mygo.Window
	shell   *DesktopShell
	library *Library
	actions *History

	session      context.Context
	stop         context.CancelFunc
	stopRead     context.CancelFunc
	resume       uint64
	history      nativeHistory
	list         uikit.HistoryListState
	preferences  map[string]string
	query, kind  string
	theme, mode  string
	focusSearch  bool
	showSettings bool
	reading      bool
	private      bool
	pinned       bool
	failed       bool
	mutating     bool
}

func (n *nativeMain) stopSession() {
	if n.stop != nil {
		n.stop()
		n.stop = nil
	}
	n.session = nil
	n.stopRead = nil
	n.history.reset(protocol.RuntimeState{}, "", "")
	n.reading, n.failed, n.mutating, n.private = false, false, false, false
}

func (n *nativeMain) prepare(request uint64) {
	n.stopSession()
	session, cancel := context.WithCancel(context.Background())
	n.session, n.stop, n.resume = session, cancel, request
	n.reading = true
	go n.open(session)
}

func (n *nativeMain) open(session context.Context) {
	for session.Err() == nil {
		read, cancel := context.WithTimeout(session, 4*time.Second)
		status, err := n.library.Status(read)
		if err == nil {
			raw, _ := n.library.Preferences(read)
			settings := make(map[string]string)
			_ = json.Unmarshal(raw, &settings)
			mode := n.colorMode(read, settings)
			cancel()
			n.update(session, func() {
				n.configured(settings, mode)
				n.private = status.Privacy.Active
				n.history.reset(status.State, n.query, n.kind)
				n.list.Reset()
				n.loadPage()
				go n.watch(session, status.State.EventSequence)
			})
			return
		}
		cancel()
		// A resident can wake the process while the Host is still opening
		// its profile. Keep the pending show request until the first page.
		timer := time.NewTimer(220 * time.Millisecond)
		select {
		case <-timer.C:
		case <-session.Done():
			timer.Stop()
			return
		}
	}
}

func (n *nativeMain) colorMode(ctx context.Context, settings map[string]string) string {
	mode := settings["app.color_mode"]
	if mode != "light" && mode != "dark" {
		mode = n.shell.SystemThemeMode(ctx)
	}
	if mode != "dark" {
		mode = "light"
	}
	return mode
}

func (n *nativeMain) configured(settings map[string]string, mode string) {
	n.preferences, n.theme, n.mode = settings, settings["app.theme"], mode
	n.pinned = settings["app.window_pinned"] == "true"
	n.window.SetAlwaysOnTop(n.pinned)
}

func (n *nativeMain) update(session context.Context, fn func()) {
	n.window.Update(func() {
		if !n.window.IsDestroyed() && session.Err() == nil && n.session == session {
			fn()
		}
	})
}

func (n *nativeMain) loadPage() {
	if n.session == nil || n.history.state.ServiceInstanceId == "" {
		return
	}
	if n.stopRead != nil {
		n.stopRead()
	}
	request := n.history.request()
	session := n.session
	read, cancel := context.WithTimeout(session, 4*time.Second)
	n.stopRead, n.reading, n.failed = cancel, true, false
	go func() {
		defer cancel()
		page, err := n.library.nativeHistoryPage(read, request)
		n.update(session, func() {
			if !n.history.accepts(request) {
				return
			}
			n.reading = false
			if err != nil || !n.history.apply(request, page) {
				n.failed = true
				if !n.window.IsVisible() && n.resume != 0 {
					n.retryPending(session, request)
				}
				return
			}
			if n.resume != 0 {
				go n.reveal(session, n.resume, page.state)
			}
		})
	}()
}

func (n *nativeMain) retryPending(session context.Context, request nativeHistoryRequest) {
	time.AfterFunc(220*time.Millisecond, func() {
		n.update(session, func() {
			if n.resume != 0 && n.history.accepts(request) && n.failed {
				n.loadPage()
			}
		})
	})
}

func (n *nativeMain) reveal(session context.Context, request uint64, state protocol.RuntimeState) {
	ctx, cancel := context.WithTimeout(session, 4*time.Second)
	defer cancel()
	ok, _ := n.shell.completeResume(ctx, n.window, request, state.ServiceInstanceId, state.PrivacyEpoch)
	n.update(session, func() {
		if ok && n.resume == request {
			n.resume = 0
		}
	})
}

func (n *nativeMain) watch(session context.Context, cursor uint64) {
	for session.Err() == nil {
		read, cancel := context.WithTimeout(session, 5*time.Second)
		events, err := n.actions.Changes(read, &cursor)
		cancel()
		if err != nil {
			n.update(session, func() { n.prepare(n.resume) })
			return
		}
		cursor = events.State.EventSequence
		if len(events.Events) > 0 || events.ResyncRequired {
			n.update(session, func() { n.applyEvents(events) })
		}
	}
}

func (n *nativeMain) applyEvents(events protocol.EventsResult) {
	current, next := n.history.state, events.State
	if current.ServiceInstanceId != next.ServiceInstanceId || current.PrivacyEpoch != next.PrivacyEpoch || events.ResyncRequired {
		n.prepare(n.resume)
		return
	}
	for _, event := range events.Events {
		if event.Event == protocol.EventKindServiceStateChanged && event.Data.Status != nil &&
			(*event.Data.Status == "view_settings_changed" || *event.Data.Status == "application_settings_reset") {
			n.prepare(n.resume)
			return
		}
	}
	if next.HistoryRevision > current.HistoryRevision {
		n.history.reset(next, n.query, n.kind)
		n.loadPage()
	}
}

func (n *nativeMain) editQuery(composing bool) {
	query := strings.TrimSpace(n.query)
	if n.session != nil && n.history.state.ServiceInstanceId != "" && !composing &&
		(query != n.history.query || n.kind != n.history.kind) {
		n.history.reset(n.history.state, query, n.kind)
		n.list.Reset()
		n.loadPage()
	}
}

func (n *nativeMain) rowAction(row protocol.DesktopEntry, generation uint64, action string) {
	if n.session == nil || n.history.generation != generation || n.mutating || n.failed {
		return
	}
	n.mutating = true
	session, state := n.session, n.history.state
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		var err error
		if action == "copy" {
			_, err = n.actions.CopyEntry(ctx, protocol.DesktopCopyParams{
				Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
			})
		} else {
			params := protocol.DesktopMutationParams{
				Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Action: action,
			}
			if action == "pin" {
				pinned := !row.IsPinned
				params.Pinned = &pinned
			}
			_, err = n.library.MutateEntry(ctx, params)
		}
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.failed = true
			}
		})
	}()
}

func (n *nativeMain) saveSetting(key, value string) {
	n.writeSetting(func(ctx context.Context, epoch uint64) error {
		return n.library.SetSetting(ctx, key, value, epoch)
	})
}

func (n *nativeMain) togglePin() {
	pinned := !n.pinned
	n.writeSetting(func(ctx context.Context, epoch uint64) error {
		return n.library.SetWindowPinned(ctx, pinned, epoch)
	})
}

func (n *nativeMain) writeSetting(write func(context.Context, uint64) error) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating = true
	session, state := n.session, n.history.state
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		err := write(ctx, state.PrivacyEpoch)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.failed = true
				return
			}
			n.prepare(n.resume)
		})
	}()
}
