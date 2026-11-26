package quickpaste

import (
	"context"
	"errors"
	"log"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/theme"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

// The resident Desktop already has ordinary GUI metadata authority. Enrich
// only IDs admitted by the restricted Surface page; never reveal a hidden row.
func enrichDesktopPage(ctx context.Context, host *client.Client, request protocol.SurfacePageParams, page *protocol.SurfacePage) error {
	var snapshot protocol.BodySnapshot
	params := protocol.DesktopPageParams{Limit: request.Limit, Offset: request.Offset, Query: request.Query}
	if err := host.Call(ctx, protocol.MethodDesktopHistoryPage, params, "BodySnapshot", &snapshot); err != nil {
		return err
	}
	if snapshot.State.ServiceInstanceId != request.ServiceInstanceId || snapshot.State.PrivacyEpoch != request.ExpectedEpoch || snapshot.State.HistoryRevision != request.ExpectedHistoryRevision {
		return client.ErrProtocol
	}
	var metadata protocol.DesktopEntries
	if err := host.ReadSnapshot(ctx, snapshot, "DesktopEntries", &metadata); err != nil {
		return err
	}
	byID := make(map[int64]string)
	for _, entry := range metadata.Entries {
		if entry.ContentType != "rich_text" && entry.ContentType != "url" && entry.ContentType != "link" {
			continue
		}
		text := strings.TrimSpace(entry.Preview)
		lower := strings.ToLower(text)
		if strings.HasPrefix(lower, "data:") || strings.HasPrefix(lower, "file:") || strings.HasPrefix(lower, "asset:") || strings.ContainsAny(text, "<>") {
			continue
		}
		byID[entry.Id] = text
	}
	for i := range page.Entries {
		row := &page.Entries[i]
		if row.PrivacyVisibility == "ordinary" && row.Preview == nil {
			if text := byID[row.Id]; text != "" {
				row.Preview = &text
			}
		}
	}
	return nil
}

type desktopPresenter struct {
	owner             *client.Client
	connection        *client.Client
	ticket            protocol.QuickTicket
	shortcut          string
	notice            bool
	record            func(string) bool
	refreshAppearance func()
	placeAtCursor     func(*mygo.Window)
	opening           bool
	unavailable       bool
	request           uint64
	shown             time.Time
}

// NewDesktopPanel runs on the MyGo UI thread. The standalone Desktop owns
// this native window and the connection that identifies its process to Rust.
func NewDesktopPanel(owner *client.Client, placeAtCursor func(*mygo.Window)) *Panel {
	p := &Panel{local: &desktopPresenter{owner: owner, placeAtCursor: placeAtCursor}, mode: "light", locale: "zh-CN", opacity: 1}
	mygo.App.OnBeforeQuit(func(*mygo.QuitEvent) {
		p.stopping = true
		if p.local.shortcut != "" {
			mygo.GlobalShortcut.Unregister(p.local.shortcut)
		}
		p.releaseDesktopDisplay()
	})
	p.RefreshShortcut()
	return p
}

func (p *Panel) prepareDesktopWindow() {
	p.window = mygo.NewWindow(mygo.WindowOptions{Title: "DezirClip 快速粘贴", Width: 560, Height: 440,
		Hidden: true, Panel: true, Frameless: true, Transparent: true, DisableResize: true, Content: ui.View(p.view)})
	window := p.window
	p.window.OnClose(func(e *mygo.CloseEvent) {
		if !p.stopping {
			e.PreventDefault()
			p.dismiss("cancel")
		}
	})
	p.window.OnBlur(func() {
		if p.window != window {
			return
		}
		const settle = 150 * time.Millisecond
		if delay := settle - time.Since(p.local.shown); delay > 0 {
			request := p.local.request
			time.AfterFunc(delay, func() {
				mygo.RunOnMain(func() {
					if !p.stopping && request == p.local.request && window == p.window && !window.IsFocused() {
						p.dismiss("blur")
					}
				})
			})
		} else {
			p.dismiss("blur")
		}
	})
}

func (p *Panel) preferences() (protocol.QuickPreferences, error) {
	var pref protocol.QuickPreferences
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	read := func() error {
		return p.local.owner.DesktopCall(ctx, protocol.MethodDesktopQuickPreferences, protocol.EmptyParams{}, "QuickPreferences", &pref)
	}
	err := read()
	var remote *client.RemoteError
	if errors.As(err, &remote) && remote.Code == protocol.WireErrorCodeBusy {
		err = read()
	}
	return pref, err
}

func (p *Panel) SetShortcutRecorder(record func(string) bool) { p.local.record = record }

func (p *Panel) SetAppearanceRefresher(refresh func()) { p.local.refreshAppearance = refresh }

func (p *Panel) SetAppearance(appearance theme.Appearance, kit uikit.Kit) {
	if !appearance.Valid() {
		return
	}
	p.applyAppearance(appearance, kit)
	if p.window != nil && !p.window.IsDestroyed() {
		p.window.Update(func() {})
	}
}

func (p *Panel) applyAppearance(appearance theme.Appearance, kit uikit.Kit) {
	p.appearance = &appearance
	p.kit = kit.WithSurfaceOpacity(int(appearance.Opacity*100 + .5))
	p.mode, p.locale, p.opacity = appearance.Mode, appearance.Locale, appearance.Opacity
}

func (p *Panel) HasShortcut() bool { return p.local != nil && p.local.shortcut != "" }

func (p *Panel) RefreshShortcut() error {
	if p.stopping {
		return errors.New("QuickPasteStopping")
	}
	pref, err := p.preferences()
	if err != nil {
		log.Print("Quick paste settings unavailable")
		return err
	}
	hotkey := pref.Hotkey
	if !pref.Enabled {
		hotkey = ""
		p.local.request++
		p.local.opening = false
		if p.model.Visible {
			p.dismiss("cancel")
		}
		p.releaseDesktopDisplay()
	}
	if hotkey == p.local.shortcut {
		return nil
	}
	if hotkey != "" {
		if err := mygo.GlobalShortcut.Register(hotkey, func() {
			if p.local.record == nil || !p.local.record(hotkey) {
				p.toggleDesktop(hotkey)
			}
		}); err != nil {
			log.Printf("Quick paste shortcut unavailable: %v", err)
			return err
		}
	}
	if p.local.shortcut != "" {
		mygo.GlobalShortcut.Unregister(p.local.shortcut)
	}
	p.local.shortcut = hotkey
	return nil
}

func (p *Panel) ShowDesktop() {
	p.toggleDesktop("")
}

func (p *Panel) toggleDesktop(shortcut string) {
	if p.stopping || p.local == nil {
		return
	}
	if p.model.Visible || p.local.unavailable {
		p.dismiss("cancel")
		return
	}
	if p.local.opening {
		return
	}
	p.local.request++
	request := p.local.request
	p.local.opening = true
	if p.local.refreshAppearance != nil {
		p.local.refreshAppearance()
	}
	go p.openDesktop(request, shortcut)
}

func (p *Panel) openDesktop(request uint64, shortcut string) {
	pref, err := p.preferences()
	if err == nil && (!pref.Enabled || shortcut != "" && pref.Hotkey != shortcut) {
		mygo.RunOnMain(func() {
			if !p.stopping && p.local.request == request {
				p.local.opening = false
				_ = p.RefreshShortcut()
			}
		})
		return
	}
	var host *client.Client
	var ticket protocol.QuickTicket
	if err == nil {
		host, ticket, err = p.beginDesktopDisplay()
	}
	mygo.RunOnMain(func() {
		if p.stopping || p.local.request != request {
			if host != nil {
				host.Close()
			}
			return
		}
		p.local.opening = false
		if p.window == nil || p.window.IsDestroyed() {
			p.prepareDesktopWindow()
		}
		p.local.unavailable = err != nil
		if err != nil {
			log.Printf("Quick paste could not open: %v", err)
		} else {
			p.local.connection, p.local.ticket = host, ticket
			p.model.Show(host.Hello().SessionId, ticket.Token, ticket.State)
			p.model.ShowNotice, p.local.notice = p.local.notice, false
		}
		p.local.placeAtCursor(p.window)
		p.local.shown = time.Now()
		p.window.Invalidate()
		p.window.Show()
		p.window.Focus()
		if host != nil {
			p.watchDesktopDisplay(host, ticket)
		}
	})
}

func (p *Panel) beginDesktopDisplay() (*client.Client, protocol.QuickTicket, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	var err error
	for attempt := 0; attempt < 2; attempt++ {
		host, connectErr := p.local.owner.DesktopConnection(ctx)
		if connectErr != nil {
			return nil, protocol.QuickTicket{}, connectErr
		}
		var ticket protocol.QuickTicket
		ticket, err = host.BeginQuickPaste(ctx, host.Hello().State)
		if err == nil {
			return host, ticket, nil
		}
		host.Close()
		var remote *client.RemoteError
		if !errors.As(err, &remote) ||
			(remote.Code != protocol.WireErrorCodeHistoryRevisionChanged && remote.Code != protocol.WireErrorCodePrivacyEpochChanged) {
			break
		}
	}
	return nil, protocol.QuickTicket{}, err
}

func (p *Panel) watchDesktopDisplay(host *client.Client, ticket protocol.QuickTicket) {
	window := p.window
	p.query(0)
	displayCtx, stop := context.WithCancel(context.Background())
	p.displayCancel = stop
	go func() {
		events, err := host.DesktopConnection(displayCtx)
		if err != nil {
			window.Update(func() {
				if p.model.Display == ticket.Token {
					p.releaseDesktopDisplay()
				}
			})
			return
		}
		defer events.Close()
		p.watchDisplay(displayCtx, events, ticket.State, ticket.Token, window)
	}()
}

func (p *Panel) finishDesktopDisplay(id int64, restore bool) {
	host, ticket, state := p.local.connection, p.local.ticket, p.model.State
	window := p.window
	// Keep the panel foreground until Rust validates and restores the originally
	// captured target. A blur during delivery must not issue a second action.
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		var err error
		if id != 0 {
			err = host.PasteQuickSelection(ctx, ticket, state, id)
		} else {
			err = host.DismissQuickPaste(ctx, ticket, restore)
		}
		if err != nil {
			log.Printf("Quick paste action failed: %v", err)
		}
		window.Update(func() {
			if p.model.Display != ticket.Token {
				return
			}
			p.local.notice = id != 0 && err != nil
			p.releaseDesktopDisplay()
		})
	}()
}

func (p *Panel) releaseDesktopDisplay() {
	connection := p.local.connection
	p.local.connection = nil
	p.local.ticket = protocol.QuickTicket{}
	p.local.unavailable = false
	window := p.window
	p.window = nil
	p.hideLocal()
	if window != nil {
		window.Destroy()
	}
	if connection != nil {
		connection.Close()
	}
}
