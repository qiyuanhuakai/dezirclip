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

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/quickpaste"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
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
	if font := nativeInterfaceFont(settings); font != "" {
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
	refreshPinnedAppearance(owner, a, settings)
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
	refreshPinnedAppearance(owner, appearance, settings)
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
