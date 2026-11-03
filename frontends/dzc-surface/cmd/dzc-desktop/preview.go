package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"image"
	"math"
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
	resident     *residentBridge
	main, window *mygo.Window
	generation   uint64
	cancel       context.CancelFunc
}

type previewPlacement struct {
	Point  mygo.Point
	Source mygo.Rectangle
}

// Keep the flyout outside the triggering row, including when its full size cannot fit beside the main window.
func previewBounds(area, source mygo.Rectangle, preferred mygo.Point) mygo.Rectangle {
	width, height := min(360, area.Width), min(200, area.Height)
	if width <= 0 || height <= 0 {
		return mygo.Rectangle{}
	}
	x1, y1 := area.X, area.Y
	x2, y2 := x1+area.Width, y1+area.Height
	const gap = 4
	// Each available strip excludes the entire source row on one axis.
	strips := [][4]int{
		{x1, y1, min(x2, source.X-gap), y2}, {max(x1, source.X+source.Width+gap), y1, x2, y2},
		{x1, y1, x2, min(y2, source.Y-gap)}, {x1, max(y1, source.Y+source.Height+gap), x2, y2},
	}
	var best mygo.Rectangle
	var distance int64
	for _, s := range strips {
		w, h := min(width, s[2]-s[0]), min(height, s[3]-s[1])
		if w <= 0 || h <= 0 {
			continue
		}
		x, y := max(s[0], min(preferred.X, s[2]-w)), max(s[1], min(preferred.Y, s[3]-h))
		dx, dy := int64(x-preferred.X), int64(y-preferred.Y)
		d := dx*dx + dy*dy
		if w*h > best.Width*best.Height || (w*h == best.Width*best.Height && d < distance) {
			best = mygo.Rectangle{X: x, Y: y, Width: w, Height: h}
			distance = d
		}
	}
	return best
}

type previewLayout struct {
	Main     mygo.Rectangle
	WorkArea mygo.Rectangle
	Width    int
	Height   int
}

// Convert a physical monitor work area into the coordinate system of the parent.
// Leave two physical pixels for the native flyout's anchor and DPI rounding.
func previewParentArea(parent mygo.Rectangle, origin mygo.Point, work mygo.Rectangle, scale float64) mygo.Rectangle {
	if scale <= 0 {
		return mygo.Rectangle{}
	}
	left := int(math.Ceil(float64(work.X-origin.X+2) / scale))
	top := int(math.Ceil(float64(work.Y-origin.Y+2) / scale))
	right := int(math.Floor(float64(work.X+work.Width-origin.X-2) / scale))
	bottom := int(math.Floor(float64(work.Y+work.Height-origin.Y-2) / scale))
	return mygo.Rectangle{X: parent.X + left, Y: parent.Y + top, Width: right - left, Height: bottom - top}
}

func (*Preview) Layout(ctx context.Context) (previewLayout, error) {
	main := mygo.CallerWindow(ctx)
	if main == nil {
		return previewLayout{}, errors.New("WindowUnavailable")
	}
	bounds := main.ContentBounds()
	area, _ := previewDisplay(main, bounds)
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
	Pin, Close, Retry, Concealed, Failed              string
	ZoomIn, ZoomOut, Fit, FitLabel, Actual, ImageZoom string
	Opacity                                           int
	Dark                                              bool
	Background                                        *struct{ R, G, B uint8 }
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

func (p *Preview) ShowEntry(ctx context.Context, sequence uint64, id int64, instance string, epoch uint64, reveal bool, placement previewPlacement, appearance previewAppearance) {
	p.selectEntry(ctx, sequence, id, protocol.RuntimeState{ServiceInstanceId: instance, PrivacyEpoch: epoch}, reveal, &placement, appearance)
}

func (p *Preview) Hide(ctx context.Context, sequence uint64) {
	p.selectEntry(ctx, sequence, 0, protocol.RuntimeState{}, false, nil, previewAppearance{Opacity: 65})
}

func (p *Preview) selectEntry(ctx context.Context, sequence uint64, id int64, state protocol.RuntimeState, reveal bool, placement *previewPlacement, appearance previewAppearance) {
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
		go p.load(ctx, main, sequence, id, state, reveal, placement, appearance)
	})
}

type previewBody struct {
	source, text string
	bitmap       *ui.Bitmap
	formatted    *richtext.Document
}

