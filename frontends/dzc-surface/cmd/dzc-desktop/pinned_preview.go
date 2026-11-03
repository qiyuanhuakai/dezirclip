package main

import (
	"context"
	"errors"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Only the selection and its authority cross the pipe. Each pinned native window
// reads its own body and follows the Service even after the main WebView exits.
type pinPreviewRequest struct {
	Params     protocol.DesktopEntryParams
	Appearance previewAppearance
	Bounds     mygo.Rectangle
}

type pinnedPreview struct {
	owner   *client.Client
	ticket  pinPreviewRequest
	window  *mygo.Window
	cancel  context.CancelFunc
	body    *previewBody
	loading bool
}

func openPinnedPreview(owner *client.Client, ticket *pinPreviewRequest, closed func()) error {
	if ticket == nil || ticket.Params.Id <= 0 || ticket.Params.ServiceInstanceId != owner.Hello().State.ServiceInstanceId ||
		ticket.Bounds.Width <= 0 || ticket.Bounds.Height <= 0 {
		return errors.New("InvalidPreview")
	}
	p := &pinnedPreview{owner: owner, ticket: *ticket}
	frame := ticket.Bounds
	area := mygo.Screen.DisplayMatching(frame).WorkArea
	if area.Width < 1 || area.Height < 1 {
		return errors.New("PreviewDisplayUnavailable")
	}
	width, height := min(360, area.Width), min(240, area.Height)
	p.window = mygo.NewWindow(mygo.WindowOptions{Title: "DezirClip 固定预览", Width: width, Height: height, Hidden: true,
		Frameless: true, SkipTaskbar: true, Transparent: true, Content: ui.View(p.view)})
	p.window.OnClosed(func() { p.conceal(); closed() })
	p.window.OnBlur(func() {
		if p.ticket.Params.Reveal {
			p.conceal()
			p.window.Update(func() {})
		}
	})
	p.window.SetAlwaysOnTop(true)
	p.window.SetPosition(max(area.X, min(frame.X, area.X+area.Width-width)), max(area.Y, min(frame.Y, area.Y+area.Height-height)))
	p.window.ShowInactive()
	if !ticket.Params.Reveal {
		p.refresh(false)
	}
	return nil
}

func (p *pinnedPreview) conceal() {
	if p.cancel != nil {
		p.cancel()
		p.cancel = nil
	}
	p.body = nil
	p.loading = false
}

func (p *pinnedPreview) view(c *ui.Context) {
	a := p.ticket.Appearance
	c.SetTheme(a.theme())
	ui.Column(c).Fill().Padding(10).Children(func() {
		ui.Row(c).FillWidth().Children(func() {
			label := "DezirClip"
			if p.body != nil && p.body.source != "" {
				label = p.body.source
			}
			ui.Text(c, label).Grow(1).DragWindow()
			if ui.Button(c, a.Close).Clicked() {
				p.window.Update(p.window.Close)
			}
		})
		if p.body != nil {
			p.body.draw(c)
			return
		}
		ui.Text(c, a.Concealed).Wrap()
		if ui.Button(c, a.Retry).Disabled(p.loading).Clicked() {
			p.refresh(true)
		}
	})
}

func (p *pinnedPreview) refresh(retry bool) {
	p.conceal()
	ctx, cancel := context.WithCancel(context.Background())
	p.cancel = cancel
	p.loading = true
	go func() {
		defer func() {
			p.window.Update(func() {
				if ctx.Err() == nil {
					p.conceal()
				}
			})
		}()
		loadCtx, stop := context.WithTimeout(ctx, 6*time.Second)
		defer stop()
		host, err := p.owner.DesktopConnection(loadCtx)
		if err != nil {
			return
		}
		defer host.Close()
		params := p.ticket.Params
		var status protocol.ServiceStatus
		if host.Call(loadCtx, protocol.MethodServiceStatus, struct{}{}, "ServiceStatus", &status) != nil || status.Privacy.Active ||
			status.State.ServiceInstanceId != params.ServiceInstanceId {
			return
		}
		if retry {
			params.ExpectedEpoch = status.State.PrivacyEpoch
		}
		body, state, err := readPreview(loadCtx, host, params, true)
		if err != nil {
			return
		}
		var latest protocol.RuntimeState
		if host.Call(loadCtx, protocol.MethodHealth, struct{}{}, "RuntimeState", &latest) != nil || state.PrivacyEpoch != latest.PrivacyEpoch ||
			state.HistoryRevision != latest.HistoryRevision || state.ServiceInstanceId != latest.ServiceInstanceId {
			return
		}
		stop()
		p.window.Update(func() {
			if ctx.Err() != nil {
				return
			}
			if params.Reveal && !p.window.IsFocused() {
				p.conceal()
				return
			}
			p.body = body
			p.loading = false
		})
		wait := uint16(500)
		for ctx.Err() == nil {
			var result protocol.EventsResult
			poll, done := context.WithTimeout(ctx, 2*time.Second)
			err := host.Call(poll, protocol.MethodEventsSubscribe, protocol.SubscribeParams{AfterSequence: state.EventSequence, WaitMs: &wait}, "EventsResult", &result)
			done()
			if err != nil || result.ResyncRequired || result.State.ServiceInstanceId != params.ServiceInstanceId || result.State.PrivacyEpoch != params.ExpectedEpoch {
				return
			}
			for _, event := range result.Events {
				if event.Event == protocol.EventKindPrivacyStateChanged || event.Event == protocol.EventKindHistoryReset ||
					((event.Event == protocol.EventKindEntryDeleted || event.Event == protocol.EventKindEntryUpdated) &&
						(event.Data.EntryId == nil || *event.Data.EntryId == params.Id)) {
					return
				}
			}
			state = result.State
		}
	}()
}
