package main

import (
	"context"
	"encoding/json"
	"math"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

// All control state belongs to the UI thread. While one write is pending,
// subsequent slider movements replace the desired value for the next write.
type previewOpacityControl struct {
	value, committed int
	slider           float64
	writing, failed  bool
	epoch, revision  uint64
	library          Library
	window           *mygo.Window
}

func (l *Library) previewOpacity(ctx context.Context) (int, error) {
	raw, err := l.Preferences(ctx)
	if err != nil {
		return 0, err
	}
	var prefs struct {
		Preview string `json:"app.preview_opacity"`
		Surface string `json:"app.surface_opacity"`
	}
	if err = json.Unmarshal(raw, &prefs); err != nil {
		return 0, err
	}
	for _, value := range []string{prefs.Preview, prefs.Surface} {
		v, err := strconv.ParseFloat(strings.TrimSpace(value), 64)
		if err == nil && !math.IsNaN(v) && !math.IsInf(v, 0) {
			return int(math.Round(min(100, max(0, v)))), nil
		}
	}
	return 65, nil
}

func (o *previewOpacityControl) reload() {
	if o.writing {
		return
	}
	o.revision++
	version := o.revision
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		value, err := o.library.previewOpacity(ctx)
		o.window.Update(func() {
			if err == nil && version == o.revision {
				o.value, o.committed, o.slider = value, value, float64(value)
			}
		})
	}()
}

func (o *previewOpacityControl) draw(c *ui.Context, a previewAppearance) {
	if a.OpacityLabel == "" {
		return
	}
	t := c.Theme()
	ui.Row(c).Width(150).Height(26).Gap(5).Children(func() {
		slider := ui.SliderBase(c, &o.slider, 0, 100).Grow(1).Height(26).PaddingX(6).Label(a.OpacityLabel).Tooltip(a.OpacityLabel)
		slider.Draw(func(p *ui.Painter, r ui.Rect) {
			track := ui.Rect{X: r.X + 6, Y: r.Y + 11, W: max(0, r.W-12), H: 4}
			p.Stroke(track, t.Border, 2, 1)
			x := track.X + track.W*float32(o.slider/100)
			p.Fill(ui.Rect{X: x - 5, Y: r.Y + 7, W: 10, H: 12}, t.Surface.Alpha(.8), 3)
			p.Stroke(ui.Rect{X: x - 5, Y: r.Y + 7, W: 10, H: 12}, t.Border, 3, 1)
		})
		if slider.Changed() {
			next := int(math.Round(o.slider))
			if o.value != next {
				o.value = next
				o.revision++
				o.failed = false
				if !o.writing {
					o.save()
				}
			}
		}
		ui.Text(c, strconv.Itoa(o.value)+"%").FontSize(12).Width(32)
	})
	if o.failed {
		ui.Text(c, a.Failed)
	}
}

func (o *previewOpacityControl) save() {
	value, epoch := o.value, o.epoch
	o.writing = true
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		err := o.library.SetSetting(ctx, "app.preview_opacity", strconv.Itoa(value), epoch)
		o.window.Update(func() {
			if err == nil {
				o.committed = value
			}
			if o.epoch == epoch && o.value != value {
				o.save()
				return
			}
			o.writing = false
			o.failed = err != nil
			if err != nil {
				o.value = o.committed
				o.slider = float64(o.value)
			}
			o.reload()
		})
	}()
}
