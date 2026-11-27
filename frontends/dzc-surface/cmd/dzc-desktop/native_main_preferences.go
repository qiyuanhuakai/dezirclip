package main

import (
	"context"
	"encoding/json"
	"maps"
	"strconv"
	"time"

	"github.com/egoist/mygo"
)

func (n *nativeMain) saveSetting(key, value string) {
	if saved, ok := n.preferences[key]; ok && saved == value {
		return
	}
	n.savePreference(key, value, 4*time.Second, func(ctx context.Context, epoch uint64) error {
		return n.library.SetSetting(ctx, key, value, epoch)
	}, nil)
}

func (n *nativeMain) preferenceUnavailable(key string) bool {
	return n.session == nil || n.history.state.ServiceInstanceId == "" || n.preferenceBusy(key)
}

func (n *nativeMain) preferenceBusy(key string) bool {
	return n.mutating || n.pendingSettings[key]
}

func (n *nativeMain) savePreference(key, value string, timeout time.Duration, write func(context.Context, uint64) error, finish func(error)) {
	if n.preferenceUnavailable(key) {
		return
	}
	if n.pendingSettings == nil {
		n.pendingSettings = make(map[string]bool)
	}
	n.pendingSettings[key] = true
	n.viewSettingsRevision++
	session, epoch := n.session, n.history.state.PrivacyEpoch
	go func() {
		ctx, cancel := context.WithTimeout(session, timeout)
		defer cancel()
		err := write(ctx, epoch)
		n.update(session, func() {
			delete(n.pendingSettings, key)
			if finish != nil {
				finish(err)
			}
			if err == nil {
				n.applySavedSetting(key, value)
			} else {
				n.notice = n.text("native_setting_save_failed")
			}
			// Include external writes and recover an acknowledgement lost after commit.
			n.refreshViewSettings()
		})
	}()
}

func (n *nativeMain) applySavedSetting(key, value string) {
	settings := maps.Clone(n.preferences)
	if settings == nil {
		settings = make(map[string]string, 1)
	}
	settings[key] = value
	if n.applyViewSettings(settings) {
		n.viewSettingsRevision++
	}
}

func (n *nativeMain) togglePin() {
	const key = "app.window_pinned"
	pinned := !n.pinned
	n.savePreference(key, strconv.FormatBool(pinned), 4*time.Second, func(ctx context.Context, epoch uint64) error {
		return n.library.SetWindowPinned(ctx, pinned, epoch)
	}, func(err error) {
		if err == nil {
			n.shell.dock.autoPinned = false
			n.syncWindowPin()
		}
	})
}

// Coalesce preference notifications while keeping the live history, scroll
// positions and selection owned by the current presentation.
func (n *nativeMain) refreshViewSettings() {
	if n.session == nil || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.viewSettingsRevision++
	if n.readingViewSettings || len(n.pendingSettings) != 0 {
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
		n.updateIfChanged(session, func() bool {
			n.readingViewSettings = false
			if revision != n.viewSettingsRevision {
				n.refreshViewSettings()
				return false
			}
			if n.appearanceBusy {
				n.appearanceSync = true
			} else if !n.winV.changing && err == nil && settings != nil {
				return n.applyViewSettings(settings)
			}
			return false
		})
	}()
}

func (n *nativeMain) updateIfChanged(session context.Context, apply func() bool) {
	if session.Err() != nil {
		return
	}
	mygo.RunOnMain(func() {
		if !n.window.IsDestroyed() && session.Err() == nil && n.session == session && apply() {
			n.window.Invalidate()
		}
	})
}

func (n *nativeMain) applyViewSettings(settings map[string]string) bool {
	previous := n.preferences
	if maps.Equal(previous, settings) {
		return false
	}
	background := n.backgroundKey(n.history.state)
	if previous["app.theme"] != settings["app.theme"] || n.mode != n.colorMode(settings) ||
		n.importedTheme != nil && previous["app.custom_themes"] != settings["app.custom_themes"] ||
		n.language() != mainLanguage(settings["app.language"]) ||
		nativeInterfaceFont(previous) != nativeInterfaceFont(settings) ||
		!n.showSettings && (previous["app.compact_mode"] == "true") != (settings["app.compact_mode"] == "true") {
		n.startAppearanceMotion()
	}
	n.configured(settings)
	if next := n.backgroundKey(n.history.state); background != next && n.session != nil && n.window != nil {
		n.loadBackground()
	} else if previous[nativeBackgroundOpacity.key] != settings[nativeBackgroundOpacity.key] && n.window != nil {
		n.animateBackground()
	}
	if n.preview == nil || n.preview.opacity == nil {
		return true
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
	return true
}

func (n *nativeMain) startAppearanceMotion() {
	n.appearanceMotionUntil = time.Now().Add(nativeAppearanceMotion)
	n.appearanceRevision++
}
