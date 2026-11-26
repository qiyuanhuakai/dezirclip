package main

import (
	"context"
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/quickpaste"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/theme"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-presets"
)

type appearanceCache struct {
	Key        string
	Appearance theme.Appearance
}

func appearanceKey(settings map[string]string) string {
	values := []string{}
	for _, key := range []string{"app.theme", "app.color_mode", "app.custom_themes", "app.font_main", "app.clipboard_item_font_size", "app.surface_opacity", "app.language"} {
		values = append(values, settings[key])
	}
	encoded, _ := json.Marshal(values)
	return string(encoded)
}

func nativeAppearancePath(owner *client.Client) string {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	profile, err := (&Library{owner}).ProfileInfo(ctx)
	if err != nil || !filepath.IsAbs(profile.DataDirectory) {
		return ""
	}
	return filepath.Join(profile.DataDirectory, "mygo-native-appearance.json")
}

func nativeInterfaceFont(settings map[string]string) string {
	font := strings.TrimSpace(settings["app.font_main"])
	if len(font) > 256 {
		return ""
	}
	return font
}

func opacityPreference(values ...string) int {
	for _, value := range values {
		number, err := strconv.ParseFloat(strings.TrimSpace(value), 64)
		if err == nil && !math.IsNaN(number) && !math.IsInf(number, 0) {
			return int(math.Round(max(0, min(100, number))))
		}
	}
	return 65
}

type nativeSurfaceAppearance struct {
	base       string
	quickPaste theme.Appearance
	preview    theme.Appearance
	preset     presets.Preset
}

func surfaceThemeSelection(settings map[string]string) (string, *protocol.DesktopThemePackage) {
	base := settings["app.theme"]
	if strings.HasPrefix(base, "custom:") {
		if imported := selectedImportedTheme(importedThemeCatalog(settings["app.custom_themes"]), base); imported != nil {
			return imported.Base, imported
		}
		base = ""
	}
	if base == "" {
		base = "mica"
	}
	return base, nil
}

func resolveNativeSurfaceAppearance(settings map[string]string, systemDark bool) nativeSurfaceAppearance {
	mode := settings["app.color_mode"]
	if mode != "light" && mode != "dark" {
		mode = "light"
		if systemDark {
			mode = "dark"
		}
	}
	base, imported := surfaceThemeSelection(settings)
	preset := presets.Select(base, mode)
	if imported != nil {
		preset = presets.Imported(base, mode, imported.Css)
	}
	a := theme.Appearance{
		Mode: mode, Locale: string(mainLanguage(settings["app.language"])), Tokens: preset.Tokens(),
		Opacity: float64(opacityPreference(settings["app.surface_opacity"])) / 100,
	}
	if font := nativeInterfaceFont(settings); font != "" {
		a.Tokens.FontFamilies = []string{font}
	}
	if tint := preset.SurfaceTint(); tint != "" {
		a.Tokens.Background = tint
	}
	a.Tokens.FontSize = nativeItemSize.value(settings)
	preview := a
	preview.Opacity = float64(opacityPreference(settings["app.preview_opacity"], settings["app.surface_opacity"])) / 100
	return nativeSurfaceAppearance{base: base, quickPaste: a, preview: preview, preset: preset}
}

