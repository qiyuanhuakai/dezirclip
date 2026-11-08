package main

import (
	"errors"
	"strings"

	"github.com/egoist/mygo"
)

// Run on the resident UI thread, which owns every desktop global shortcut.
func testNativeHotkey(hotkey string) error {
	if hotkey == "" {
		return nil
	}
	if len(hotkey) > 128 || strings.ContainsAny(hotkey, "\r\n\x00") {
		return errors.New("InvalidShortcut")
	}
	// Register rejects an existing local claim without changing its callback.
	// Only a claim created by this probe may be released here.
	if err := mygo.GlobalShortcut.Register(hotkey, func() {}); err != nil {
		return err
	}
	mygo.GlobalShortcut.Unregister(hotkey)
	return nil
}
