package main

import (
	"context"
	"encoding/json"
	"maps"
	"strconv"
	"time"
)

func (n *nativeMain) saveSetting(key, value string) {
	n.writeSettingWithEffect(func(ctx context.Context, epoch uint64) error {
		return n.library.SetSetting(ctx, key, value, epoch)
	}, func() { n.applySavedSetting(key, value) })
}

func (n *nativeMain) applySavedSetting(key, value string) {
	settings := maps.Clone(n.preferences)
	if settings == nil {
		settings = make(map[string]string, 1)
	}
	settings[key] = value
	n.applyViewSettings(settings)
}

func (n *nativeMain) togglePin() {
	pinned := !n.pinned
	n.writeSettingWithEffect(func(ctx context.Context, epoch uint64) error {
		return n.library.SetWindowPinned(ctx, pinned, epoch)
	}, func() {
		n.shell.dock.autoPinned = false
		n.applySavedSetting("app.window_pinned", strconv.FormatBool(pinned))
		n.syncWindowPin()
	})
}

func (n *nativeMain) writeSetting(write func(context.Context, uint64) error) {
	n.writeSettingWithEffect(write, nil)
}

func (n *nativeMain) writeSettingWithEffect(write func(context.Context, uint64) error, onSaved func()) {
	n.writeSettingWithResult(4*time.Second, write, func(err error) {
		if err == nil && onSaved != nil {
			onSaved()
		}
	})
}

func (n *nativeMain) writeSettingWithResult(timeout time.Duration, write func(context.Context, uint64) error, onFinished func(error)) {
	n.performSettingWrite(timeout, write, func(err error) {
		onFinished(err)
		if err == nil {
			n.refreshViewSettings()
		}
	})
}

func (n *nativeMain) performSettingWrite(timeout time.Duration, write func(context.Context, uint64) error, onFinished func(error)) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating, n.notice = true, ""
	session, state := n.session, n.history.state
	go func() {
		ctx, cancel := context.WithTimeout(session, timeout)
		defer cancel()
		err := write(ctx, state.PrivacyEpoch)
		n.update(session, func() {
			n.mutating = false
			onFinished(err)
			if err != nil {
				n.notice = n.text("native_setting_save_failed")
			}
		})
	}()
}

// Coalesce preference notifications while keeping the live history, scroll
// positions and selection owned by the current presentation.
func (n *nativeMain) refreshViewSettings() {
	if n.session == nil || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.viewSettingsRevision++
	if n.readingViewSettings {
		return
	}
	n.readingViewSettings = true
	session, state, revision := n.session, n.history.state, n.viewSettingsRevision
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		raw, err := n.library.preferences(ctx, state)
		var settings map[string]string
		if err == nil {
			err = json.Unmarshal(raw, &settings)
		}
		n.update(session, func() {
			n.readingViewSettings = false
			if revision != n.viewSettingsRevision {
				n.refreshViewSettings()
				return
			}
			if n.appearanceBusy {
				n.appearanceSync = true
			} else if !n.winV.changing && err == nil && settings != nil {
				n.applyViewSettings(settings)
			}
		})
	}()
}

func (n *nativeMain) applyViewSettings(settings map[string]string) {
	previous := n.preferences
	if maps.Equal(previous, settings) {
		return
	}
	background := n.backgroundKey(n.history.state)
	if previous["app.theme"] != settings["app.theme"] || previous["app.color_mode"] != settings["app.color_mode"] ||
		previous["app.custom_themes"] != settings["app.custom_themes"] {
		n.appearanceMotionUntil = time.Now().Add(nativeAppearanceMotion)
	}
	n.configured(settings)
	if next := n.backgroundKey(n.history.state); background != next && n.session != nil && n.window != nil {
		n.loadBackground()
	} else if previous[nativeBackgroundOpacity.key] != settings[nativeBackgroundOpacity.key] && n.window != nil {
		n.animateBackground()
	}
	if n.preview == nil || n.preview.opacity == nil {
		return
	}
	key, surface := nativePreviewOpacity.key, nativeSurfaceOpacity.key
	opacity := opacityPreference(settings[key], settings[surface])
	if opacity != opacityPreference(previous[key], previous[surface]) {
		control := n.preview.opacity
		control.applyPreference(opacity)
		if control.window != nil {
			control.window.Invalidate()
		}
	}
}
