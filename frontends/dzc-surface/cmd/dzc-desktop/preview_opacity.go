package main

import (
	"context"
	"encoding/json"
	"strconv"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
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
	return opacityPreference(prefs.Preview, prefs.Surface), nil
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

func (o *previewOpacityControl) drawToolbar(c *ui.Context, a previewAppearance, options uikit.PreviewToolbarOptions) {
	if options.OpacityLabel != "" {
		options.Opacity = &o.slider
		options.OnOpacityChange = func(value float64) {
			next := int(value)
			if o.value == next {
				return
			}
			o.value = next
			o.revision++
			o.failed = false
			c.Invalidate()
			if !o.writing && o.window != nil {
				o.save()
			}
		}
	}
	a.kit().PreviewToolbar(c, options)
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
