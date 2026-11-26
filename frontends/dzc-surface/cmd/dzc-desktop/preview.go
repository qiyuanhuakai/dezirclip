package main

import (
	"bytes"
	"context"
	"errors"
	"image"
	"math"
	"strings"
	"time"

	"github.com/MoeForge/moego/base64"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	surfaceTheme "github.com/qiyuanhuakai/deziro-ui/packages/ui-native/theme"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-presets"
)

// The WebView supplies only a row ID. The preview reads its own privacy-bound
// snapshot and never sends full content or image bytes through JavaScript.
type Preview struct {
	owner        *client.Client
	resident     *residentBridge
	main, window *mygo.Window
	generation   uint64
	entryID      int64
	cancel       context.CancelFunc
	opacity      *previewOpacityControl
}

type previewPlacement struct {
	Point  mygo.Point
	Source mygo.Rectangle
}

// Keep the flyout outside the triggering row, including when its full size cannot fit beside the main window.
func previewBounds(area, source mygo.Rectangle, preferred mygo.Point) mygo.Rectangle {
	return previewSizedBounds(area, source, preferred, 480, 320)
}

func previewSizedBounds(area, source mygo.Rectangle, preferred mygo.Point, desiredWidth, desiredHeight int) mygo.Rectangle {
	width, height := min(desiredWidth, area.Width), min(desiredHeight, area.Height)
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
	var bestBeside bool
	var distance int64
	for side, s := range strips {
		w, h := min(width, s[2]-s[0]), min(height, s[3]-s[1])
		if w <= 0 || h <= 0 {
			continue
		}
		x := max(s[0], min(source.X+(source.Width-w)/2, s[2]-w))
		y := max(s[1], min(preferred.Y, s[3]-h))
		switch side {
		case 0: // Left: keep the adjoining edges close.
			x = s[2] - w
		case 1:
			x = s[0]
		case 2:
			y = s[3] - h
		case 3:
			y = s[1]
		}
		dx, dy := int64(x-preferred.X), int64(y-preferred.Y)
		d := dx*dx + dy*dy
		beside := side < 2
		sameArea := w*h == best.Width*best.Height
		betterAlignment := beside && !bestBeside || beside == bestBeside && d < distance
		if w*h > best.Width*best.Height || sameArea && betterAlignment {
			best = mygo.Rectangle{X: x, Y: y, Width: w, Height: h}
			bestBeside, distance = beside, d
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
	p.entryID = 0
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
	Language, BaseTheme                                             string
	Palette                                                         *surfaceTheme.Appearance
	Title, Pin, Close, Drag, Retry, Concealed, Failed, OpacityLabel string
	ZoomIn, ZoomOut, Fit, FitLabel, Actual, ImageZoom               string
	Opacity                                                         int
	Dark                                                            bool
	Background                                                      *struct{ R, G, B uint8 }
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
	return a.kit().FloatingPanel(c, background, 10, 6)
}

func (a previewAppearance) kit() uikit.Kit {
	if a.BaseTheme != "" {
		mode := "light"
		if a.Dark {
			mode = "dark"
		}
		kit := presets.Select(a.BaseTheme, mode).Kit()
		if a.Palette != nil && a.Palette.Valid() && len(a.Palette.Tokens.FontFamilies) > 0 {
			kit = kit.WithInterfaceFont(a.Palette.Tokens.FontFamilies[0])
		}
		return kit
	}
	if a.Palette != nil && a.Palette.Valid() {
		return uikit.NewKit(a.Palette.Mode, a.Palette.Tokens, nil)
	}
	if a.Dark {
		return uikit.Kit{Mode: "dark"}
	}
	return uikit.Kit{Mode: "light"}
}

func (a previewAppearance) toolbarOptions(pinned bool) uikit.PreviewToolbarOptions {
	title := a.Title
	if title == "" {
		title = "DezirClip"
	}
	return uikit.PreviewToolbarOptions{
		Title: title, PinLabel: a.Pin, CloseLabel: a.Close, DragLabel: a.Drag,
		OpacityLabel: a.OpacityLabel, Pinned: pinned,
		ImageLabels: uikit.ImageZoomLabels{In: a.ZoomIn, Out: a.ZoomOut, Actual: a.Actual, Fit: a.Fit, FitLabel: a.FitLabel},
	}
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
		p.present(main, sequence, id, state, reveal, placement, appearance)
	})
}

