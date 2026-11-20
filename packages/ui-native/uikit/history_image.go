package uikit

import (
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

// HistoryImageOptions describes the visible state of a thumbnail in a history card.
type HistoryImageOptions struct {
	Source                      ui.ImageSource
	Width                       float32
	Label, Unavailable, Missing string
	Compact                     bool
}

var historyImageOff = icon("ImageOff")

func (k Kit) HistoryImage(c *ui.Context, options HistoryImageOptions) {
	if options.Width <= 0 {
		options.Width, _ = c.Size()
	}
	widget := "attachmentImage"
	if options.Compact {
		widget += "Compact"
	}
	switch {
	case options.Missing != "":
		style := k.style(widget + "Missing")
		k.historyImagePlaceholder(c, style, options.Width, options.Missing, true)
	case options.Source != nil:
		style := k.style(widget)
		if style.MaxHeight <= 0 {
			style.MaxHeight = 100
			if options.Compact {
				style.MaxHeight = 44
			}
		}
		width, height := historyImageSize(options.Source, options.Width, style)
		paint(ui.Image(c, options.Source).Size(width, height).Fit(ui.Contain).Label(options.Label), style)
	case options.Unavailable != "":
		style := k.style(widget + "Unavailable")
		k.historyImagePlaceholder(c, style, options.Width, options.Unavailable, false)
	default:
		style := k.style(widget + "Pending")
		paint(ui.Box(c).Size(min(options.Width, style.Width), style.Height), style).PassThrough()
	}
}

func horizontalBorders(style theme.WidgetStyle) float32 {
	if style.BorderWidths != nil {
		return style.BorderWidths[1] + style.BorderWidths[3]
	}
	return 2 * style.BorderWidth
}

func historyImageSize(source ui.ImageSource, available float32, style theme.WidgetStyle) (float32, float32) {
	var width, height float32
	switch src := source.(type) {
	case *ui.Bitmap:
		w, h := src.Size()
		width, height = float32(w), float32(h)
	case *ui.SVG:
		width, height = src.Size()
	}
	borderWidth, borderHeight := horizontalBorders(style), 2*style.BorderWidth
	if widths := style.BorderWidths; widths != nil {
		borderHeight = widths[0] + widths[2]
	}
	innerWidth := max(1, available-borderWidth)
	innerHeight := max(1, style.MaxHeight-borderHeight)
	ratio := min(1, innerWidth/max(1, width), innerHeight/max(1, height))
	minimum := min(available, style.MinWidth)
	if width*ratio+borderWidth < minimum {
		ratio = min((minimum-borderWidth)/max(1, width), innerHeight/max(1, height))
	}
	return min(available, max(minimum, width*ratio+borderWidth)), height*ratio + borderHeight
}

func (k Kit) historyImagePlaceholder(c *ui.Context, style theme.WidgetStyle, width float32, label string, missing bool) {
	box := ui.Row(c).Width(width).MinHeight(style.MinHeight).AlignItems(ui.Center).Justify(ui.Center).Gap(style.ColumnGap)
	iconSize := float32(20)
	if missing {
		box.Column().Gap(style.RowGap).Height(style.Height)
		iconSize = 24
	}
	paint(box, style).Children(func() {
		ui.Icon(c, historyImageOff).Size(iconSize, iconSize).TextColor(styleColor(style.Text))
		text(c, label, style).Wrap().TextAlign(ui.Center)
	})
}
