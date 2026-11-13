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
	surfaceTheme "github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/theme"
)

// The WebView supplies only a row ID. The preview reads its own privacy-bound
// snapshot and never sends full content or image bytes through JavaScript.
type Preview struct {
	owner        *client.Client
	resident     *residentBridge
	main, window *mygo.Window
	generation   uint64
	cancel       context.CancelFunc
	opacity      *previewOpacityControl
}

type previewPlacement struct {
	Point  mygo.Point
	Source mygo.Rectangle
}

// Keep the flyout outside the triggering row, including when its full size cannot fit beside the main window.
func previewBounds(area, source mygo.Rectangle, preferred mygo.Point) mygo.Rectangle {
	width, height := min(480, area.Width), min(320, area.Height)
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
	return previewLayout{bounds, area, min(480, area.Width), min(320, area.Height)}, nil
}

func (p *Preview) clear() {
	p.opacity = nil
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
	Palette                                                   *surfaceTheme.Appearance
	Title, Pin, Close, Retry, Concealed, Failed, OpacityLabel string
	ZoomIn, ZoomOut, Fit, FitLabel, Actual, ImageZoom         string
	Opacity                                                   int
	Dark                                                      bool
	Background                                                *struct{ R, G, B uint8 }
}

func (a previewAppearance) theme() *ui.Theme {
	theme := ui.LightTheme()
	if a.Dark {
		theme = ui.DarkTheme()
	}
	if a.Palette != nil && a.Palette.Valid() {
		surfaceTheme.Apply(theme, a.Palette.Tokens)
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

func (a previewAppearance) panel(c *ui.Context) ui.Element {
	theme := a.theme()
	background := theme.Background
	// Tooltip defaults otherwise inherit the transparent root as their text color.
	theme.Inverse, theme.InverseText = theme.Text, background
	theme.Inverse.A, theme.InverseText.A = 255, 255
	theme.Background = background.Alpha(0)
	c.SetTheme(theme)
	return ui.Column(c).Fill().Background(background).Radius(max(12, theme.Radius)).Border(1, theme.Border.Alpha(.6)).Padding(10).Gap(6)
}

func previewIcon(c *ui.Context, label string, pin bool) ui.Element {
	t := c.Theme()
	return ui.ButtonBase(c).Size(26, 26).Radius(8).Border(1, t.Text.Alpha(.2)).Background(t.Surface.Alpha(.65)).Label(label).Tooltip(label).Draw(func(p *ui.Painter, r ui.Rect) {
		x, y := r.X+13, r.Y+13
		if pin {
			var outline ui.Path
			outline.MoveTo(x-4, y-7)
			outline.LineTo(x+4, y-7)
			outline.LineTo(x+4, y-5)
			outline.LineTo(x+2, y-4)
			outline.LineTo(x+2, y)
			outline.LineTo(x+5, y+2)
			outline.LineTo(x+5, y+4)
			outline.LineTo(x-5, y+4)
			outline.LineTo(x-5, y+2)
			outline.LineTo(x-2, y)
			outline.LineTo(x-2, y-4)
			outline.LineTo(x-4, y-5)
			outline.Close()
			p.StrokePath(&outline, 1.4, t.Text)
			p.Line(x, y+4, x, y+8, 1.4, t.Text)
		} else {
			var cross ui.Path
			cross.MoveTo(x-4, y-4)
			cross.LineTo(x+4, y+4)
			cross.MoveTo(x+4, y-4)
			cross.LineTo(x-4, y+4)
			p.StrokePath(&cross, 1.5, t.Text)
		}
	})
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
	sourceIcon   *ui.Bitmap
	formatted    *richtext.Document
	kind         string
	timestamp    int64
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
	} else if (row.ContentType == "rich_text" || row.ContentType == "file" || row.ContentType == "files" || row.ContentType == "video") && row.HtmlContent != nil && len(*row.HtmlContent) <= 2*1024*1024 {
		html := strings.NewReplacer("<figure>", "<div>", "</figure>", "</div>", "<figcaption>", "<p>", "</figcaption>", "</p>").Replace(*row.HtmlContent)
		formatted = previewDocument(richtext.ParseHTML(html))
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
	if bitmap == nil && formatted == nil {
		formatted = previewDocument(richtext.NewText(text))
	}
	var sourceIcon *ui.Bitmap
	if !params.Reveal && row.SourceApp != "" {
		iconParams := params
		iconParams.ThumbnailDimension = nil
		var iconSnapshot protocol.BodySnapshot
		if host.Call(ctx, protocol.MethodDesktopHistorySourceIcon, iconParams, "BodySnapshot", &iconSnapshot) == nil {
			var icon protocol.SourceAppIcon
			if host.ReadSnapshot(ctx, iconSnapshot, "SourceAppIcon", &icon) == nil && icon.Src != nil {
				sourceIcon = previewSourceBitmap(*icon.Src)
			}
		}
	}
	var latest protocol.RuntimeState
	if host.Call(ctx, protocol.MethodHealth, struct{}{}, "RuntimeState", &latest) != nil || latest.PrivacyEpoch != snapshot.State.PrivacyEpoch || latest.ServiceInstanceId != snapshot.State.ServiceInstanceId {
		return nil, snapshot.State, client.ErrProtocol
	}
	return &previewBody{sourceIcon: sourceIcon, source: row.SourceApp, text: text, bitmap: bitmap, formatted: formatted, kind: row.ContentType, timestamp: row.Timestamp}, snapshot.State, nil
}

func (body *previewBody) metadata(c *ui.Context) {
	ui.Row(c).FillWidth().Gap(5).Children(func() {
		previewMetadataIcon(c, body.kind)
		ui.Text(c, body.kind).SingleLine().FontSize(11).TextColor(c.Theme().TextMuted)
		if body.source != "" {
			if body.sourceIcon != nil {
				ui.Image(c, body.sourceIcon).Size(14, 14).Fit(ui.Contain)
			} else {
				previewMetadataIcon(c, "application")
			}
			ui.Text(c, body.source).SingleLine().FontSize(11).TextColor(c.Theme().TextMuted).Grow(1)
		}
		if body.timestamp > 0 {
			previewMetadataIcon(c, "clock")
			ui.Text(c, time.UnixMilli(body.timestamp).Format("01-02 15:04")).SingleLine().FontSize(11).TextColor(c.Theme().TextMuted)
		}
	})
}

func (body *previewBody) draw(c *ui.Context) {
	if body.bitmap != nil {
		ui.Image(c, body.bitmap).Fill().Fit(ui.Contain)
	} else if body.formatted != nil {
		drawRichPreview(c, body.formatted)
	} else {
		ui.Text(c, body.text).Wrap()
	}
}

func (p *Preview) ReloadOpacity(ctx context.Context) {
	main := mygo.CallerWindow(ctx)
	mygo.RunOnMain(func() {
		if main != nil && main == p.main && p.opacity != nil {
			p.opacity.reload()
		}
	})
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

func (p *Preview) IsAlwaysOnTop(ctx context.Context) bool {
	main := mygo.CallerWindow(ctx)
	onTop := false
	mygo.RunOnMain(func() {
		if main != nil && p.main == main && p.window != nil && !p.window.IsDestroyed() {
			onTop = p.window.IsVisible() && p.window.IsAlwaysOnTop()
		}
	})
	return onTop
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
		opacity := &previewOpacityControl{value: appearance.Opacity, slider: float64(appearance.Opacity), committed: appearance.Opacity, epoch: params.ExpectedEpoch, library: Library{p.owner}}
		pinning, failed := false, false
		p.window = mygo.NewFlyout(mygo.FlyoutOptions{Parent: main, Anchor: anchor, Width: frame.Width, Height: frame.Height, Hidden: true, Content: ui.View(func(c *ui.Context) {
			appearance.Opacity = opacity.value
			appearance.panel(c).Children(func() {
				ui.Row(c).FillWidth().Height(30).Gap(6).Children(func() {
					title := appearance.Title
					if title == "" {
						title = "DezirClip"
					}
					ui.Text(c, title).Grow(1).FontSize(13).SingleLine()
					opacity.draw(c, appearance)
					if appearance.Close != "" && previewIcon(c, appearance.Close, false).Disabled(pinning).Clicked() {
						main.Update(func() {
							if p.generation == sequence {
								p.clear()
							}
						})
					}
					if appearance.Pin != "" && previewIcon(c, appearance.Pin, true).Disabled(pinning).Clicked() {
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
				body.metadata(c)
				if failed {
					ui.Text(c, appearance.Failed)
				}
				body.draw(c)
			})
		})})
		opacity.window = p.window
		p.opacity = opacity
		opacity.reload()
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
