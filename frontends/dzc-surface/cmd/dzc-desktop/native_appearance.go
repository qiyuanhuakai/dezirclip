package main

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/quickpaste"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/theme"
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

func restoreNativeAppearance(owner *client.Client, panel *quickpaste.Panel, settings map[string]string) {
	mode := settings["app.color_mode"]
	if mode != "light" && mode != "dark" {
		mode = (&DesktopShell{}).SystemThemeMode(context.Background())
	}
	if mode != "dark" {
		mode = "light"
	}
	a := theme.Appearance{Mode: mode, Locale: settings["app.language"], Opacity: .65, Tokens: theme.Select(mode)}
	if value, err := strconv.ParseFloat(settings["app.surface_opacity"], 64); err == nil {
		a.Opacity = max(0, min(1, value/100))
	}
	if font := strings.TrimSpace(settings["app.font_main"]); font != "" && len(font) <= 256 {
		a.Tokens.FontFamilies = []string{font}
	}
	if size, err := strconv.Atoi(settings["app.clipboard_item_font_size"]); err == nil {
		a.Tokens.FontSize = max(8, min(32, size))
	}
	path := nativeAppearancePath(owner)
	if info, err := os.Lstat(path); err == nil && info.Mode().IsRegular() && info.Size() <= 65536 {
		if bytes, err := os.ReadFile(path); err == nil {
			var cached appearanceCache
			if json.Unmarshal(bytes, &cached) == nil && cached.Key == appearanceKey(settings) && cached.Appearance.Mode == mode && cached.Appearance.Valid() {
				a = cached.Appearance
			}
		}
	}
	panel.SetAppearance(a)
	refreshPinnedAppearance(owner, a)
}

func saveNativeAppearance(owner *client.Client, panel *quickpaste.Panel, appearance theme.Appearance) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	settingsJSON, err := (&Library{owner}).Preferences(ctx)
	var settings map[string]string
	if err != nil || json.Unmarshal(settingsJSON, &settings) != nil {
		panel.SetAppearance(appearance)
		return
	}
	appearance.Locale = settings["app.language"]
	panel.SetAppearance(appearance)
	refreshPinnedAppearance(owner, appearance)
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
