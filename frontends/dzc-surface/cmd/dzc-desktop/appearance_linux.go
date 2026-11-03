package main

import (
	"context"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/egoist/mygo"
)

func mainWindowScale(w *mygo.Window) float64 {
	return mygo.Screen.DisplayMatching(w.Bounds()).ScaleFactor
}

func configureSoftwareWebview(child *exec.Cmd) {
	child.Env = append(os.Environ(), "WEBKIT_DISABLE_COMPOSITING_MODE=1", "WEBKIT_DISABLE_DMABUF_RENDERER=1")
}

func placeMainAtCursor(w *mygo.Window) {
	_, display := os.LookupEnv("WAYLAND_DISPLAY")
	_, socket := os.LookupEnv("WAYLAND_SOCKET")
	if display || socket || strings.EqualFold(os.Getenv("XDG_SESSION_TYPE"), "wayland") {
		return
	}
	cursor := mygo.Screen.CursorScreenPoint()
	area := mygo.Screen.DisplayNearestPoint(cursor).WorkArea
	width, height := w.Size()
	if area.Width <= 0 || area.Height <= 0 || width <= 0 || height <= 0 {
		return
	}
	point := nearCursor(cursor, area, width, height)
	w.SetPosition(point.X, point.Y)
}

func (*DesktopShell) PlatformInfo() platformInfo {
	return platformInfo{Platform: "linux", Linux: true}
}

// Creating an AppIndicator exports an item even on desktops without a tray host.
func trayHostAvailable(_ *mygo.Tray) bool {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, "gdbus", "call", "--session",
		"--dest", "org.kde.StatusNotifierWatcher", "--object-path", "/StatusNotifierWatcher",
		"--method", "org.freedesktop.DBus.Properties.Get",
		"org.kde.StatusNotifierWatcher", "IsStatusNotifierHostRegistered").Output()
	return err == nil && strings.TrimSpace(string(out)) == "(<true>,)"
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
