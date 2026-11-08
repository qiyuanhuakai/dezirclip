package main

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo"
)

func mainShortcutName(key string) string {
	key = strings.TrimSpace(key)
	if strings.EqualFold(key, "MouseMiddle") || strings.EqualFold(key, "MButton") {
		return "MouseMiddle"
	}
	// Match dzc-platform's persisted chord identity, including aliases and order.
	modifiers := map[string]bool{}
	selected := ""
	for _, part := range strings.Split(key, "+") {
		part = strings.ToLower(strings.TrimSpace(part))
		modifier := ""
		switch part {
		case "ctrl", "control":
			modifier = "Ctrl"
		case "alt":
			modifier = "Alt"
		case "shift":
			modifier = "Shift"
		case "super", "meta", "win", "windows", "cmd", "command":
			modifier = "Super"
		}
		if modifier != "" {
			if modifiers[modifier] {
				return key
			}
			modifiers[modifier] = true
			continue
		}
		if selected != "" {
			return key
		}
		aliases := map[string]string{
			"space": "Space", "tab": "Tab", "enter": "Enter", "return": "Enter",
			"esc": "Escape", "escape": "Escape", "backspace": "Backspace",
			"delete": "Delete", "del": "Delete", "insert": "Insert", "ins": "Insert",
			"home": "Home", "end": "End", "pageup": "PageUp", "pagedown": "PageDown",
			"left": "Left", "arrowleft": "Left", "right": "Right", "arrowright": "Right",
			"up": "Up", "arrowup": "Up", "down": "Down", "arrowdown": "Down",
		}
		selected = aliases[part]
		if len(part) == 1 && ((part[0] >= 'a' && part[0] <= 'z') || (part[0] >= '0' && part[0] <= '9')) {
			selected = strings.ToUpper(part)
		}
		if strings.HasPrefix(part, "f") {
			if n, err := strconv.Atoi(part[1:]); err == nil && n >= 1 && n <= 24 && part == "f"+strconv.Itoa(n) {
				selected = strings.ToUpper(part)
			}
		}
		if selected == "" {
			return key
		}
	}
	if selected == "" {
		return key
	}
	parts := []string{}
	for _, modifier := range []string{"Ctrl", "Alt", "Shift", "Super"} {
		if modifiers[modifier] {
			parts = append(parts, modifier)
		}
	}
	return strings.Join(append(parts, selected), "+")
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
