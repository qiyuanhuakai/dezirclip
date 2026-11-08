package main

import (
	"context"
	"os"
	"os/exec"
	"strings"
	"sync"
	"time"

	"github.com/ebitengine/purego"
	"github.com/egoist/mygo"
)

func mainWindowScale(w *mygo.Window) float64 {
	return mygo.Screen.DisplayMatching(w.Bounds()).ScaleFactor
}

func dockingAvailable() bool {
	services, ok := os.LookupEnv("DEZIRCLIP_DISABLE_LINUX_SERVICES")
	if !ok {
		services = os.Getenv("TIEZ_DISABLE_LINUX_SERVICES")
	}
	for _, service := range strings.FieldsFunc(services, func(r rune) bool { return r == ',' || r == ';' || r == ' ' }) {
		if strings.EqualFold(service, "edge_docking") || strings.EqualFold(service, "all") {
			return false
		}
	}
	_, wayland := os.LookupEnv("WAYLAND_DISPLAY")
	_, socket := os.LookupEnv("WAYLAND_SOCKET")
	return !wayland && !socket && !strings.EqualFold(os.Getenv("XDG_SESSION_TYPE"), "wayland")
}

// GDK reports modifier and mouse-button state against the same root coordinates as GTK.
var dockPointer = sync.OnceValue(func() func() (mygo.Point, bool, bool) {
	lib, err := purego.Dlopen("libgdk-3.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil
	}
	var rootWindow func() uintptr
	var pointer func(uintptr, *int32, *int32, *uint32) uintptr
	purego.RegisterLibFunc(&rootWindow, lib, "gdk_get_default_root_window")
	purego.RegisterLibFunc(&pointer, lib, "gdk_window_get_pointer")
	return func() (mygo.Point, bool, bool) {
		root := rootWindow()
		if root == 0 {
			return mygo.Point{}, false, false
		}
		var x, y int32
		var mask uint32
		pointer(root, &x, &y, &mask)
		return mygo.Point{X: int(x), Y: int(y)}, mask&(7<<8) != 0, true
	}
})

func readDockSample(w *mygo.Window) (dockSample, bool) {
	read := dockPointer()
	if read == nil {
		return dockSample{}, false
	}
	p, pressed, ok := read()
	bounds := w.Bounds()
	return dockSample{window: bounds, monitor: mygo.Screen.DisplayMatching(bounds).Bounds, cursor: p, pressed: pressed}, ok
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

func previewDisplay(_ *mygo.Window, bounds mygo.Rectangle) (mygo.Rectangle, mygo.Point) {
	return mygo.Screen.DisplayMatching(bounds).WorkArea, mygo.Screen.CursorScreenPoint()
}

func previewPointer(w *mygo.Window) (mygo.Rectangle, mygo.Point, bool) {
	return w.Bounds(), mygo.Screen.CursorScreenPoint(), true
}
