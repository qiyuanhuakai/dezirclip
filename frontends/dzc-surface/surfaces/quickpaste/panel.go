package quickpaste

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/control"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/model"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/theme"
)

type outgoing struct {
	message protocol.SurfaceControlMessage
	done    chan error
}

// Panel state is touched only by MyGo's main thread. Network and private-pipe
// goroutines deliver results through Window.Update with the original tickets.
type Panel struct {
	window         *mygo.Window
	model          model.Model
	init           *protocol.SurfaceInitialize
	initializing   bool
	stopping       bool
	locale         string
	mode           string
	opacity        float64
	search         string
	list           ui.ListState
	out            chan outgoing
	sequence       uint64
	requests       map[string]bool
	displays       map[string]bool
	initCancel     context.CancelFunc
	displayCancel  context.CancelFunc
	queryCancel    context.CancelFunc
	debounce       *time.Timer
	debounceUntil  time.Time
	actionDeadline *time.Timer
}

func Run() error {
	if !buildinfo.Valid() {
		return fmt.Errorf("surface build identity is unavailable")
	}
	p := &Panel{out: make(chan outgoing, 16), requests: make(map[string]bool), displays: make(map[string]bool), mode: "light", locale: "en", opacity: 1}
	mygo.App.WhenReady(func() {
		p.window = mygo.NewWindow(mygo.WindowOptions{Title: "DezirClip", Width: 560, Height: 440, Hidden: true, Panel: true, Frameless: true, Transparent: true, DisableResize: true, Content: ui.View(p.view)})
		p.window.OnClose(func(e *mygo.CloseEvent) {
			if p.stopping {
				return
			}
			e.PreventDefault()
			p.dismiss("cancel")
		})
		p.window.OnBlur(func() {
			if p.model.Visible {
				p.dismiss("blur")
			}
		})
		go p.writeLoop()
		go p.readLoop()
	})
	mygo.App.OnBeforeQuit(func(_ *mygo.QuitEvent) { p.stopLocal() })
	mygo.App.OnWindowAllClosed(func() {})
	return mygo.App.Run()
}

func (p *Panel) readLoop() {
	for {
		message, err := control.Read(os.Stdin, control.ParentToChild)
		if err != nil {
			p.window.Update(func() { p.stopLocal(); mygo.App.Quit() })
			return
		}
		p.window.Update(func() { p.receive(message) })
	}
}

func (p *Panel) writeLoop() {
	for next := range p.out {
		err := control.Write(os.Stdout, next.message, control.ChildToParent)
		next.done <- err
		if err != nil {
			p.window.Update(func() { p.stopLocal(); mygo.App.Quit() })
			return
		}
	}
}

func (p *Panel) send(id string, kind protocol.SurfaceControlType, payload any, after func()) {
	message, err := control.Message(id, kind, payload)
	if err != nil {
		p.stopLocal()
		mygo.App.Quit()
		return
	}
	next := outgoing{message, make(chan error, 1)}
	select {
	case p.out <- next:
	default:
		p.stopLocal()
		mygo.App.Quit()
		return
	}
	go func() {
		select {
		case err := <-next.done:
			p.window.Update(func() {
				if err != nil {
					p.stopLocal()
					mygo.App.Quit()
				} else if after != nil {
					after()
				}
			})
		case <-time.After(5 * time.Second):
			p.window.Update(func() { p.stopLocal(); mygo.App.Quit() })
		}
	}()
}

func (p *Panel) nextID() string { p.sequence++; return "surface-" + strconv.FormatUint(p.sequence, 10) }

func (p *Panel) fail(id, code string) {
	p.hideLocal()
	session := ""
	if p.init != nil {
		session = p.init.SurfaceSessionId
	}
	p.send(id, protocol.SurfaceControlTypeFailed, protocol.SurfaceFailed{SurfaceSessionId: session, ErrorCode: code}, nil)
}

func decode(raw json.RawMessage, schema string, value any) bool {
	return protocol.Decode(schema, raw, value) == nil
}

