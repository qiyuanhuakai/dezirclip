package main

import (
	"context"
	"os/exec"
	"unsafe"

	"github.com/egoist/mygo"
	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

func configureSoftwareWebview(child *exec.Cmd) {
	child.Args = append(child.Args, "--software-webview")
}

func (*DesktopShell) PlatformInfo() platformInfo {
	build := windows.RtlGetVersion().BuildNumber
	return platformInfo{Platform: "windows", Windows10: build >= 10240 && build < 22000, Windows11: build >= 22000}
}

func (*DesktopShell) SystemThemeMode(context.Context) string {
	key, err := registry.OpenKey(registry.CURRENT_USER, `Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`, registry.QUERY_VALUE)
	if err != nil {
		return "system"
	}
	defer key.Close()
	light, _, err := key.GetIntegerValue("AppsUseLightTheme")
	if err != nil {
		return "system"
	}
	if light == 0 {
		return "dark"
	}
	return "light"
}

var setComposition = windows.NewLazySystemDLL("user32.dll").NewProc("SetWindowCompositionAttribute")

var (
	user32           = windows.NewLazySystemDLL("user32.dll")
	clientOrigin     = user32.NewProc("ClientToScreen")
	windowMonitor    = user32.NewProc("MonitorFromWindow")
	windowDPI        = user32.NewProc("GetDpiForWindow")
	cursorPosition   = user32.NewProc("GetPhysicalCursorPos")
	cursorMonitor    = user32.NewProc("MonitorFromRect")
	monitorInfo      = user32.NewProc("GetMonitorInfoW")
	windowRect       = user32.NewProc("GetWindowRect")
	moveWindow       = user32.NewProc("SetWindowPos")
	mouseButtonState = user32.NewProc("GetAsyncKeyState")
)

func dockingAvailable() bool { return true }

func restoreMainPosition(w *mygo.Window, docked bool) {
	type rect struct{ left, top, right, bottom int32 }
	var bounds rect
	if ok, _, _ := windowRect.Call(w.NativeHandle(), uintptr(unsafe.Pointer(&bounds))); ok == 0 {
		return
	}
	monitor, _, _ := cursorMonitor.Call(uintptr(unsafe.Pointer(&bounds)), 2)
	var info struct {
		size         uint32
		bounds, work rect
		flags        uint32
	}
	info.size = uint32(unsafe.Sizeof(info))
	if ok, _, _ := monitorInfo.Call(monitor, uintptr(unsafe.Pointer(&info))); ok == 0 {
		return
	}
	point := visibleMainPosition(mygo.Rectangle{X: int(bounds.left), Y: int(bounds.top), Width: int(bounds.right - bounds.left), Height: int(bounds.bottom - bounds.top)},
		mygo.Rectangle{X: int(info.work.left), Y: int(info.work.top), Width: int(info.work.right - info.work.left), Height: int(info.work.bottom - info.work.top)}, docked)
	moveWindow.Call(w.NativeHandle(), 0, uintptr(point.X), uintptr(point.Y), 0, 0, 0x0001|0x0004|0x0010)
}

func readDockSample(w *mygo.Window) (dockSample, bool) {
	// Keep all three rectangles/points in physical pixels even across mixed-DPI displays.
	type rect struct{ left, top, right, bottom int32 }
	var bounds rect
	var point struct{ x, y int32 }
	if ok, _, _ := windowRect.Call(w.NativeHandle(), uintptr(unsafe.Pointer(&bounds))); ok == 0 {
		return dockSample{}, false
	}
	if ok, _, _ := cursorPosition.Call(uintptr(unsafe.Pointer(&point))); ok == 0 {
		return dockSample{}, false
	}
	monitor, _, _ := cursorMonitor.Call(uintptr(unsafe.Pointer(&bounds)), 2)
	var info struct {
		size         uint32
		bounds, work rect
		flags        uint32
	}
	info.size = uint32(unsafe.Sizeof(info))
	if ok, _, _ := monitorInfo.Call(monitor, uintptr(unsafe.Pointer(&info))); ok == 0 {
		return dockSample{}, false
	}
	rectOf := func(r rect) mygo.Rectangle {
		return mygo.Rectangle{X: int(r.left), Y: int(r.top), Width: int(r.right - r.left), Height: int(r.bottom - r.top)}
	}
	sample := dockSample{window: rectOf(bounds), monitor: rectOf(info.bounds), cursor: mygo.Point{X: int(point.x), Y: int(point.y)}}
	for _, button := range []uintptr{1, 2, 4} {
		state, _, _ := mouseButtonState.Call(button)
		if state&0x8000 != 0 {
			sample.pressed = true
			break
		}
	}
	return sample, true
}

func mainWindowScale(w *mygo.Window) float64 {
	dpi, _, _ := windowDPI.Call(w.NativeHandle())
	return float64(dpi) / 96
}

func placeWindowAtCursor(w *mygo.Window) {
	type rect struct{ left, top, right, bottom int32 }
	var cursor struct{ x, y int32 }
	if ok, _, _ := cursorPosition.Call(uintptr(unsafe.Pointer(&cursor))); ok == 0 {
		return
	}
	probe := rect{cursor.x, cursor.y, cursor.x + 1, cursor.y + 1}
	monitor, _, _ := cursorMonitor.Call(uintptr(unsafe.Pointer(&probe)), 2)
	var info struct {
		size         uint32
		bounds, work rect
		flags        uint32
	}
	info.size = uint32(unsafe.Sizeof(info))
	if ok, _, _ := monitorInfo.Call(monitor, uintptr(unsafe.Pointer(&info))); ok == 0 {
		return
	}
	area := mygo.Rectangle{X: int(info.work.left), Y: int(info.work.top),
		Width: int(info.work.right - info.work.left), Height: int(info.work.bottom - info.work.top)}
	hwnd := w.NativeHandle()
	// A move across monitors can synchronously resize the window for its new DPI.
	// Read the actual physical outer rectangle again before the final placement.
	for range 2 {
		var bounds rect
		if ok, _, _ := windowRect.Call(hwnd, uintptr(unsafe.Pointer(&bounds))); ok == 0 {
			return
		}
		point := nearCursor(mygo.Point{X: int(cursor.x), Y: int(cursor.y)}, area,
			int(bounds.right-bounds.left), int(bounds.bottom-bounds.top))
		if point.X == int(bounds.left) && point.Y == int(bounds.top) {
			return
		}
		const noSizeZOrderOrActivate = 0x0001 | 0x0004 | 0x0010
		if ok, _, _ := moveWindow.Call(hwnd, 0, uintptr(point.X), uintptr(point.Y), 0, 0, noSizeZOrderOrActivate); ok == 0 {
			return
		}
	}
}

// Before Windows 11 22H2 the compositor exposes acrylic through the accent policy.
func legacyAcrylic(hwnd windows.HWND, enabled, dark bool) error {
	policy := [4]uint32{0, 2, 0, 0}
	if enabled {
		policy = [4]uint32{4, 0, 0x28f0f0f0, 0}
		if dark {
			policy[2] = 0x281e1e1e
		}
	}
	attr := struct {
		attribute uint32
		value     *[4]uint32
		size      uintptr
	}{19, &policy, unsafe.Sizeof(policy)}
	if err := setComposition.Find(); err != nil {
		return err
	}
	ok, _, err := setComposition.Call(uintptr(hwnd), uintptr(unsafe.Pointer(&attr)))
	if ok == 0 {
		return err
	}
	return nil
}

func applyWindowAppearance(w *mygo.Window, theme, mode string, border bool) error {
	build := windows.RtlGetVersion().BuildNumber
	hwnd := windows.HWND(w.NativeHandle())
	set := func(attribute, value uint32) error {
		return windows.DwmSetWindowAttribute(hwnd, attribute, unsafe.Pointer(&value), 4)
	}
	dark := uint32(0)
	if mode == "dark" {
		dark = 1
	}
	material := mygo.VibrancyNone
	if theme == "mica" && build >= 22000 {
		material = mygo.VibrancyMica
	} else if theme == "acrylic" && build >= 17763 {
		material = mygo.VibrancyAcrylic
	}
	w.SetVibrancy(material)
	w.SetHasShadow(border && (theme != "mica" && theme != "acrylic" || build >= 22000))
	if build >= 18985 {
		if err := set(windows.DWMWA_USE_IMMERSIVE_DARK_MODE, dark); err != nil {
			return err
		}
	}
	if build >= 22000 {
		color := uint32(0xffffffff)
		if !border {
			color = 0xfffffffe
		}
		if err := set(windows.DWMWA_BORDER_COLOR, color); err != nil {
			return err
		}
		if err := set(windows.DWMWA_WINDOW_CORNER_PREFERENCE, 2); err != nil {
			return err
		}
	}
	if build >= 17763 && build < 22621 {
		if build >= 22000 {
			mica := uint32(0)
			if material == mygo.VibrancyMica {
				mica = 1
			}
			if err := set(1029, mica); err != nil {
				return err
			}
		}
		return legacyAcrylic(hwnd, material == mygo.VibrancyAcrylic, mode == "dark")
	}
	return nil
}

func trayHostAvailable(tray *mygo.Tray) bool {
	area := tray.Bounds()
	return area.Width > 0 && area.Height > 0
}

func previewDisplay(w *mygo.Window, bounds mygo.Rectangle) (mygo.Rectangle, mygo.Point) {
	// MyGo translates Flyout.Anchor using the parent HWND's client origin and DPI.
	// Select its monitor in physical coordinates before converting the work area.
	var origin, cursor struct{ x, y int32 }
	handle := w.NativeHandle()
	if ok, _, _ := clientOrigin.Call(handle, uintptr(unsafe.Pointer(&origin))); ok == 0 {
		return mygo.Rectangle{}, mygo.Point{}
	}
	monitor, _, _ := windowMonitor.Call(handle, 2)
	var info struct {
		size          uint32
		display, work struct{ left, top, right, bottom int32 }
		flags         uint32
	}
	info.size = uint32(unsafe.Sizeof(info))
	if ok, _, _ := monitorInfo.Call(monitor, uintptr(unsafe.Pointer(&info))); ok == 0 {
		return mygo.Rectangle{}, mygo.Point{}
	}
	if ok, _, _ := cursorPosition.Call(uintptr(unsafe.Pointer(&cursor))); ok == 0 {
		return mygo.Rectangle{}, mygo.Point{}
	}
	scale := mainWindowScale(w)
	if scale <= 0 {
		return mygo.Rectangle{}, mygo.Point{}
	}
	area := mygo.Rectangle{X: int(info.work.left), Y: int(info.work.top), Width: int(info.work.right - info.work.left), Height: int(info.work.bottom - info.work.top)}
	work := previewParentArea(bounds, mygo.Point{X: int(origin.x), Y: int(origin.y)}, area, scale)
	point := mygo.Point{X: bounds.X + int(float64(cursor.x-origin.x)/scale), Y: bounds.Y + int(float64(cursor.y-origin.y)/scale)}
	return work, point
}

func previewPointer(w *mygo.Window) (mygo.Rectangle, mygo.Point, bool) {
	var rect struct{ left, top, right, bottom int32 }
	var p struct{ x, y int32 }
	if ok, _, _ := windowRect.Call(w.NativeHandle(), uintptr(unsafe.Pointer(&rect))); ok == 0 {
		return mygo.Rectangle{}, mygo.Point{}, false
	}
	if ok, _, _ := cursorPosition.Call(uintptr(unsafe.Pointer(&p))); ok == 0 {
		return mygo.Rectangle{}, mygo.Point{}, false
	}
	return mygo.Rectangle{X: int(rect.left), Y: int(rect.top), Width: int(rect.right - rect.left), Height: int(rect.bottom - rect.top)}, mygo.Point{X: int(p.x), Y: int(p.y)}, true
}