func readPreview(ctx context.Context, host *client.Client, params protocol.DesktopEntryParams, full bool) (*previewBody, protocol.RuntimeState, error) {
	size := uint32(560)
	if !full {
		params.ThumbnailDimension = &size
	}
	var snapshot protocol.BodySnapshot
	if err := host.Call(ctx, protocol.MethodDesktopHistoryGet, params, "BodySnapshot", &snapshot); err != nil {
		return nil, snapshot.State, err
	}
	var row protocol.DesktopEntry
	if err := host.ReadSnapshot(ctx, snapshot, "DesktopEntry", &row); err != nil {
		return nil, snapshot.State, err
	}
	if row.Id != params.Id {
		return nil, snapshot.State, client.ErrProtocol
	}
	var bitmap *ui.Bitmap
	var formatted *richtext.Document
	text := row.Content
	if row.ContentType == "image" {
		header, encoded, ok := strings.Cut(text, ",")
		if !ok || !strings.HasPrefix(header, "data:image/") || !strings.HasSuffix(header, ";base64") {
			return nil, snapshot.State, client.ErrProtocol
		}
		raw, err := base64.StdEncoding.DecodeString(encoded)
		if err != nil {
			return nil, snapshot.State, client.ErrProtocol
		}
		config, _, err := image.DecodeConfig(bytes.NewReader(raw))
		if err != nil || config.Height <= 0 || config.Width <= 0 || config.Width > 64*1024*1024/config.Height {
			clear(raw)
			return nil, snapshot.State, client.ErrProtocol
		}
		bitmap, err = ui.DecodeBitmap(raw)
		clear(raw)
		if err != nil {
			return nil, snapshot.State, client.ErrProtocol
		}
		text = ""
	} else if row.ContentType == "rich_text" && row.HtmlContent != nil && len(*row.HtmlContent) <= 64*1024 {
		formatted = previewDocument(richtext.ParseHTML(*row.HtmlContent))
	} else {
		if text == "" {
			text = row.Preview
		}
		// Bound shaping and layout work for long clipboard entries.
		if !full {
			for index := range text {
				if index >= 4096 {
					text = strings.Clone(text[:index]) + "…"
					break
				}
			}
		}
	}
	if full && bitmap == nil && formatted == nil {
		formatted = previewDocument(richtext.NewText(text))
	}
	return &previewBody{row.SourceApp, text, bitmap, formatted}, snapshot.State, nil
}

func (body *previewBody) draw(c *ui.Context) {
	if body.bitmap != nil {
		ui.Image(c, body.bitmap).Fill()
	} else if body.formatted != nil {
		drawRichPreview(c, body.formatted)
	} else {
		ui.Text(c, body.text).Wrap()
	}
}

func (p *Preview) PointerInside(ctx context.Context, sequence uint64) bool {
	main := mygo.CallerWindow(ctx)
	inside := false
	mygo.RunOnMain(func() {
		if p.main != main || p.generation != sequence || p.window == nil {
			return
		}
		b, pt, ok := previewPointer(p.window)
		if !ok {
			return
		}
		inside = pt.X >= b.X && pt.X < b.X+b.Width && pt.Y >= b.Y && pt.Y < b.Y+b.Height
	})
	return inside
}

func (p *Preview) load(ctx context.Context, main *mygo.Window, sequence uint64, id int64, state protocol.RuntimeState, reveal bool, placement *previewPlacement, appearance previewAppearance) {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	host, err := p.owner.DesktopConnection(ctx)
	if err != nil {
		return
	}
	defer host.Close()
	params := protocol.DesktopEntryParams{Id: id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: reveal}
	body, _, err := readPreview(ctx, host, params, false)
	if err != nil {
		return
	}
	main.Update(func() {
		if main != p.main || p.generation != sequence || p.cancel == nil || !main.IsFocused() {
			return
		}
		bounds := main.ContentBounds()
		area, cursor := previewDisplay(main, bounds)
		var preferred mygo.Point
		var source mygo.Rectangle
		if placement == nil {
			source = mygo.Rectangle{X: cursor.X, Y: cursor.Y, Width: 1, Height: 1}
			preferred = mygo.Point{X: cursor.X + 16, Y: cursor.Y}
		} else {
			preferred = mygo.Point{X: bounds.X + placement.Point.X, Y: bounds.Y + placement.Point.Y}
			source = placement.Source
			source.X += bounds.X
			source.Y += bounds.Y
		}
		frame := previewBounds(area, source, preferred)
		if frame.Width == 0 {
			return
		}
		anchor := mygo.Rectangle{X: frame.X - bounds.X, Y: frame.Y - bounds.Y}
		theme := appearance.theme()
		pinning, failed := false, false
		p.window = mygo.NewFlyout(mygo.FlyoutOptions{Parent: main, Anchor: anchor, Width: frame.Width, Height: frame.Height, Hidden: true, Content: ui.View(func(c *ui.Context) {
			c.SetTheme(theme)
			ui.Column(c).Fill().Padding(10).Children(func() {
				ui.Row(c).FillWidth().Children(func() {
					ui.Text(c, body.source).Grow(1)
					if appearance.Close != "" && ui.Button(c, appearance.Close).Disabled(pinning).Clicked() {
						main.Update(func() {
							if p.generation == sequence {
								p.clear()
							}
						})
					}
					if appearance.Pin != "" && ui.Button(c, appearance.Pin).Disabled(pinning).Clicked() {
						pinning = true
						ticket := pinPreviewRequest{Params: params, Appearance: appearance, Bounds: p.window.Bounds()}
						go func() {
							err := p.resident.send(context.Background(), residentRequest{Operation: "pin-preview", Preview: &ticket}, 10*time.Second)
							main.Update(func() {
								if p.generation == sequence && p.window != nil {
									if err == nil {
										p.clear()
									} else {
										pinning = false
										failed = true
										p.window.Update(func() {})
									}
								}
							})
						}()
					}
				})
				if failed {
					ui.Text(c, appearance.Failed)
				}
				body.draw(c)
			})
		})})
		p.window.SetTitle("DezirClip 悬浮预览")
		p.window.ShowInactive()
	})
}

func previewDocument(contents richtext.Text) *richtext.Document {
	doc := richtext.NewDocument(contents)
	doc.SetSelection(0, 0)
	return doc
}

func drawRichPreview(c *ui.Context, document *richtext.Document) {
	richtext.EditorBase(c, document, richtext.EditorOptions{
		ReadOnly: true, Label: "富文本预览", OnLink: func(string) {},
	}).Fill()
}