func (p *Panel) receive(message protocol.SurfaceControlMessage) {
	if p.stopping {
		return
	}
	if p.requests[message.RequestId] || len(p.requests) >= 4096 {
		p.stopLocal()
		mygo.App.Quit()
		return
	}
	p.requests[message.RequestId] = true
	switch message.Type {
	case protocol.SurfaceControlTypeInitialize:
		var init protocol.SurfaceInitialize
		if p.init != nil || p.initializing || !decode(message.Payload, "SurfaceInitialize", &init) || !control.SameBuild(init.Build, buildinfo.Identity()) {
			p.fail(message.RequestId, "initialize_rejected")
			return
		}
		p.initializing = true
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		p.initCancel = cancel
		go func() {
			host, err := client.DialSurface(ctx, init.Endpoint, init.Grant, buildinfo.Identity())
			var state protocol.RuntimeState
			if err == nil {
				state = host.Hello().State
				host.Close()
			}
			cancel()
			p.window.Update(func() {
				if p.stopping {
					return
				}
				p.initializing = false
				if err != nil || !model.SameIdentity(init.State, state) || state.EventSequence < init.State.EventSequence {
					p.fail(message.RequestId, "host_handshake_rejected")
					return
				}
				p.init = &init
				p.init.State = state
				p.locale, p.mode = init.Locale, init.Theme.Mode
				p.opacity = init.Theme.Opacity
				mygo.App.SetLocale(model.NativeLocale(init.Locale))
				p.send(message.RequestId, protocol.SurfaceControlTypeReady, protocol.SurfaceReady{SurfaceSessionId: init.SurfaceSessionId, Build: buildinfo.Identity(), State: state}, nil)
			})
		}()
	case protocol.SurfaceControlTypeShow:
		var show protocol.SurfaceShow
		if p.init == nil || !decode(message.Payload, "SurfaceShow", &show) || show.SurfaceSessionId != p.init.SurfaceSessionId || p.displays[show.DisplayRequestId] || len(p.displays) >= 4096 {
			p.fail(message.RequestId, "show_rejected")
			return
		}
		bounds, valid := model.LogicalBounds(show.Rect, show.Scale)
		if !valid || show.State.ServiceInstanceId != p.init.State.ServiceInstanceId || show.State.PrivacyEpoch != p.init.State.PrivacyEpoch {
			p.fail(message.RequestId, "state_changed")
			return
		}
		if !p.hideLocal() {
			p.stopping = true
			mygo.App.Quit()
			return
		}
		p.displays[show.DisplayRequestId] = true
		p.model.Show(show.SurfaceSessionId, show.DisplayRequestId, show.State)
		p.model.ShowNotice = show.Notice != nil && *show.Notice == "paste_unavailable"
		p.search = ""
		p.list = ui.ListState{}
		p.locale, p.mode = show.Locale, show.Theme.Mode
		p.opacity = show.Theme.Opacity
		mygo.App.SetLocale(model.NativeLocale(show.Locale))
		ctx, cancel := context.WithCancel(context.Background())
		p.displayCancel = cancel
		endpoint, grant := p.init.Endpoint, p.init.Grant
		go p.connectDisplay(ctx, show, bounds, message.RequestId, endpoint, grant)
	case protocol.SurfaceControlTypeHide:
		var hide protocol.SurfaceHide
		if p.init == nil || !decode(message.Payload, "SurfaceHide", &hide) || hide.SurfaceSessionId != p.init.SurfaceSessionId {
			p.fail(message.RequestId, "hide_rejected")
			return
		}
		// A late hide ACK must never hide a newer activation.
		if hide.DisplayRequestId == p.model.Display {
			if !p.hideLocal() {
				p.stopping = true
				mygo.App.Quit()
				return
			}
		}
		p.send(message.RequestId, protocol.SurfaceControlTypeHidden, protocol.SurfaceHidden{SurfaceSessionId: hide.SurfaceSessionId, DisplayRequestId: hide.DisplayRequestId}, nil)
	case protocol.SurfaceControlTypePasteResult:
		var result protocol.SurfacePasteResult
		if p.init == nil || !decode(message.Payload, "SurfacePasteResult", &result) || result.SurfaceSessionId != p.init.SurfaceSessionId {
			p.fail(message.RequestId, "action_result_rejected")
			return
		}
		if result.DisplayRequestId == p.model.Display && result.ActionId == p.model.PendingAction {
			if p.actionDeadline != nil {
				p.actionDeadline.Stop()
			}
			if result.Performed {
				p.hideLocal()
			} else {
				p.model.PendingAction = ""
				p.model.Error = p.word("paste_failed")
			}
		}
	case protocol.SurfaceControlTypeShutdown:
		var stop protocol.SurfaceShutdown
		if !decode(message.Payload, "SurfaceShutdown", &stop) || (p.init != nil && stop.SurfaceSessionId != p.init.SurfaceSessionId) {
			p.fail(message.RequestId, "shutdown_rejected")
			return
		}
		p.stopLocal()
		p.send(message.RequestId, protocol.SurfaceControlTypeStopped, protocol.SurfaceStopped{SurfaceSessionId: stop.SurfaceSessionId}, func() { mygo.App.Quit() })
	default:
		p.stopLocal()
		mygo.App.Quit()
	}
}

