package main

import (
	"strconv"
	"syscall"

	"github.com/egoist/mygo/ui"
	"golang.org/x/sys/windows"
)

var (
	shortcutUser32         = windows.NewLazySystemDLL("user32.dll")
	shortcutComctl         = windows.NewLazySystemDLL("comctl32.dll")
	shortcutGetFocus       = shortcutUser32.NewProc("GetFocus")
	shortcutIsChild        = shortcutUser32.NewProc("IsChild")
	shortcutKeyState       = shortcutUser32.NewProc("GetKeyState")
	shortcutSetSubclass    = shortcutComctl.NewProc("SetWindowSubclass")
	shortcutRemoveSubclass = shortcutComctl.NewProc("RemoveWindowSubclass")
	shortcutDefSubclass    = shortcutComctl.NewProc("DefSubclassProc")
	shortcutInputWindows   = map[uintptr]*nativeMain{} // Renderer UI thread.
)

const shortcutSubclassID = 0x445a43

func (n *nativeMain) installShortcutInput() {
	window := n.window.NativeHandle()
	input, _, _ := shortcutGetFocus.Call()
	if input != window {
		if child, _, _ := shortcutIsChild.Call(window, input); child == 0 {
			return
		}
	}
	if ok, _, _ := shortcutSetSubclass.Call(input, shortcutInputCallback, shortcutSubclassID, 0); ok != 0 {
		shortcutInputWindows[input] = n
	}
}

var shortcutInputCallback uintptr

func init() {
	shortcutInputCallback = syscall.NewCallback(dispatchShortcutInput)
}

func dispatchShortcutInput(window uintptr, message uint32, key, data, id, _ uintptr) uintptr {
	const (
		ncDestroy     = 0x82
		keyDown       = 0x100
		keyUp         = 0x101
		systemKeyDown = 0x104
		systemKeyUp   = 0x105
		printScreen   = 0x2c
	)
	if message == ncDestroy {
		delete(shortcutInputWindows, window)
		shortcutRemoveSubclass.Call(window, shortcutInputCallback, id)
	}
	initialPress := (message == keyDown || message == systemKeyDown) && data&(1<<30) == 0
	printScreenRelease := key == printScreen && (message == keyUp || message == systemKeyUp)
	if n := shortcutInputWindows[window]; n != nil && (initialPress || printScreenRelease) {
		if form := n.shortcut; form != nil && form.open && form.listening && !form.manual && mainWindowFocused(n.window) {
			if name := windowsExtendedShortcutKey(key); name != "" {
				n.captureNativeShortcut(form, shortcutChord(name, windowsShortcutModifiers()))
				return 0
			}
		}
	}
	result, _, _ := shortcutDefSubclass.Call(window, uintptr(message), key, data)
	return result
}

func windowsExtendedShortcutKey(key uintptr) string {
	switch {
	case key >= 0x60 && key <= 0x69:
		return "Numpad" + strconv.Itoa(int(key-0x60))
	case key >= 0x7c && key <= 0x87:
		return "F" + strconv.Itoa(int(key-0x6f))
	default:
		return map[uintptr]string{
			0x13: "Pause", 0x14: "CapsLock", 0x2c: "PrintScreen", 0x5d: "ContextMenu",
			0x90: "NumLock", 0x91: "ScrollLock",
			0x6a: "NumpadMultiply", 0x6b: "NumpadAdd", 0x6c: "NumpadSeparator", 0x6d: "NumpadSubtract",
			0x6e: "NumpadDecimal", 0x6f: "NumpadDivide",
			0x92: "NumpadEqual", 0xe2: "IntlBackslash",
			0xad: "VolumeMute", 0xae: "VolumeDown", 0xaf: "VolumeUp", 0xb0: "MediaTrackNext",
			0xb1: "MediaTrackPrevious", 0xb2: "MediaStop", 0xb3: "MediaPlayPause",
		}[key]
	}
}

func windowsShortcutModifiers() ui.Modifiers {
	var modifiers ui.Modifiers
	for _, binding := range []struct {
		key      uintptr
		modifier ui.Modifiers
	}{{0x10, ui.Shift}, {0x11, ui.Ctrl}, {0x12, ui.Alt}, {0x5b, ui.Super}, {0x5c, ui.Super}} {
		if state, _, _ := shortcutKeyState.Call(binding.key); state&0x8000 != 0 {
			modifiers |= binding.modifier
		}
	}
	return modifiers
}
