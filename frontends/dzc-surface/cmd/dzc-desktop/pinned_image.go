package main

import (
	"fmt"
	"math"

	"github.com/egoist/mygo/ui"
)

// Pan is the displacement from the centered image, in viewport DIPs.
// A zero zoom tracks the viewport as the pinned window is resized.
type pinnedImageView struct {
	zoom, fit     float32
	width, height float32
	pan, grab     ui.Point
	dragging      bool
}

func (v *pinnedImageView) factor() float32 {
	if v.zoom > 0 {
		return v.zoom
	}
	return v.fit
}

func (v *pinnedImageView) zoomAt(factor, x, y float32) {
	before := v.factor()
	if before <= 0 {
		return
	}
	v.zoom = max(0.1, min(8, factor))
	ratio := v.zoom / before
	v.pan.X = (x-v.width/2)*(1-ratio) + v.pan.X*ratio
	v.pan.Y = (y-v.height/2)*(1-ratio) + v.pan.Y*ratio
}

func pinPan(value, view, size float32) float32 {
	limit := (size - view) / 2
	if limit < 0 {
		limit = (view+size)/2 - min(24, view, size)
	}
	return max(-limit, min(limit, value))
}

func (v *pinnedImageView) draw(c *ui.Context, bitmap *ui.Bitmap, a previewAppearance) {
	w, h := bitmap.Size()
	ui.Row(c).FillWidth().Children(func() {
		factor := max(0.1, min(8, v.factor()))
		if ui.Button(c, "−").Label(a.ZoomOut).Tooltip(a.ZoomOut).Disabled(factor <= 0.1).Clicked() {
			v.zoomAt(factor*0.8, v.width/2, v.height/2)
		}
		label := a.FitLabel
		if v.zoom > 0 {
			label = fmt.Sprintf("%.0f%%", 100*v.zoom)
		}
		ui.Text(c, label)
		if ui.Button(c, "+").Label(a.ZoomIn).Tooltip(a.ZoomIn).Disabled(factor >= 8).Clicked() {
			v.zoomAt(factor*1.25, v.width/2, v.height/2)
		}
		if ui.Button(c, a.FitLabel).Label(a.Fit).Tooltip(a.Fit).Clicked() {
			v.zoom = 0
			v.pan = ui.Point{}
		}
		if ui.Button(c, "100%").Label(a.Actual).Tooltip(a.Actual).Clicked() {
			v.zoomAt(1, v.width/2, v.height/2)
		}
	})
	ui.Box(c).Fill().Label(a.ImageZoom).HandleInput(func(e ui.InputEvent) bool {
		switch e.Kind {
		case ui.InputScroll:
			if e.DY == 0 {
				return false
			}
			multiplier := float32(1.2)
			if e.DY > 0 {
				multiplier = 1 / multiplier
			}
			v.zoomAt(v.factor()*multiplier, e.X, e.Y)
		case ui.InputZoom:
			if e.Scale <= 0 {
				return false
			}
			v.zoomAt(v.factor()*e.Scale, e.X, e.Y)
		case ui.InputPointerDown:
			if e.Button != 0 {
				return false
			}
			v.dragging = true
			v.grab = ui.Point{X: e.X - v.pan.X, Y: e.Y - v.pan.Y}
		case ui.InputPointerMove:
			if !v.dragging {
				return false
			}
			v.pan = ui.Point{X: e.X - v.grab.X, Y: e.Y - v.grab.Y}
		case ui.InputPointerUp:
			if !v.dragging || e.Button != 0 {
				return false
			}
			v.dragging = false
		default:
			return false
		}
		return true
	}).Draw(func(p *ui.Painter, r ui.Rect) {
		v.width, v.height = r.W, r.H
		v.fit = min(1, r.W/float32(w), r.H/float32(h))
		width := max(1, float32(math.Round(float64(float32(w)*v.factor()))))
		height := max(1, float32(math.Round(float64(float32(h)*v.factor()))))
		v.pan.X = pinPan(v.pan.X, r.W, width)
		v.pan.Y = pinPan(v.pan.Y, r.H, height)
		dst := ui.Rect{X: r.X + (r.W-width)/2 + v.pan.X, Y: r.Y + (r.H-height)/2 + v.pan.Y, W: width, H: height}
		p.Clip(r, 0, func() { p.ImageRegion(bitmap, ui.Rect{}, dst, ui.ImageOptions{}) })
	})
}