func (p *Panel) connectDisplay(ctx context.Context, show protocol.SurfaceShow, bounds model.Bounds, requestID, endpoint, grant string) {
	host, err := client.DialSurface(ctx, endpoint, grant, buildinfo.Identity())
	if err != nil {
		p.window.Update(func() {
			if p.model.Display == show.DisplayRequestId {
				p.fail(requestID, "host_unavailable")
			}
		})
		return
	}
	defer host.Close()
	state := host.Hello().State
	accepted := make(chan bool, 1)
	p.window.Update(func() {
		if ctx.Err() != nil || p.model.Display != show.DisplayRequestId {
			accepted <- false
			return
		}
		if !model.SameIdentity(state, show.State) || state.EventSequence < show.State.EventSequence {
			p.fail(requestID, "state_changed")
			accepted <- false
			return
		}
		p.model.State = state
		p.window.SetBounds(mygo.Rectangle{X: bounds.X, Y: bounds.Y, Width: bounds.Width, Height: bounds.Height})
		p.window.Show()
		if !p.window.IsVisible() {
			p.fail(requestID, "show_failed")
			accepted <- false
			return
		}
		p.send(requestID, protocol.SurfaceControlTypeShown, protocol.SurfaceShown{SurfaceSessionId: show.SurfaceSessionId, DisplayRequestId: show.DisplayRequestId, State: state}, nil)
		p.query(0)
		accepted <- true
	})
	select {
	case ok := <-accepted:
		if !ok {
			return
		}
	case <-ctx.Done():
		return
	}
	ticker := time.NewTicker(500 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			zero := uint16(0)
			var events protocol.EventsResult
			err := host.Call(ctx, protocol.MethodEventsSubscribe, protocol.SubscribeParams{AfterSequence: state.EventSequence, WaitMs: &zero}, "EventsResult", &events)
			if ctx.Err() != nil {
				return
			}
			if err != nil {
				p.window.Update(func() {
					if p.model.Display == show.DisplayRequestId {
						p.fail(p.nextID(), "host_unavailable")
					}
				})
				return
			}
			previous := state
			nextState := events.State
			state = nextState
			p.window.Update(func() {
				if ctx.Err() != nil || p.model.Display != show.DisplayRequestId {
					return
				}
				if events.ResyncRequired || nextState.ServiceInstanceId != previous.ServiceInstanceId || nextState.PrivacyEpoch != previous.PrivacyEpoch || nextState.HistoryRevision < previous.HistoryRevision || nextState.EventSequence < previous.EventSequence {
					p.fail(p.nextID(), "resync_required")
					return
				}
				if nextState.HistoryRevision != p.model.State.HistoryRevision {
					p.cancelQuery()
					p.model.Sequence++
					p.model.Entries = nil
					p.model.Selected = 0
					p.model.HasMore = false
					p.model.State = nextState
					if !p.model.Composing {
						p.scheduleQuery()
					}
				}
			})
		}
	}
}

func (p *Panel) cancelQuery() {
	if p.queryCancel != nil {
		p.queryCancel()
		p.queryCancel = nil
	}
	if p.debounce != nil {
		p.debounce.Stop()
		p.debounce = nil
	}
}

func (p *Panel) query(offset uint32) {
	if !p.model.Visible || p.model.Composing || p.init == nil {
		return
	}
	p.cancelQuery()
	ticket := p.model.Begin(offset)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	p.queryCancel = cancel
	endpoint, grant := p.init.Endpoint, p.init.Grant
	go func() {
		defer cancel()
		host, err := client.DialSurface(ctx, endpoint, grant, buildinfo.Identity())
		var page protocol.SurfacePage
		if err == nil {
			defer host.Close()
			request := protocol.SurfacePageParams{ServiceInstanceId: ticket.State.ServiceInstanceId, ExpectedEpoch: ticket.State.PrivacyEpoch, ExpectedHistoryRevision: ticket.State.HistoryRevision, Limit: model.PageSize, Offset: offset}
			if ticket.Query != "" {
				query := ticket.Query
				mode := protocol.SearchModeFuzzy
				request.Query = &query
				request.Mode = &mode
			}
			page, err = host.SurfacePage(ctx, request)
		}
		cancelled := ctx.Err() == context.Canceled
		p.window.Update(func() {
			if !p.model.Current(ticket) || cancelled {
				return
			}
			if err != nil {
				p.model.Loading = false
				p.model.Entries = nil
				p.model.HasMore = false
				p.model.Error = p.word("read_failed")
				return
			}
			if !p.model.Apply(ticket, page) {
				p.fail(p.nextID(), "state_changed")
			}
		})
	}()
}

