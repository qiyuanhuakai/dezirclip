package uikit

import (
	"fmt"
	"math"

	"github.com/egoist/mygo/ui"
)

type PreviewToolbarOptions struct {
	Title, PinLabel, CloseLabel, OpacityLabel string
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
	toolbar := paint(ui.Row(c).FillWidth().Gap(4).AlignItems(ui.Center), k.style("previewToolbar"))
	toolbar.Children(func() {
		title := ui.Row(c).Gap(6).AlignItems(ui.Center)
		if options.Pinned {
			title.DragWindow()
		}
		title.Children(func() {
			style := k.style("previewToolbarTitle")
			ui.Icon(c, previewGrip).Size(14, 14).TextColor(c.Theme().TextMuted).PassThrough()
			text(c, options.Title, style).SingleLine().PassThrough()
		})
		ui.Row(c).Grow(1).Gap(6).AlignItems(ui.Center).Justify(ui.End).Wrap().Children(func() {
			if options.Opacity != nil {
				k.previewOpacity(c, options)
			}
			if options.Image != nil {
				k.previewZoom(c, options.Image, options.ImageLabels)
			}
		})
		ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
			label, run := options.PinLabel, options.OnPin
			if options.Pinned {
				label, run = options.CloseLabel, options.OnClose
			}
			if k.previewButton(c, label, "", headerIcons["pin"], options.Pinned, options.PinPending).Clicked() && run != nil {
				run()
			}
			if !options.Pinned && k.previewButton(c, options.CloseLabel, "", closeIcon, false, false).Clicked() && options.OnClose != nil {
				options.OnClose()
			}
		})
	})
	return toolbar
}

func (k Kit) previewButton(c *ui.Context, label, value string, symbol *ui.SVG, active, disabled bool) ui.Element {
	style := k.style("previewToolbarButton")
	if active {
		style = k.style("previewToolbarActive")
	}
	if symbol == nil {
		style = k.style("previewToolbarActual")
	}
	button := ui.ButtonBase(c).Label(label).Tooltip(label).MinWidth(style.Width).Height(style.Height).Disabled(disabled)
	style = k.interactive(c, button, style, active)
	paint(button, style)
	button.Children(func() {
		if symbol != nil {
			ui.Icon(c, symbol).Size(14, 14).TextColor(styleColor(style.Text))
		} else {
			text(c, value, style)
		}
	})
	return button
}

func (k Kit) previewOpacity(c *ui.Context, options PreviewToolbarOptions) {
	ui.Row(c).Grow(1).MinWidth(90).MaxWidth(220).Gap(6).AlignItems(ui.Center).Children(func() {
		ui.Icon(c, previewBlend).Size(14, 14).TextColor(c.Theme().TextMuted)
		slider := ui.SliderBase(c, options.Opacity, 0, 100).Grow(1).MinWidth(40).MaxWidth(160).
			Height(18).PaddingX(6).Label(options.OpacityLabel).Tooltip(options.OpacityLabel)
		if slider.Changed() {
			*options.Opacity = math.Round(*options.Opacity)
			if options.OnOpacityChange != nil {
				options.OnOpacityChange(*options.Opacity)
			}
		}
		slider.Draw(func(p *ui.Painter, r ui.Rect) {
			palette := c.Theme()
			track := ui.Rect{X: r.X + 6, Y: r.Y + (r.H-6)/2, W: max(0, r.W-12), H: 6}
			p.Fill(track, palette.Background, 4)
			p.Stroke(track, palette.Border, 4, 1)
			thumb := ui.Rect{X: track.X + track.W*float32(*options.Opacity/100) - 7, Y: r.Y + (r.H-14)/2, W: 14, H: 14}
			p.Fill(thumb, palette.Surface, 4)
			p.Stroke(thumb, palette.Border, 4, 2)
		})
		text(c, fmt.Sprintf("%.0f%%", *options.Opacity), k.style("previewToolbarTitle")).Width(32)
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
