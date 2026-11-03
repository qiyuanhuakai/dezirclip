package main

import (
	"context"
	"encoding/base64"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// The WebView supplies only a row ID. The preview reads its own privacy-bound
// snapshot and never sends full content or image bytes through JavaScript.
type Preview struct {
	owner        *client.Client
	main, window *mygo.Window
	generation   uint64
	cancel       context.CancelFunc
}

func (p *Preview) clear() {
	if p.cancel != nil {
		p.cancel()
		p.cancel = nil
	}
	if p.window != nil {
		p.window.Destroy()
		p.window = nil
	}
}

func (p *Preview) Select(ctx context.Context, sequence uint64, id int64, state protocol.RuntimeState) {
	main := mygo.CallerWindow(ctx)
	if main == nil {
		return
	}
	main.Update(func() {
		if main != p.main || sequence <= p.generation {
			return
		}
		p.generation = sequence
		p.clear()
		if id == 0 || !p.main.IsFocused() {
			return
		}
		ctx, cancel := context.WithCancel(context.Background())
		p.cancel = cancel
		go p.load(ctx, main, sequence, id, state)
	})
}

func (p *Preview) load(ctx context.Context, main *mygo.Window, sequence uint64, id int64, state protocol.RuntimeState) {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	host, err := p.owner.DesktopConnection(ctx)
	if err != nil {
		return
	}
	defer host.Close()
	size := uint32(560)
	params := protocol.DesktopEntryParams{Id: id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, ThumbnailDimension: &size}
	var snapshot protocol.BodySnapshot
	if host.Call(ctx, protocol.MethodDesktopHistoryGet, params, "BodySnapshot", &snapshot) != nil {
		return
	}
	var row protocol.DesktopEntry
	if host.ReadSnapshot(ctx, snapshot, "DesktopEntry", &row) != nil || row.Id != id {
		return
	}
	var bitmap *ui.Bitmap
	text := row.Content
	if row.ContentType == "image" {
		header, encoded, ok := strings.Cut(text, ",")
		if !ok || !strings.HasPrefix(header, "data:image/") || !strings.HasSuffix(header, ";base64") {
			return
		}
		raw, err := base64.StdEncoding.DecodeString(encoded)
		if err != nil {
			return
		}
		bitmap, err = ui.DecodeBitmap(raw)
		clear(raw)
		if err != nil {
			return
		}
		text = ""
	} else {
		if text == "" {
			text = row.Preview
		}
		// Bound shaping and layout work for long clipboard entries.
		for index := range text {
			if index >= 4096 {
				text = strings.Clone(text[:index]) + "…"
				break
			}
		}
	}
	source := row.SourceApp
	main.Update(func() {
		if main != p.main || p.generation != sequence || p.cancel == nil || !main.IsFocused() {
			return
		}
		cursor := mygo.Screen.CursorScreenPoint()
		area := mygo.Screen.DisplayNearestPoint(cursor).WorkArea
		width, height := min(360, area.Width), min(200, area.Height)
		p.window = mygo.NewWindow(mygo.WindowOptions{Title: "DezirClip 悬浮预览", Width: width, Height: height, Hidden: true, Frameless: true, Panel: true, Content: ui.View(func(c *ui.Context) {
			ui.Column(c).Fill().Padding(10).Children(func() {
				ui.Text(c, source)
				if bitmap != nil {
					ui.Image(c, bitmap).Fill()
				} else {
					ui.Text(c, text).Wrap()
				}
			})
		})})
		p.window.SetIgnoreMouseEvents(true)
		x := cursor.X + 16
		if x+width > area.X+area.Width {
			x = cursor.X - width - 16
		}
		p.window.SetPosition(max(area.X, x), max(area.Y, min(cursor.Y, area.Y+area.Height-height)))
		p.window.ShowInactive()
	})
}
