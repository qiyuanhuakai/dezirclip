package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"image"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

const (
	backgroundSetting = "app.custom_background"
	maxBackgroundFile = 10 << 20
	maxBackgroundWire = 16 << 20
)

var nativeBackgroundOpacity = nativeRangeSetting{key: "app.custom_background_opacity", unit: "%", maximum: 100, fallback: 45}

type nativeBackgroundKey struct {
	path, instance string
	epoch          uint64
}

type nativeBackground struct {
	key     nativeBackgroundKey
	bitmap  *ui.Bitmap
	gif     *nativeBackgroundGIF
	timer   *time.Timer
	loading bool
}

func (b *nativeBackground) pause() {
	b.loading = false
	if b.timer != nil {
		b.timer.Stop()
		b.timer = nil
	}
}

func (n *nativeMain) clearBackground() {
	n.background.pause()
	n.background = nativeBackground{}
}

func (n *nativeMain) backgroundKey(state protocol.RuntimeState) nativeBackgroundKey {
	switch n.themeBase() {
	case "mica", "acrylic", "liquid-glass":
		if path := n.preferences[backgroundSetting]; path != "" && state.ServiceInstanceId != "" {
			return nativeBackgroundKey{path: path, instance: state.ServiceInstanceId, epoch: state.PrivacyEpoch}
		}
	}
	return nativeBackgroundKey{}
}

func (n *nativeMain) loadBackground() {
	key := n.backgroundKey(n.history.state)
	if key.path == "" {
		n.clearBackground()
		return
	}
	if n.background.key == key && n.background.bitmap != nil {
		n.animateBackground()
		return
	}
	n.clearBackground()
	n.background = nativeBackground{key: key, loading: true}
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		source, err := n.library.Background(ctx, key.epoch, key.path)
		var bitmap *ui.Bitmap
		var animation *nativeBackgroundGIF
		if err == nil && source != nil {
			bitmap, animation = decodeNativeBackground(*source)
		}
		n.update(session, func() {
			if n.background.key != key {
				return
			}
			n.background.bitmap, n.background.gif, n.background.loading = bitmap, animation, false
			if bitmap == nil && n.notice == "" {
				n.notice = n.text("custom_background_load_failed")
			}
			n.animateBackground()
			n.revealWhenReady()
		})
	}()
}

func decodeNativeBackground(source string) (*ui.Bitmap, *nativeBackgroundGIF) {
	header, encoded, found := strings.Cut(source, ",")
	if !found || len(encoded) > base64.StdEncoding.EncodedLen(maxBackgroundWire) {
		return nil, nil
	}
	switch header {
	case "data:image/png;base64", "data:image/jpeg;base64", "data:image/gif;base64", "data:image/webp;base64":
	default:
		return nil, nil
	}
	raw, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil || len(raw) > maxBackgroundWire {
		return nil, nil
	}
	defer clear(raw)
	config, format, err := image.DecodeConfig(bytes.NewReader(raw))
	long, short := max(config.Width, config.Height), min(config.Width, config.Height)
	if err != nil || short <= 0 || long > 3840 || short > 2160 || header != "data:image/"+format+";base64" {
		return nil, nil
	}
	if format == "gif" {
		if animation := loadBackgroundGIF(raw); animation != nil {
			return ui.NewBitmap(animation.front), animation
		}
	}
	bitmap, err := ui.DecodeBitmap(raw)
	if err != nil {
		return nil, nil
	}
	return bitmap, nil
}

func (n *nativeMain) drawBackground(c *ui.Context, radius int) {
	b := &n.background
	opacity := nativeBackgroundOpacity.value(n.preferences)
	if b.bitmap != nil && opacity != 0 && b.key == n.backgroundKey(n.history.state) {
		ui.Image(c, b.bitmap).Absolute().Top(0).Right(0).Bottom(0).Left(0).
			Fit(ui.Cover).Radius(float32(radius)).Opacity(float32(opacity) / 100).PassThrough()
	}
}

func (n *nativeMain) animateBackground() {
	b := &n.background
	if n.session == nil || b.gif == nil || b.timer != nil || !n.window.IsVisible() ||
		b.key != n.backgroundKey(n.history.state) || nativeBackgroundOpacity.value(n.preferences) == 0 {
		return
	}
	animation := b.gif
	delay, ok := animation.nextDelay()
	if !ok {
		return
	}
	session := n.session
	b.timer = time.AfterFunc(delay, func() {
		n.update(session, func() {
			if b.gif != animation {
				return
			}
			b.timer = nil
			if n.window.IsVisible() {
				b.bitmap.Update(animation.advance())
				n.animateBackground()
			}
		})
	})
}

func (n *nativeMain) backgroundSettings(c *ui.Context) {
	switch n.themeBase() {
	case "mica", "acrylic", "liquid-glass":
	default:
		return
	}
	path := n.preferences[backgroundSetting]
	label := n.text("choose_background")
	if path != "" {
		label = n.text("change_background")
	}
	ui.Text(c, n.text("custom_background")).FontWeight(600)
	ui.Row(c).Gap(6).Wrap().Children(func() {
		disabled := n.mutating || n.session == nil || n.history.state.ServiceInstanceId == ""
		if uikit.Button(c, uikit.ButtonOptions{Label: label, Disabled: disabled}).Clicked() {
			n.chooseBackground(label)
		}
		if path != "" && uikit.Button(c, uikit.ButtonOptions{Label: n.text("clear_background"), Disabled: disabled}).Clicked() {
			n.saveSetting(backgroundSetting, "")
		}
	})
	if path != "" {
		ui.Text(c, filepath.Base(path)).TextColor(c.Theme().TextMuted).Wrap()
		n.rangeSetting(c, nativeBackgroundOpacity, n.text("background_opacity"), nativeBackgroundOpacity.value(n.preferences))
	}
}

func (n *nativeMain) chooseBackground(label string) {
	state := n.history.state
	if n.session == nil || state.ServiceInstanceId == "" || n.mutating {
		return
	}
	n.mutating, n.appearanceBusy, n.notice = true, true, ""
	window, session, language := n.window, n.session, n.language()
	n.shell.dock.dialogs.Add(1)
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
			Parent: window, Title: label,
			Filters: []mygo.FileFilter{{Name: language.text("native_background_file_filter"), Extensions: []string{"png", "jpg", "jpeg", "webp", "gif"}}},
		})
		n.shell.dock.dialogs.Add(-1)
		var notice string
		saved := false
		if err != nil {
			notice = language.text("custom_background_save_failed")
		} else if len(paths) != 0 && session.Err() == nil {
			path := paths[0]
			size, err := n.shell.FileSize(path)
			switch {
			case err != nil:
				notice = language.text("custom_background_save_failed")
			case size > maxBackgroundFile:
				notice = language.text("background_size_error")
			default:
				ctx, cancel := context.WithTimeout(session, 4*time.Second)
				err = n.library.SetSetting(ctx, backgroundSetting, path, state.PrivacyEpoch)
				cancel()
				if err != nil {
					notice = language.text("custom_background_save_failed")
				} else {
					saved = true
				}
			}
		}
		n.update(session, func() {
			n.mutating, n.appearanceBusy = false, false
			if window.IsVisible() && !window.IsMinimized() && !window.IsFocused() {
				window.Focus()
			}
			if saved {
				n.clearBackground()
			}
			if saved || n.appearanceSync {
				n.prepare(n.resume)
			}
			n.notice = notice
		})
	}()
}