func (p *Panel) edit(query string, composing bool) {
	if !p.model.Edit(query, composing) {
		return
	}
	p.cancelQuery()
	p.debounceUntil = time.Now().Add(300 * time.Millisecond)
	if composing {
		return
	}
	p.scheduleQuery()
}

func (p *Panel) scheduleQuery() {
	if !p.model.Visible || p.model.Composing {
		return
	}
	remaining := time.Until(p.debounceUntil)
	if remaining <= 0 {
		p.query(0)
		return
	}
	ticket := p.model.Begin(0)
	p.debounce = time.AfterFunc(remaining, func() {
		p.window.Update(func() {
			if p.model.Current(ticket) && !p.model.Composing {
				p.query(0)
			}
		})
	})
}

func (p *Panel) paste() {
	// Read the current native input binding, not a value captured by an old frame.
	if p.search != p.model.RawQuery {
		p.edit(p.search, p.model.Composing)
		return
	}
	if !p.model.CanSelect() {
		return
	}
	action := p.nextID()
	p.model.PendingAction = action
	row := p.model.Entries[p.model.Selected]
	p.send(action, protocol.SurfaceControlTypePaste, protocol.SurfacePaste{SurfaceSessionId: p.model.Session, DisplayRequestId: p.model.Display, ActionId: action, EntryId: row.Id, State: p.model.State}, nil)
	display := p.model.Display
	p.actionDeadline = time.AfterFunc(5*time.Second, func() {
		p.window.Update(func() {
			if p.model.Display == display && p.model.PendingAction == action {
				p.fail(p.nextID(), "action_timeout")
			}
		})
	})
}

func (p *Panel) dismiss(reason string) {
	if !p.model.Visible {
		return
	}
	message := protocol.SurfaceDismiss{SurfaceSessionId: p.model.Session, DisplayRequestId: p.model.Display, Reason: reason}
	if !p.hideLocal() {
		p.stopping = true
		mygo.App.Quit()
		return
	}
	p.send(p.nextID(), protocol.SurfaceControlTypeDismiss, message, nil)
}

func (p *Panel) hideLocal() bool {
	if p.displayCancel != nil {
		p.displayCancel()
		p.displayCancel = nil
	}
	p.cancelQuery()
	p.debounceUntil = time.Time{}
	if p.actionDeadline != nil {
		p.actionDeadline.Stop()
		p.actionDeadline = nil
	}
	p.model.Clear()
	p.search = ""
	p.list = ui.ListState{}
	if p.window != nil {
		p.window.Hide()
		return !p.window.IsVisible()
	}
	return true
}

func (p *Panel) stopLocal() {
	p.stopping = true
	if p.initCancel != nil {
		p.initCancel()
	}
	p.hideLocal()
	if p.init != nil {
		p.init.Grant = ""
		p.init.Endpoint = ""
	}
}

func (p *Panel) word(key string) string {
	zh := strings.HasPrefix(p.locale, "zh")
	traditional := p.locale == "zh-TW" || p.locale == "zh-Hant" || p.locale == "tw"
	words := map[string][3]string{
		"search": {"Search history", "搜索历史", "搜尋歷史"}, "empty": {"No results", "没有匹配记录", "沒有相符記錄"},
		"loading": {"Loading…", "正在加载…", "正在載入…"}, "hidden": {"Protected record", "受保护记录", "受保護記錄"},
		"read_failed":  {"Could not read history. Retry.", "读取失败，请重试。", "讀取失敗，請重試。"},
		"paste_failed": {"Paste failed. Select again to retry.", "粘贴失败，请重新选择。", "貼上失敗，請重新選擇。"},
		"paste_notice": {"Previous paste could not be confirmed. Check the target content before selecting again.", "上次粘贴无法确认，请先检查目标内容后重新选择。", "上次貼上無法確認，請先檢查目標內容後重新選擇。"},
		"more":         {"Load more", "加载更多", "載入更多"}, "retry": {"Retry", "重试", "重試"},
		"hint": {"↑ ↓ Select   Enter Paste   Esc Close", "↑ ↓ 选择   Enter 粘贴   Esc 关闭", "↑ ↓ 選擇   Enter 貼上   Esc 關閉"},
	}
	index := 0
	if zh || p.locale == "tw" {
		index = 1
	}
	if traditional {
		index = 2
	}
	return words[key][index]
}

