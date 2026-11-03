package main

import (
	"context"
	"encoding/base64"
	"errors"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
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

type previewLayout struct {
	Main     mygo.Rectangle
	WorkArea mygo.Rectangle
	Width    int
	Height   int
}

func (*Preview) Layout(ctx context.Context) (previewLayout, error) {
	main := mygo.CallerWindow(ctx)
	if main == nil {
		return previewLayout{}, errors.New("WindowUnavailable")
	}
	bounds := main.ContentBounds()
	area := mygo.Screen.DisplayMatching(bounds).WorkArea
	if bounds.Width < 1 || bounds.Height < 1 || area.Width < 1 || area.Height < 1 {
		return previewLayout{}, errors.New("PreviewPositionUnavailable")
	}
	return previewLayout{bounds, area, min(360, area.Width), min(200, area.Height)}, nil
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

type previewAppearance struct {
	Opacity    int
	Dark       bool
	Background *struct{ R, G, B uint8 }
}

func (a previewAppearance) theme() *ui.Theme {
	theme := ui.LightTheme()
	if a.Dark {
		theme = ui.DarkTheme()
	}
	if rgb := a.Background; rgb != nil {
		theme.Background = ui.RGB(rgb.R, rgb.G, rgb.B)
	}
	theme.Background = theme.Background.Alpha(float32(max(0, min(100, a.Opacity))) / 100)
	return theme
}

func (p *Preview) Select(ctx context.Context, sequence uint64, id int64, state protocol.RuntimeState) {
	p.selectEntry(ctx, sequence, id, state, false, nil, previewAppearance{Opacity: 65})
}

func (p *Preview) ShowEntry(ctx context.Context, sequence uint64, id int64, instance string, epoch uint64, reveal bool, point mygo.Point, appearance previewAppearance) {
	p.selectEntry(ctx, sequence, id, protocol.RuntimeState{ServiceInstanceId: instance, PrivacyEpoch: epoch}, reveal, &point, appearance)
}

func (p *Preview) Hide(ctx context.Context, sequence uint64) {
	p.selectEntry(ctx, sequence, 0, protocol.RuntimeState{}, false, nil, previewAppearance{Opacity: 65})
}

func (p *Preview) selectEntry(ctx context.Context, sequence uint64, id int64, state protocol.RuntimeState, reveal bool, point *mygo.Point, appearance previewAppearance) {
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
		go p.load(ctx, main, sequence, id, state, reveal, point, appearance)
	})
}

func (p *Preview) load(ctx context.Context, main *mygo.Window, sequence uint64, id int64, state protocol.RuntimeState, reveal bool, point *mygo.Point, appearance previewAppearance) {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	host, err := p.owner.DesktopConnection(ctx)
	if err != nil {
		return
	}
	defer host.Close()
	size := uint32(560)
	params := protocol.DesktopEntryParams{Id: id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, ThumbnailDimension: &size, Reveal: reveal}
	var snapshot protocol.BodySnapshot
	if host.Call(ctx, protocol.MethodDesktopHistoryGet, params, "BodySnapshot", &snapshot) != nil {
		return
	}
	var row protocol.DesktopEntry
	if host.ReadSnapshot(ctx, snapshot, "DesktopEntry", &row) != nil || row.Id != id {
		return
	}
	var bitmap *ui.Bitmap
	var formatted *richtext.Document
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
	} else if row.ContentType == "rich_text" && row.HtmlContent != nil && len(*row.HtmlContent) <= 64*1024 {
		formatted = richPreviewDocument(*row.HtmlContent)
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
		bounds := main.ContentBounds()
		area := mygo.Screen.DisplayMatching(bounds).WorkArea
		var cursor mygo.Point
		if point == nil {
			cursor = mygo.Screen.CursorScreenPoint()
			area = mygo.Screen.DisplayNearestPoint(cursor).WorkArea
		}
		if area.Width < 1 || area.Height < 1 {
			return
		}
		width, height := min(360, area.Width), min(200, area.Height)
		x, y := cursor.X+16, cursor.Y
		if point != nil {
			x, y = bounds.X+point.X, bounds.Y+point.Y
		} else if x+width > area.X+area.Width {
			x = cursor.X - width - 16
		}
		anchor := mygo.Rectangle{X: max(area.X, min(x, area.X+area.Width-width)) - bounds.X, Y: max(area.Y, min(y, area.Y+area.Height-height)) - bounds.Y}
		theme := appearance.theme()
		p.window = mygo.NewFlyout(mygo.FlyoutOptions{Parent: main, Anchor: anchor, Width: width, Height: height, Hidden: true, Content: ui.View(func(c *ui.Context) {
			c.SetTheme(theme)
			ui.Column(c).Fill().Padding(10).Children(func() {
				ui.Text(c, source)
				if bitmap != nil {
					ui.Image(c, bitmap).Fill()
				} else if formatted != nil {
					richtext.Editor(c, formatted, richtext.EditorOptions{
						ReadOnly: true, Label: "富文本预览", OnLink: func(string) {},
					}).Fill()
				} else {
					ui.Text(c, text).Wrap()
				}
			})
		})})
		p.window.SetTitle("DezirClip 悬浮预览")
		p.window.SetIgnoreMouseEvents(true)
		p.window.ShowInactive()
	})
}

func richPreviewDocument(html string) *richtext.Document {
	doc := richtext.NewDocument(richtext.ParseHTML(html))
	doc.SetSelection(0, 0)
	return doc
}
