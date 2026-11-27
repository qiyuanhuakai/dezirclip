package main

import (
	"context"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

const mainShortcutSetting = "app.hotkey"

type nativeShortcutForm struct {
	received   bool
	key        string
	open       bool
	listening  bool
	focus      bool
	manual     bool
	rendered   bool
	focused    bool
	focusInRow bool
	typed      string
	cancel     context.CancelFunc
	captured   chan string
}

func (n *nativeMain) shortcutAvailable() bool {
	return n.session != nil && n.history.state.ServiceInstanceId != "" && !n.mutating &&
		n.shortcut == nil && n.privacyDialog == nil && n.backup == nil && !n.winV.changing
}

func (n *nativeMain) shortcutValue(key, fallback string) string {
	if value, stored := n.preferences[key]; stored {
		return value
	}
	return fallback
}

func (n *nativeMain) globalShortcutSettings(c *ui.Context) {
	label := n.text("global_hotkey")
	kit := n.mainKit()
	bindings := nativeMainChords(n.shortcutValue(mainShortcutSetting, ""))
	disabled := !n.shortcutAvailable()
	ui.Column(c).ColumnSpan(-1).Children(func() {
		kit.HotkeyListEditor(c, label, func() {
			n.shortcutBinding(c, kit, mainShortcutSetting, "+ "+label, "", "+ "+label)
		}, func() {
			n.shortcutControls(c, kit, mainShortcutSetting)
			kit.HotkeyListWithOptions(c, uikit.HotkeyListOptions{
				Values: bindings, Placeholder: n.text("not_set"), RemoveLabel: n.text("delete"),
				Disabled: disabled, CanRemove: n.canRemoveMainShortcut,
				OnRemove: func(chord string) {
					updated := slices.DeleteFunc(slices.Clone(bindings), func(value string) bool { return value == chord })
					n.saveNativeShortcut(mainShortcutSetting, strings.Join(updated, "\n"))
				},
			})
			kit.HotkeyError(c, n.shortcutFeedback(mainShortcutSetting))
		})
	})
	n.winVSettings(c, kit)
}

func nativeMainChords(raw string) []string {
	parts := strings.FieldsFunc(raw, func(r rune) bool { return r == '\r' || r == '\n' })
	for i, part := range parts {
		parts[i] = mainShortcutName(part)
	}
	return slices.DeleteFunc(parts, func(chord string) bool { return chord == "" })
}

func (n *nativeMain) shortcutSetting(c *ui.Context, key, label, fallback string) {
	n.shortcutSettingWithOptions(c, key, label, fallback, uikit.HotkeyRowOptions{})
}

func (n *nativeMain) shortcutSettingWithOptions(c *ui.Context, key, label, fallback string, options uikit.HotkeyRowOptions) {
	value := n.shortcutValue(key, fallback)
	options.Disabled = !n.shortcutAvailable()
	options.Error = n.shortcutFeedback(key)
	if value == "" {
		options.OnClear = nil
	}
	kit := n.mainKit()
	ui.Column(c).MinWidth(0).Children(func() {
		kit.HotkeyRowWithOptions(c, label, options, func() {
			n.shortcutBinding(c, kit, key, label, value, n.text("not_set"))
		})
		n.shortcutControls(c, kit, key)
	})
}

func (n *nativeMain) openShortcut(key string) {
	if !n.shortcutAvailable() || n.window == nil || !mainWindowFocused(n.window) {
		return
	}
	n.shortcutErrKey, n.shortcutErrText = "", ""
	session, state := n.session, n.history.state
	lifetime, cancel := context.WithTimeout(session, 45*time.Second)
	form := &nativeShortcutForm{
		key: key, open: true, focus: true, rendered: true, cancel: cancel, captured: make(chan string, 1),
	}
	n.shortcut = form
	go n.recordNativeShortcut(session, lifetime, state, form)
}

func (n *nativeMain) recordNativeShortcut(session, lifetime context.Context, state protocol.RuntimeState, form *nativeShortcutForm) {
	if lifetime.Err() != nil {
		n.finishNativeShortcut(session, lifetime, form, "", nil)
		return
	}
	// Complete both resident requests in order even if the window hides while
	// the first acknowledgement is in flight.
	err := n.shell.changeRecordingMode(context.Background(), true, state.PrivacyEpoch)
	var chord string
	if err == nil {
		n.update(session, func() {
			if n.shortcut == form && form.open && lifetime.Err() == nil {
				form.listening = true
				n.installShortcutInput()
			}
		})
		chord = n.waitForNativeShortcut(lifetime, form.captured)
	}
	stopErr := n.shell.changeRecordingMode(context.Background(), false, state.PrivacyEpoch)
	if err == nil {
		err = stopErr
	}
	n.finishNativeShortcut(session, lifetime, form, chord, err)
}

func (n *nativeMain) waitForNativeShortcut(ctx context.Context, captured <-chan string) string {
	// X11 focus events can precede the window manager's update of
	// _NET_ACTIVE_WINDOW. Recheck only during recording.
	focus := time.NewTicker(120 * time.Millisecond)
	defer focus.Stop()
	for {
		select {
		case chord := <-captured:
			return chord
		case <-ctx.Done():
			return ""
		case <-focus.C:
			if !mainWindowFocused(n.window) {
				return ""
			}
		}
	}
}

func (n *nativeMain) finishNativeShortcut(session, lifetime context.Context, form *nativeShortcutForm, chord string, err error) {
	n.window.Update(func() {
		if n.window.IsDestroyed() || n.shortcut != form {
			return
		}
		cancelled := lifetime.Err() != nil || !form.open || n.session != session
		form.cancel()
		n.shortcut = nil
		if cancelled || !mainWindowFocused(n.window) {
			return
		}
		if err != nil {
			n.reportShortcutError(form.key, n.shortcutError(err))
			return
		}
		if chord != "" {
			n.applyRecordedShortcut(form.key, chord)
		}
	})
}

func (n *nativeMain) cancelShortcut() {
	if form := n.shortcut; form != nil {
		form.open, form.listening = false, false
		form.cancel()
	}
}

func (n *nativeMain) receiveShortcut(event recordingEvent) {
	if form := n.shortcut; form != nil && form.open {
		if event.Cancelled {
			n.cancelShortcut()
			n.window.Invalidate()
		} else if !form.manual {
			n.captureNativeShortcut(form, event.Hotkey)
		}
	}
}

func (n *nativeMain) captureNativeShortcut(form *nativeShortcutForm, chord string) {
	if n.shortcut == form && form.open && form.listening && chord != "" {
		if !mainWindowFocused(n.window) {
			n.cancelShortcut()
			return
		}
		form.listening, form.received = false, true
		form.captured <- chord
	}
}

func (n *nativeMain) shortcutBinding(c *ui.Context, kit uikit.Kit, key, label, value, placeholder string) {
	form := n.shortcut
	active := form != nil && form.key == key && form.open
	if !active {
		if kit.HotkeyBinding(c, label, value, placeholder, false, !n.shortcutAvailable()).
			Tooltip(n.text("hotkey_click_hint")).Clicked() {
			n.openShortcut(key)
		}
		return
	}

	form.rendered = true
	if form.manual {
		n.drawTypedShortcut(c, kit, form)
		return
	}

	status := n.text("common_loading")
	if form.listening {
		status = n.text("waiting_for_input")
	}
	field := kit.HotkeyBinding(c, label, "", status, true, false).
		Tooltip(n.text("hotkey_recording_esc"))
	n.recordShortcutInput(field, form)
	form.focusInRow = field.Focused()
}

func (n *nativeMain) shortcutControls(c *ui.Context, kit uikit.Kit, key string) {
	form := n.shortcut
	if form == nil || form.key != key || !form.open {
		return
	}
	ui.Row(c).FillWidth().Gap(6).Wrap().Justify(ui.End).Children(func() {
		mode := n.text("native_shortcut_type")
		if form.manual {
			mode = n.text("native_shortcut_record")
			chord := typedShortcutChord(form.typed)
			save := kit.SettingsDataAction(c, n.text("common_save"), "", !form.listening || chord == "")
			form.focusInRow = form.focusInRow || save.Focused()
			if save.Clicked() {
				n.captureNativeShortcut(form, chord)
			}
		}
		switchMode := kit.SettingsDataAction(c, mode, "", !form.listening)
		cancel := kit.SettingsDataAction(c, n.text("common_cancel"), "", false)
		form.focusInRow = form.focusInRow || switchMode.Focused() || cancel.Focused()
		if switchMode.Clicked() {
			form.manual = !form.manual
			form.focused, form.focus = false, true
		}
		if cancel.Clicked() {
			n.cancelShortcut()
		}
	})
}

func (n *nativeMain) shortcutFeedback(key string) string {
	if n.shortcutErrKey == key {
		return n.shortcutErrText
	}
	return ""
}

func (n *nativeMain) reportShortcutError(key, message string) {
	n.shortcutErrKey, n.shortcutErrText = key, message
}

func (n *nativeMain) recordShortcutInput(field ui.Element, form *nativeShortcutForm) {
	field.HandleInput(func(event ui.InputEvent) bool {
		switch {
		case event.Kind == ui.InputKeyDown && event.Key == ui.KeyEscape && event.Mods == 0:
			n.cancelShortcut()
			return true
		case event.Kind == ui.InputKeyDown:
			if !event.Repeat {
				n.captureNativeShortcut(form, nativeShortcutChord(event.Key, event.Mods))
			}
			return true
		case event.Kind == ui.InputPointerDown && event.Button == 2 && event.Mods == 0 && form.key == mainShortcutSetting:
			n.captureNativeShortcut(form, "MouseMiddle")
			return true
		default:
			return false
		}
	})
	if form.focus {
		field.Focus()
		form.focus = false
	}
}

func (n *nativeMain) drawTypedShortcut(c *ui.Context, kit uikit.Kit, form *nativeShortcutForm) {
	field := kit.TextField(c, &form.typed, n.text("native_shortcut_input")).
		MinWidth(136).Placeholder("Ctrl+Alt+K").HandleInput(func(event ui.InputEvent) bool {
		if event.Kind == ui.InputKeyDown && event.Key == ui.KeyEscape && event.Mods == 0 {
			n.cancelShortcut()
			return true
		}
		return false
	})
	if form.focus {
		field.Focus()
		form.focus = false
	}
	form.focusInRow = field.Focused()
	if field.Submitted() {
		n.captureNativeShortcut(form, typedShortcutChord(form.typed))
	}
}

func typedShortcutChord(value string) string {
	if len(value) > 128 || strings.ContainsAny(value, "\r\n\x00") {
		return ""
	}
	return mainShortcutName(value)
}

func nativeShortcutChord(key ui.Key, mods ui.Modifiers) string {
	var name string
	switch {
	case key >= ui.KeyA && key <= ui.KeyZ:
		name = string(rune('A' + key - ui.KeyA))
	case key >= ui.Key0 && key <= ui.Key9:
		name = string(rune('0' + key - ui.Key0))
	case key >= ui.KeyF1 && key <= ui.KeyF12:
		name = "F" + strconv.Itoa(int(key-ui.KeyF1)+1)
	default:
		name = map[ui.Key]string{
			ui.KeyEnter: "Enter", ui.KeyBackspace: "Backspace", ui.KeyTab: "Tab", ui.KeySpace: "Space",
			ui.KeyDelete: "Delete", ui.KeyInsert: "Insert", ui.KeyHome: "Home", ui.KeyEnd: "End",
			ui.KeyPageUp: "PageUp", ui.KeyPageDown: "PageDown", ui.KeyLeft: "Left", ui.KeyRight: "Right",
			ui.KeyUp: "Up", ui.KeyDown: "Down", ui.KeyEscape: "Escape",
			ui.KeyBackquote: "Backquote", ui.KeyMinus: "Minus", ui.KeyEqual: "Equal",
			ui.KeyBracketLeft: "BracketLeft", ui.KeyBracketRight: "BracketRight", ui.KeyBackslash: "Backslash",
			ui.KeySemicolon: "Semicolon", ui.KeyQuote: "Quote", ui.KeyComma: "Comma", ui.KeyPeriod: "Period",
			ui.KeySlash: "Slash", ui.KeyContextMenu: "ContextMenu",
		}[key]
	}
	if name == "" || (name == "Escape" && mods == 0) {
		return ""
	}
	return shortcutChord(name, mods)
}

func shortcutChord(name string, mods ui.Modifiers) string {
	var keys []string
	for _, modifier := range []struct {
		code ui.Modifiers
		name string
	}{{ui.Ctrl, "Ctrl"}, {ui.Alt, "Alt"}, {ui.Shift, "Shift"}, {ui.Super, "Super"}} {
		if mods&modifier.code != 0 {
			keys = append(keys, modifier.name)
		}
	}
	return strings.Join(append(keys, name), "+")
}

func (n *nativeMain) applyRecordedShortcut(key, recorded string) {
	chord := mainShortcutName(recorded)
	if key != mainShortcutSetting {
		if chord == "MouseMiddle" {
			n.reportShortcutError(key, n.text("hotkey_unavailable"))
			return
		}
		if chord != mainShortcutName(n.shortcutValue(key, "")) {
			n.saveNativeShortcut(key, chord)
		}
		return
	}
	chords := nativeMainChords(n.shortcutValue(key, ""))
	if slices.Contains(chords, chord) {
		n.reportShortcutError(key, n.text("hotkey_unavailable"))
		return
	}
	n.saveNativeShortcut(key, strings.Join(append(chords, chord), "\n"))
}

func (n *nativeMain) shortcutError(err error) string {
	reason := strings.ToLower(err.Error())
	if strings.Contains(reason, "busy") || strings.Contains(reason, "already") ||
		strings.Contains(reason, "conflict") || strings.Contains(reason, "in use") {
		return n.text("privacy_hotkey_busy")
	}
	return n.text("hotkey_unavailable")
}

func (n *nativeMain) saveNativeShortcut(key, value string) {
	if !n.shortcutAvailable() {
		return
	}
	session, state := n.session, n.history.state
	n.mutating = true
	n.shortcutErrKey, n.shortcutErrText = "", ""
	go func() {
		ctx, cancel := context.WithTimeout(session, 15*time.Second)
		defer cancel()
		var err error
		switch key {
		case mainShortcutSetting:
			err = n.shell.SetMainHotkeys(ctx, value, state.PrivacyEpoch)
		case "app.search_hotkey":
			err = n.shell.SetSearchHotkey(ctx, value, state.PrivacyEpoch)
		case "app.sequential_hotkey", "app.rich_paste_hotkey":
			err = n.shell.SetPasteHotkey(ctx, key, value, state.PrivacyEpoch)
		case "app.privacy_session_hotkey":
			_, err = n.library.SetPrivacyHotkey(ctx, value)
		default:
			err = n.shell.SetPanelHotkey(ctx, key, value, state.PrivacyEpoch)
		}
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.reportShortcutError(key, n.shortcutError(err))
				return
			}
			n.cancelShortcut()
			n.refreshViewSettings()
		})
	}()
}
