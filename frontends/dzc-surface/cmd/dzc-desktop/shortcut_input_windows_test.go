package main

import (
	"os"
	"testing"
)

func TestWindowsExtendedShortcutKeyMapping(t *testing.T) {
	for key, want := range map[uintptr]string{
		0x31: "", 0x61: "Numpad1", 0x6b: "NumpadAdd", 0x7c: "F13",
		0x2c: "PrintScreen", 0x90: "NumLock", 0x13: "Pause", 0x5d: "ContextMenu",
		0x6c: "NumpadSeparator", 0x92: "NumpadEqual", 0xe2: "IntlBackslash",
	} {
		if got := windowsExtendedShortcutKey(key); got != want {
			t.Errorf("virtual key %#x = %q, want %q", key, got, want)
		}
	}
}

func TestWindowsExtendedShortcutRegistration(t *testing.T) {
	if os.Getenv("DEZIRCLIP_RUN_WINDOWS_EXTENDED_SHORTCUT_SMOKE") != "1" {
		t.Skip("requires a Windows desktop session")
	}
	for _, chord := range []string{
		"Ctrl+Alt+Shift+Pause", "Ctrl+Alt+Shift+ContextMenu", "Ctrl+Alt+Shift+IntlBackslash",
		"Ctrl+Alt+Shift+NumpadEqual", "Ctrl+Alt+Shift+NumpadSeparator",
	} {
		// Register briefly without sending physical input or changing system settings.
		for i := 0; i < 2; i++ {
			handled, err := registerExtraShortcut(chord, func() {})
			if !handled || err != nil {
				t.Fatalf("cannot register %s: %v", chord, err)
			}
			if _, err := registerExtraShortcut(chord, func() {}); err == nil {
				unregisterExtraShortcut(chord)
				t.Fatalf("duplicate registration of %s succeeded", chord)
			}
			if !unregisterExtraShortcut(chord) {
				t.Fatalf("cannot release %s", chord)
			}
		}
	}
}