func bindResidentAppearance(ctx context.Context, owner *client.Client, panel *quickpaste.Panel, startup map[string]string, native bool, settingsChanged func(map[string]string)) func() {
	var latest map[string]string
	var previousKey string
	applied := false
	apply := func(settings map[string]string) {
		if !native {
			restoreWebViewAppearance(owner, panel, settings)
			return
		}
		latest = settings
		systemDark := settings["app.color_mode"] != "light" && settings["app.color_mode"] != "dark" && mygo.Theme.IsDark()
		key := appearanceKey(settings) + "\x00" + settings["app.preview_opacity"] + strconv.FormatBool(systemDark)
		if applied && previousKey == key {
			return
		}
		applied, previousKey = true, key
		resolved := resolveNativeSurfaceAppearance(settings, systemDark)
		panel.SetAppearance(resolved.quickPaste, resolved.preset.Kit().WithInterfaceFont(nativeInterfaceFont(settings)), resolved.base)
		refreshPinnedAppearance(owner, resolved.preview, resolved.base)
	}
	apply(startup)
	settingsChanged(startup)
	if native {
		unsubscribe := mygo.Theme.OnUpdated(func() { apply(latest) })
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) { unsubscribe() })
	}

	reading, refreshPending := false, false
	var refresh func()
	refresh = func() {
		if ctx.Err() != nil {
			return
		}
		if reading {
			refreshPending = true
			return
		}
		reading = true
		go func() {
			read, cancel := context.WithTimeout(ctx, 2*time.Second)
			defer cancel()
			var values map[string]string
			encoded, err := (&Library{owner}).Preferences(read)
			if err == nil {
				err = json.Unmarshal(encoded, &values)
			}
			mygo.RunOnMain(func() {
				reading = false
				if ctx.Err() != nil {
					return
				}
				if refreshPending {
					refreshPending = false
					refresh()
				} else if err == nil {
					apply(values)
					settingsChanged(values)
				}
			})
		}()
	}
	panel.SetAppearanceRefresher(refresh)
	if native {
		return refresh
	}
	return nil
}

func restoreWebViewAppearance(owner *client.Client, panel *quickpaste.Panel, settings map[string]string) {
	mode := settings["app.color_mode"]
	if mode != "light" && mode != "dark" {
		mode = (&DesktopShell{}).SystemThemeMode(context.Background())
	}
	if mode != "dark" {
		mode = "light"
	}
	resolved := resolveNativeSurfaceAppearance(settings, mode == "dark")
	a := resolved.quickPaste
	path := nativeAppearancePath(owner)
	if info, err := os.Lstat(path); err == nil && info.Mode().IsRegular() && info.Size() <= 65536 {
		if bytes, err := os.ReadFile(path); err == nil {
			var cached appearanceCache
			if json.Unmarshal(bytes, &cached) == nil && cached.Key == appearanceKey(settings) && cached.Appearance.Mode == a.Mode && cached.Appearance.Valid() {
				a = cached.Appearance
			}
		}
	}
	a.Tokens.Background = resolved.quickPaste.Tokens.Background
	panel.SetAppearance(a, resolved.preset.Kit().WithInterfaceFont(nativeInterfaceFont(settings)), resolved.base)
	refreshPinnedAppearance(owner, resolved.preview, resolved.base)
}

func saveNativeAppearance(owner *client.Client, panel *quickpaste.Panel, appearance theme.Appearance) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	settingsJSON, err := (&Library{owner}).Preferences(ctx)
	var settings map[string]string
	if err != nil || json.Unmarshal(settingsJSON, &settings) != nil {
		return
	}
	resolved := resolveNativeSurfaceAppearance(settings, appearance.Mode == "dark")
	if appearance.Mode != resolved.quickPaste.Mode {
		return
	}
	appearance.Locale = settings["app.language"]
	appearance.Tokens.Background = resolved.quickPaste.Tokens.Background
	panel.SetAppearance(appearance, resolved.preset.Kit().WithInterfaceFont(nativeInterfaceFont(settings)), resolved.base)
	refreshPinnedAppearance(owner, resolved.preview, resolved.base)
	path := nativeAppearancePath(owner)
	if path == "" {
		return
	}
	bytes, err := json.Marshal(appearanceCache{appearanceKey(settings), appearance})
	if err != nil || len(bytes) > 65536 {
		return
	}
	file, err := os.CreateTemp(filepath.Dir(path), ".native-appearance-")
	if err != nil {
		return
	}
	defer os.Remove(file.Name())
	_, err = file.Write(bytes)
	closed := file.Close()
	if err == nil && closed == nil {
		_ = os.Rename(file.Name(), path)
	}
}
