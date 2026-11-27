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
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
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
	opacityDraft *int
	key          nativeBackgroundKey
	bitmap       *ui.Bitmap
	gif          *nativeBackgroundGIF
	timer        *time.Timer
	loading      bool
	enterUntil   time.Time
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
			if bitmap != nil {
				n.background.enterUntil = time.Now().Add(nativeAppearanceMotion)
			}
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

func (n *nativeMain) currentBackgroundOpacity() int {
	if n.background.opacityDraft != nil {
		return *n.background.opacityDraft
	}
	return nativeBackgroundOpacity.value(n.preferences)
}

func (n *nativeMain) drawBackground(c *ui.Context, radius int) bool {
	b := &n.background
	if b.bitmap == nil || b.key != n.backgroundKey(n.history.state) {
		return false
	}
	if opacity := n.currentBackgroundOpacity(); opacity != 0 {
		picture := ui.Image(c.Key(b.key.path), b.bitmap).Absolute().Top(0).Right(0).Bottom(0).Left(0).
			Fit(ui.Cover).Radius(float32(radius)).Opacity(float32(opacity) / 100).PassThrough()
		if time.Now().Before(b.enterUntil) {
			picture.Transition(ui.ElementTransition{Duration: nativeAppearanceMotion, Colors: true, Enter: &ui.Motion{}})
		}
	}
	return true
}

func (n *nativeMain) animateBackground() {
	b := &n.background
	if n.session == nil || b.gif == nil || b.timer != nil || !n.window.IsVisible() ||
		b.key != n.backgroundKey(n.history.state) || n.currentBackgroundOpacity() == 0 {
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
			if n.window.IsVisible() && n.currentBackgroundOpacity() != 0 {
				b.bitmap.Update(animation.advance())
				n.animateBackground()
			}
		})
	})
}

func (n *nativeMain) backgroundSettings(c *ui.Context, kit uikit.Kit) {
	switch n.themeBase() {
	case "mica", "acrylic", "liquid-glass":
	default:
		return
	}
	path := n.preferences[backgroundSetting]
	label := n.text("choose_background")
	var filename string
	if path != "" {
		label = n.text("change_background")
		filename = filepath.Base(path)
	}
	ui.Column(c.Key("background")).ColumnSpan(-1).Transition(nativeSettingReveal).Children(func() {
		kit.SettingsBackground(c, uikit.SettingsBackgroundOptions{
			Label: n.text("custom_background"), ChooseLabel: label, ClearLabel: n.text("clear_background"), Filename: filename,
			Disabled: n.appearanceBusy || n.preferenceUnavailable(backgroundSetting),
			OnChoose: func() { n.chooseBackground(label) },
			OnClear:  func() { n.saveSetting(backgroundSetting, "") },
		}, func() {
			n.drawBackgroundOpacity(c)
		})
	})
}

func (n *nativeMain) drawBackgroundOpacity(c *ui.Context) {
	saved, displayed := nativeBackgroundOpacity.value(n.preferences), n.currentBackgroundOpacity()
	live := n.rangeSetting(c, nativeBackgroundOpacity, n.text("background_opacity"), "", saved)
	if live == saved {
		n.background.opacityDraft = nil
	} else {
		n.background.opacityDraft = &live
	}
	if live != displayed {
		c.Invalidate()
		if displayed == 0 {
			n.animateBackground()
		}
	}
}

func (n *nativeMain) chooseBackground(label string) {
	state := n.history.state
	if n.appearanceBusy || n.preferenceUnavailable(backgroundSetting) || n.preferenceBusy("app.theme") {
		return
	}
	n.appearanceBusy, n.notice = true, ""
	window, session, language := n.window, n.session, n.language()
	wasVisible, visibility := window.IsVisible(), n.shell.resumeSequence.Load()
	n.shell.dock.dialogs.Add(1)
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
			Parent: window, Title: label,
			Filters: []mygo.FileFilter{{Name: language.text("native_background_file_filter"), Extensions: []string{"png", "jpg", "jpeg", "webp", "gif"}}},
		})
		var notice string
		var savedPath string
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
					savedPath = path
				}
			}
		}
		mygo.RunOnMain(func() {
			defer n.shell.dock.dialogs.Add(-1)
			if window.IsDestroyed() || session.Err() != nil || n.session != session {
				return
			}
			defer window.Invalidate()
			n.appearanceBusy = false
			if wasVisible && visibility == n.shell.resumeSequence.Load() && !window.IsMinimized() {
				n.shell.dock.shown = time.Now()
				if !window.IsVisible() {
					window.Show()
				}
				if !window.IsFocused() {
					window.Focus()
				}
			}
			if savedPath != "" {
				if savedPath == n.preferences[backgroundSetting] {
					n.clearBackground()
					n.loadBackground()
				} else {
					n.applySavedSetting(backgroundSetting, savedPath)
				}
			}
			if savedPath != "" || n.appearanceSync {
				n.appearanceSync = false
				n.refreshViewSettings()
			}
			n.notice = notice
		})
	}()
}
