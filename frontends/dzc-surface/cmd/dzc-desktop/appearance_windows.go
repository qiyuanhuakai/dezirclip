package main

import (
	"context"
	"unsafe"

	"github.com/egoist/mygo"
	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

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
