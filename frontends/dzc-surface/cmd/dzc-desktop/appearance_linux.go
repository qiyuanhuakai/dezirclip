package main

import (
	"context"
	"os/exec"
	"strings"
	"time"

	"github.com/egoist/mygo"
)

func (*DesktopShell) PlatformInfo() platformInfo {
	return platformInfo{Platform: "linux", Linux: true}
}

func (*DesktopShell) SystemThemeMode(ctx context.Context) string {
	ctx, cancel := context.WithTimeout(ctx, time.Second)
	defer cancel()
	for _, key := range []string{"color-scheme", "gtk-theme"} {
		out, err := exec.CommandContext(ctx, "gsettings", "get", "org.gnome.desktop.interface", key).Output()
		if err == nil {
			value := strings.ToLower(string(out))
			if strings.Contains(value, "dark") {
				return "dark"
			}
			if key == "color-scheme" && strings.Contains(value, "light") {
				return "light"
			}
		}
	}
	return "system"
}

func applyWindowAppearance(w *mygo.Window, _, _ string, _ bool) error {
	return w.SetBackgroundColor("#00000000")
}
