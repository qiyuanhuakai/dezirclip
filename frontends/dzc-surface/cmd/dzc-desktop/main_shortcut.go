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
		selected = shortcutKeyNames[part]
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

var shortcutKeyNames = map[string]string{
	"space":              "Space",
	"tab":                "Tab",
	"enter":              "Enter",
	"return":             "Enter",
	"escape":             "Escape",
	"esc":                "Escape",
	"backspace":          "Backspace",
	"delete":             "Delete",
	"del":                "Delete",
	"insert":             "Insert",
	"ins":                "Insert",
	"home":               "Home",
	"end":                "End",
	"pageup":             "PageUp",
	"pagedown":           "PageDown",
	"left":               "Left",
	"arrowleft":          "Left",
	"right":              "Right",
	"arrowright":         "Right",
	"up":                 "Up",
	"arrowup":            "Up",
	"down":               "Down",
	"arrowdown":          "Down",
	"backquote":          "Backquote",
	"`":                  "Backquote",
	"minus":              "Minus",
	"-":                  "Minus",
	"equal":              "Equal",
	"=":                  "Equal",
	"plus":               "Equal",
	"bracketleft":        "BracketLeft",
	"[":                  "BracketLeft",
	"bracketright":       "BracketRight",
	"]":                  "BracketRight",
	"backslash":          "Backslash",
	"\\":                 "Backslash",
	"semicolon":          "Semicolon",
	";":                  "Semicolon",
	"quote":              "Quote",
	"'":                  "Quote",
	"comma":              "Comma",
	",":                  "Comma",
	"period":             "Period",
	".":                  "Period",
	"slash":              "Slash",
	"/":                  "Slash",
	"capslock":           "CapsLock",
	"numlock":            "NumLock",
	"scrolllock":         "ScrollLock",
	"printscreen":        "PrintScreen",
	"pause":              "Pause",
	"pausebreak":         "Pause",
	"contextmenu":        "ContextMenu",
	"apps":               "ContextMenu",
	"numpad0":            "Numpad0",
	"num0":               "Numpad0",
	"numpad1":            "Numpad1",
	"num1":               "Numpad1",
	"numpad2":            "Numpad2",
	"num2":               "Numpad2",
	"numpad3":            "Numpad3",
	"num3":               "Numpad3",
	"numpad4":            "Numpad4",
	"num4":               "Numpad4",
	"numpad5":            "Numpad5",
	"num5":               "Numpad5",
	"numpad6":            "Numpad6",
	"num6":               "Numpad6",
	"numpad7":            "Numpad7",
	"num7":               "Numpad7",
	"numpad8":            "Numpad8",
	"num8":               "Numpad8",
	"numpad9":            "Numpad9",
	"num9":               "Numpad9",
	"numpadadd":          "NumpadAdd",
	"numadd":             "NumpadAdd",
	"numpadplus":         "NumpadAdd",
	"numplus":            "NumpadAdd",
	"numpadsubtract":     "NumpadSubtract",
	"numsub":             "NumpadSubtract",
	"numsubtract":        "NumpadSubtract",
	"numpadmultiply":     "NumpadMultiply",
	"nummult":            "NumpadMultiply",
	"nummultiply":        "NumpadMultiply",
	"numpaddivide":       "NumpadDivide",
	"numdiv":             "NumpadDivide",
	"numdivide":          "NumpadDivide",
	"numpaddecimal":      "NumpadDecimal",
	"numdec":             "NumpadDecimal",
	"numdecimal":         "NumpadDecimal",
	"numpadenter":        "NumpadEnter",
	"numenter":           "NumpadEnter",
	"numpadequal":        "NumpadEqual",
	"numequal":           "NumpadEqual",
	"volumemute":         "VolumeMute",
	"audiovolumemute":    "VolumeMute",
	"volumedown":         "VolumeDown",
	"audiovolumedown":    "VolumeDown",
	"volumeup":           "VolumeUp",
	"audiovolumeup":      "VolumeUp",
	"mediatracknext":     "MediaTrackNext",
	"medianexttrack":     "MediaTrackNext",
	"mediatrackprevious": "MediaTrackPrevious",
	"mediaprevioustrack": "MediaTrackPrevious",
	"mediatrackprev":     "MediaTrackPrevious",
	"mediastop":          "MediaStop",
	"mediaplaypause":     "MediaPlayPause",
}

var mygoShortcutKeyNames = map[string]string{
	"Backquote": "`", "Minus": "-", "Equal": "=", "BracketLeft": "[", "BracketRight": "]", "Backslash": "\\",
	"Semicolon": ";", "Quote": "'", "Comma": ",", "Period": ".", "Slash": "/",
	"NumpadAdd": "NumAdd", "NumpadSubtract": "NumSub", "NumpadMultiply": "NumMult",
	"NumpadDivide": "NumDiv", "NumpadDecimal": "NumDec",
	"MediaTrackNext": "MediaNextTrack", "MediaTrackPrevious": "MediaPreviousTrack",
}

func mygoShortcutName(raw string) string {
	key := mainShortcutName(raw)
	modifiers := ""
	if prefix, last, found := strings.CutLast(key, "+"); found {
		modifiers, key = prefix+"+", last
	}
	if keyName := mygoShortcutKeyNames[key]; keyName != "" {
		key = keyName
	}
	if strings.HasPrefix(key, "Numpad") && len(key) == 7 && key[6] >= '0' && key[6] <= '9' {
		key = "Num" + key[6:]
	}
	return modifiers + key
}

func registerMainShortcut(key string, pressed func()) error {
	key = mainShortcutName(key)
	if key == "MouseMiddle" {
		return registerMiddleShortcut(pressed)
	}
	if handled, err := registerExtraShortcut(key, pressed); handled {
		return err
	}
	return mygo.GlobalShortcut.Register(mygoShortcutName(key), pressed)
}

func unregisterMainShortcut(key string) {
	key = mainShortcutName(key)
	if key == "MouseMiddle" {
		unregisterMiddleShortcut()
		return
	}
	if !unregisterExtraShortcut(key) {
		mygo.GlobalShortcut.Unregister(mygoShortcutName(key))
	}
}