func (p *Panel) view(c *ui.Context) {
	tokens := theme.Select(p.mode)
	t := ui.LightTheme()
	if p.mode == "dark" {
		t = ui.DarkTheme()
	}
	t.Background, t.Surface, t.Text, t.TextMuted = ui.Hex(tokens.Background).Alpha(float32(p.opacity)), ui.Hex(tokens.Panel), ui.Hex(tokens.Text), ui.Hex(tokens.Muted)
	t.Accent, t.Selection, t.Border = ui.Hex(tokens.Accent), ui.Hex(tokens.Selected), ui.Hex(tokens.Border)
	t.Radius, t.FontSize = float32(tokens.Radius), float32(tokens.FontSize)
	if len(tokens.FontFamilies) > 0 {
		t.Font = tokens.FontFamilies[0]
	}
	c.SetTheme(t)
	if !p.model.Visible {
		return
	}
	display := p.model.Display
	ui.Column(c.Key("display-"+display)).Fill().Padding(float32(tokens.Padding)).Gap(float32(tokens.Gap)).Radius(float32(tokens.Radius)).Border(1, t.Border).Background(t.Background).Children(func() {
		input := ui.TextInput(c.Key("query-"+p.model.Display), &p.search).Placeholder(p.word("search")).Label(p.word("search")).Height(float32(tokens.SearchHeight)).AutoFocus()
		input.OnTextKey(func(key ui.TextKey) bool {
			if !p.model.Visible || p.model.Display != display {
				return true
			}
			if key.Mods != 0 {
				return false
			}
			switch key.Key {
			case ui.KeyEscape:
				p.dismiss("cancel")
				return true
			case ui.KeyDown:
				p.model.Move(1)
				p.list.ScrollTo(p.model.Selected, ui.Center)
				return true
			case ui.KeyUp:
				p.model.Move(-1)
				p.list.ScrollTo(p.model.Selected, ui.Center)
				return true
			case ui.KeyEnter:
				if key.Text() == p.search {
					p.paste()
				}
				return true
			}
			return false
		})
		p.edit(p.search, input.Composing())
		if p.model.ShowNotice {
			ui.Text(c, p.word("paste_notice")).TextColor(t.Text).Padding(6).Border(1, t.Border).Radius(float32(tokens.Radius))
		}
		c.OnShortcut(0, ui.KeyEscape, func() {
			if p.model.Display == display {
				p.dismiss("cancel")
			}
		})
		p.list.Selected = &p.model.Selected
		p.list.Key = func(i int) any { return p.model.Entries[i].Id }
		p.list.Label = func(i int) string { return model.DisplayText(p.model.Entries[i], p.word("hidden")) }
		list := ui.List(c.Key("entries-"+display), &p.list, len(p.model.Entries), func(i int) {
			entry := p.model.Entries[i]
			ui.Column(c).Height(float32(tokens.RowHeight)).Padding(4, 8).Children(func() {
				ui.Text(c, model.DisplayText(entry, p.word("hidden"))).FontWeight(tokens.RegularWeight)
				ui.Text(c, entry.SourceApp).FontSize(float32(tokens.FontSize - 2)).TextColor(t.TextMuted)
			})
		}).Grow(1).Disabled(p.model.Loading || p.model.PendingAction != "").Children(func() {
			if len(p.model.Entries) == 0 {
				label := p.word("empty")
				if p.model.Loading {
					label = p.word("loading")
				}
				ui.Text(c, label).TextColor(t.TextMuted).Padding(12)
			}
		})
		if list.Submitted() {
			p.paste()
		}
		if p.model.Error != "" {
			ui.Text(c, p.model.Error).TextColor(t.Danger)
			if ui.Button(c, p.word("retry")).Clicked() {
				p.query(0)
			}
		}
		if p.model.HasMore && ui.Button(c, p.word("more")).Disabled(p.model.Loading).Clicked() {
			p.query(p.model.NextOffset)
		}
		ui.Text(c, p.word("hint")).FontSize(float32(tokens.FontSize - 2)).TextColor(t.TextMuted)
	})
}
