package main

import (
	"strings"

	"github.com/egoist/mygo"
)

func mainShortcutName(key string) string {
	key = strings.TrimSpace(key)
	if strings.EqualFold(key, "MouseMiddle") || strings.EqualFold(key, "MButton") {
		return "MouseMiddle"
	}
	return key
}

func registerMainShortcut(key string, pressed func()) error {
	if mainShortcutName(key) == "MouseMiddle" {
		return registerMiddleShortcut(pressed)
	}
	return mygo.GlobalShortcut.Register(key, pressed)
}

func unregisterMainShortcut(key string) {
	if mainShortcutName(key) == "MouseMiddle" {
		unregisterMiddleShortcut()
		return
	}
	mygo.GlobalShortcut.Unregister(key)
}
