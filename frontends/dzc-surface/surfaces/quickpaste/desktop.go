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
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/theme"
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
}

// NewDesktopPanel runs on the MyGo UI thread. The standalone Desktop owns
// this native window and the connection that identifies its process to Rust.
func NewDesktopPanel(owner *client.Client, placeAtCursor func(*mygo.Window)) *Panel {
	p := &Panel{local: &desktopPresenter{owner: owner, placeAtCursor: placeAtCursor}, mode: "light", locale: "zh-CN", opacity: 1}
	p.prepareDesktopWindow()
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
	p.window.OnClose(func(e *mygo.CloseEvent) {
		if !p.stopping {
			e.PreventDefault()
			p.dismiss("cancel")
		}
	})
	p.window.OnBlur(func() { p.dismiss("blur") })
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

func (p *Panel) SetAppearance(appearance theme.Appearance) {
	if !appearance.Valid() {
		return
	}
	p.appearance = &appearance
	p.mode, p.locale, p.opacity = appearance.Mode, appearance.Locale, appearance.Opacity
	if !p.window.IsDestroyed() {
		p.window.Update(func() {})
	}
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
		if p.model.Visible {
			p.dismiss("cancel")
		}
		p.releaseDesktopDisplay()
		p.window.Destroy()
	} else if p.window.IsDestroyed() {
		p.prepareDesktopWindow()
	}
	if hotkey == p.local.shortcut {
		return nil
	}
	if hotkey != "" {
		if err := mygo.GlobalShortcut.Register(hotkey, func() {
			if p.local.record == nil || !p.local.record(hotkey) {
				p.toggleDesktop()
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

func (p *Panel) toggleDesktop() {
	if p.stopping {
		return
	}
	if p.model.Visible {
		p.dismiss("cancel")
		return
	}
	if p.local.refreshAppearance != nil {
		p.local.refreshAppearance()
	}
	pref, err := p.preferences()
	if err != nil || !pref.Enabled || pref.Hotkey != p.local.shortcut {
		p.RefreshShortcut()
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	host, err := p.local.owner.DesktopConnection(ctx)
	if err != nil {
		return
	}
	ticket, err := host.BeginQuickPaste(ctx, host.Hello().State)
	if err != nil {
		host.Close()
		return
	}
	p.local.connection, p.local.ticket = host, ticket
	p.model.Show(host.Hello().SessionId, ticket.Token, ticket.State)
	p.model.ShowNotice, p.local.notice = p.local.notice, false
	p.local.placeAtCursor(p.window)
	p.window.Show()
	p.window.Focus()
	p.query(0)
	displayCtx, stop := context.WithCancel(context.Background())
	p.displayCancel = stop
	go func() {
		events, err := host.DesktopConnection(displayCtx)
		if err != nil {
			p.window.Update(func() {
				if p.model.Display == ticket.Token {
					p.releaseDesktopDisplay()
				}
			})
			return
		}
		defer events.Close()
		p.watchDisplay(displayCtx, events, ticket.State, ticket.Token)
	}()
}

func (p *Panel) finishDesktopDisplay(id int64, restore bool) {
	host, ticket, state := p.local.connection, p.local.ticket, p.model.State
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
		p.window.Update(func() {
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
	p.hideLocal()
	if connection != nil {
		connection.Close()
	}
}
