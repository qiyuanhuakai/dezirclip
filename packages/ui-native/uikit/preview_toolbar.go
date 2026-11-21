package uikit

import (
	"fmt"
	"math"

	"github.com/egoist/mygo/ui"
)

type PreviewToolbarOptions struct {
	Title, PinLabel, CloseLabel, OpacityLabel string
	DragLabel                                 string
	Pinned, PinPending                        bool
	Opacity                                   *float64
	Image                                     *PreviewImage
	ImageLabels                               ImageZoomLabels
	OnPin, OnClose                            func()
	OnOpacityChange                           func(float64)
}

var (
	previewGrip  = icon("GripHorizontal")
	previewBlend = icon("Blend")
	previewPlus  = icon("Plus")
	previewMinus = icon("Minus")
	previewFit   = icon("Scan")
)

// PreviewToolbar renders window navigation, transparency and optional image zoom.
func (k Kit) PreviewToolbar(c *ui.Context, options PreviewToolbarOptions) ui.Element {
	width, _ := c.Size()
	stackZoom := options.Image != nil && width <= 420
	drawRow := func() {
		title := ui.Row(c).MinWidth(20).Shrink(1).Gap(6).AlignItems(ui.Center).AlignSelf(ui.Stretch).Clip()
		if options.Pinned {
			title.DragWindow().Tooltip(options.DragLabel)
		}
		title.Children(func() {
			style := k.style("previewToolbarTitle")
			ui.Icon(c, previewGrip).Size(14, 14).TextColor(c.Theme().TextMuted).PassThrough()
			text(c, options.Title, style).SingleLine().MinWidth(0).Shrink(1).PassThrough()
		})
		ui.Row(c).Grow(1).MinWidth(0).Gap(6).AlignItems(ui.Center).Justify(ui.End).Children(func() {
			if options.Opacity != nil {
				k.previewOpacity(c, options)
			}
			if options.Image != nil && !stackZoom {
				k.previewZoom(c, options.Image, options.ImageLabels)
			}
		})
		ui.Row(c).Shrink(0).Gap(6).AlignItems(ui.Center).Children(func() {
			label, run := options.PinLabel, options.OnPin
			if options.Pinned {
				label, run = options.CloseLabel, options.OnClose
			}
			if label != "" && k.previewButton(c, label, "", headerIcons["pin"], options.Pinned, options.PinPending).Clicked() && run != nil {
				run()
			}
			if !options.Pinned && options.CloseLabel != "" &&
				k.previewButton(c, options.CloseLabel, "", closeIcon, false, false).Clicked() && options.OnClose != nil {
				options.OnClose()
			}
		})
	}
	var toolbar ui.Element
	if stackZoom {
		toolbar = ui.Column(c).Gap(6)
	} else {
		toolbar = ui.Row(c).Gap(4).AlignItems(ui.Center)
	}
	paint(toolbar.FillWidth().MinWidth(0).Role(ui.RoleToolbar).Label(options.Title), k.style("previewToolbar")).Children(func() {
		if stackZoom {
			ui.Row(c).FillWidth().Gap(4).AlignItems(ui.Center).Children(drawRow)
			ui.Row(c).FillWidth().Justify(ui.End).Children(func() { k.previewZoom(c, options.Image, options.ImageLabels) })
		} else {
			drawRow()
		}
	})
	return toolbar
}

// PreviewAction draws a toolbar icon in compact or floating previews.
func (k Kit) PreviewAction(c *ui.Context, name, label string, active, disabled bool) ui.Element {
	return k.previewButton(c, label, "", headerIcons[name], active, disabled)
}

func (k Kit) previewButton(c *ui.Context, label, value string, symbol *ui.SVG, active, disabled bool) ui.Element {
	style := k.style("previewToolbarButton")
	if active {
		style = k.style("previewToolbarActive")
	}
	if symbol == nil {
		style = k.style("previewToolbarActual")
	}
	button := ui.ButtonBase(c).Label(label).Tooltip(label).MinWidth(style.Width).Height(style.Height).Disabled(disabled).FocusRing(true)
	style = k.interactive(c, button, style, active)
	paint(button, style)
	button.Children(func() {
		if symbol != nil {
			ui.Icon(c, symbol).Size(14, 14).TextColor(styleColor(style.Text)).PassThrough()
		} else {
			text(c, value, style)
		}
	})
	return button
}

func (k Kit) previewOpacity(c *ui.Context, options PreviewToolbarOptions) {
	trackStyle, thumbStyle := k.style("previewOpacityTrack"), k.style("previewOpacityThumb")
	ui.Row(c).Grow(1).MinWidth(90).MaxWidth(220).Gap(6).AlignItems(ui.Center).Children(func() {
		ui.Icon(c, previewBlend).Size(14, 14).TextColor(c.Theme().TextMuted)
		slider := ui.SliderBase(c, options.Opacity, 0, 100).Grow(1).MinWidth(40).MaxWidth(160).
			Height(18).PaddingX(thumbStyle.Width / 2).Step(1).FocusRing(true).Label(options.OpacityLabel).Tooltip(options.OpacityLabel)
		if slider.Changed() {
			*options.Opacity = math.Round(*options.Opacity)
			if options.OnOpacityChange != nil {
				options.OnOpacityChange(*options.Opacity)
			}
		}
		slider.Draw(func(p *ui.Painter, r ui.Rect) {
			track := ui.Rect{X: r.X, Y: r.Y + (r.H-trackStyle.Height)/2, W: r.W, H: trackStyle.Height}
			p.Fill(track, styleColor(trackStyle.Fill), trackStyle.Radius)
			p.Stroke(track, styleColor(trackStyle.Border), trackStyle.Radius, trackStyle.BorderWidth)
			thumb := ui.Rect{X: r.X + max(0, r.W-thumbStyle.Width)*float32(*options.Opacity/100),
				Y: r.Y + (r.H-thumbStyle.Height)/2, W: thumbStyle.Width, H: thumbStyle.Height}
			p.Fill(thumb, styleColor(thumbStyle.Fill), thumbStyle.Radius)
			p.Stroke(thumb, styleColor(thumbStyle.Border), thumbStyle.Radius, thumbStyle.BorderWidth)
		})
		ui.Row(c).Width(32).Shrink(0).Justify(ui.End).Children(func() {
			text(c, fmt.Sprintf("%.0f%%", *options.Opacity), k.style("previewToolbarTitle"))
		})
	})
}

func (k Kit) previewZoom(c *ui.Context, image *PreviewImage, labels ImageZoomLabels) {
	ui.Row(c).Gap(4).AlignItems(ui.Center).Children(func() {
		scale, fitted := image.Zoom()
		scale = min(8, max(0.1, scale))
		if k.previewButton(c, labels.Out, "", previewMinus, false, scale <= 0.1).Clicked() {
			image.SetZoom(scale * 0.8)
		}
		value := labels.FitLabel
		if !fitted {
			value = fmt.Sprintf("%.0f%%", float64(scale)*100)
		}
		ui.Row(c).Width(35).Center().Children(func() {
			text(c, value, k.style("previewToolbarTitle"))
		})
		if k.previewButton(c, labels.In, "", previewPlus, false, scale >= 8).Clicked() {
			image.SetZoom(scale * 1.25)
		}
		if k.previewButton(c, labels.Fit, "", previewFit, fitted, false).Clicked() {
			image.SetZoom(0)
		}
		if k.previewButton(c, labels.Actual, "100%", nil, false, false).Clicked() {
			image.SetZoom(1)
		}
	})
}