// present is also used by the native main window. The caller is on the UI thread.
func (p *Preview) present(main *mygo.Window, sequence uint64, id int64, state protocol.RuntimeState, reveal bool, placement *previewPlacement, appearance previewAppearance) {
	if main != p.main || sequence <= p.generation {
		return
	}
	p.generation = sequence
	p.clear()
	if id == 0 || !main.IsVisible() || main.IsMinimized() || reveal && !main.IsFocused() {
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	p.cancel, p.entryID = cancel, id
	go p.load(ctx, main, sequence, id, state, reveal, placement, appearance)
}

type previewBody struct {
	source, text string
	bitmap       *ui.Bitmap
	sourceIcon   ui.ImageSource
	formatted    *richtext.Document
	attachments  []uikit.PreviewAttachment
	kind         string
	timestamp    int64
}

func readPreview(ctx context.Context, host *client.Client, params protocol.DesktopEntryParams, full bool) (*previewBody, protocol.RuntimeState, error) {
	includeVideo := true
	omitOCR := true
	params.IncludeVideoFrame = &includeVideo
	params.OmitOcrText = &omitOCR
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
	var attachments []uikit.PreviewAttachment
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
	} else if row.ContentType == "file" || row.ContentType == "files" || row.ContentType == "video" {
		markup := ""
		if row.HtmlContent != nil {
			markup = *row.HtmlContent
		}
		attachments = uikit.ParsePreviewAttachments(markup, row.Preview)
	} else if row.ContentType == "rich_text" && row.HtmlContent != nil && len(*row.HtmlContent) <= 2*1024*1024 {
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
	if bitmap == nil && formatted == nil && len(attachments) == 0 {
		formatted = previewDocument(richtext.NewText(text))
	}
	var sourceIcon ui.ImageSource
	if !params.Reveal && row.SourceApp != "" {
		iconParams := params
		iconParams.ThumbnailDimension = nil
		iconParams.IncludeVideoFrame = nil
		var iconSnapshot protocol.BodySnapshot
		if host.Call(ctx, protocol.MethodDesktopHistorySourceIcon, iconParams, "BodySnapshot", &iconSnapshot) == nil {
			var icon protocol.SourceAppIcon
			if host.ReadSnapshot(ctx, iconSnapshot, "SourceAppIcon", &icon) == nil && icon.Src != nil {
				sourceIcon = previewSourceImage(*icon.Src)
			}
		}
	}
	var latest protocol.RuntimeState
	if host.Call(ctx, protocol.MethodHealth, struct{}{}, "RuntimeState", &latest) != nil || latest.PrivacyEpoch != snapshot.State.PrivacyEpoch || latest.ServiceInstanceId != snapshot.State.ServiceInstanceId {
		return nil, snapshot.State, client.ErrProtocol
	}
	return &previewBody{attachments: attachments, sourceIcon: sourceIcon, source: row.SourceApp, text: text, bitmap: bitmap, formatted: formatted, kind: row.ContentType, timestamp: row.Timestamp}, snapshot.State, nil
}

func (body *previewBody) metadata(c *ui.Context, language string) {
	ui.Row(c).FillWidth().Gap(5).Children(func() {
		uikit.PreviewMetadataIcon(c, body.kind)
		ui.Text(c, body.kind).SingleLine().FontSize(11).TextColor(c.Theme().TextMuted)
		if body.source != "" {
			if body.sourceIcon != nil {
				ui.Image(c, body.sourceIcon).Size(14, 14).Fit(ui.Contain)
			} else {
				uikit.PreviewMetadataIcon(c, "application")
			}
			ui.Text(c, body.source).SingleLine().FontSize(11).TextColor(c.Theme().TextMuted).Grow(1)
		}
		if body.timestamp > 0 {
			uikit.PreviewMetadataIcon(c, "clock")
			ui.Text(c, concisePreviewTime(body.timestamp, time.Now(), language)).SingleLine().FontSize(11).TextColor(c.Theme().TextMuted)
		}
	})
}

func (body *previewBody) draw(c *ui.Context) {
	if bitmap := body.mediaBitmap(); bitmap != nil {
		ui.Image(c, bitmap).Fill().Fit(ui.Contain)
	} else if len(body.attachments) > 0 {
		uikit.AttachmentPreview(c, body.attachments)
	} else if body.formatted != nil {
		drawRichPreview(c, body.formatted)
	} else {
		uikit.PreviewText(c, body.text)
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
		if main != p.main || p.generation != sequence || p.cancel == nil || !main.IsVisible() || main.IsMinimized() || reveal && !main.IsFocused() {
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
		// Fit the content before mapping the flyout, so its controls do not move under the pointer.
		theme := appearance.theme()
		width, height := body.hoverSize(area, theme)
		frame := previewSizedBounds(area, source, preferred, width, height)
		if frame.Width > 0 && frame.Width < width && body.formatted != nil {
			_, height = body.textSize(theme, frame.Width, min(area.Height, 560))
			frame = previewSizedBounds(area, source, preferred, frame.Width, height)
		}
		if frame.Width == 0 {
			return
		}
		anchor := mygo.Rectangle{X: frame.X - bounds.X, Y: frame.Y - bounds.Y}
		opacity := &previewOpacityControl{value: appearance.Opacity, slider: float64(appearance.Opacity), committed: appearance.Opacity, epoch: params.ExpectedEpoch, library: Library{p.owner}}
		pinning, failed := false, false
		p.window = mygo.NewFlyout(mygo.FlyoutOptions{Parent: main, Anchor: anchor, Width: frame.Width, Height: frame.Height, Hidden: true, Content: ui.View(func(c *ui.Context) {
			appearance.Opacity = opacity.value
			appearance.panel(c).Children(func() {
				options := appearance.toolbarOptions(false)
				options.PinPending = pinning
				options.OnClose = func() {
					if !pinning {
						main.Update(func() {
							if p.generation == sequence {
								p.clear()
							}
						})
					}
				}
				options.OnPin = func() {
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
				opacity.drawToolbar(c, appearance, options)
				body.metadata(c, appearance.Language)
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
